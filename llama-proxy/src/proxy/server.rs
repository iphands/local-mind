//! Main proxy server implementation

use axum::{
    extract::State,
    http::HeaderValue,
    routing::{any, get},
    Router,
};
use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::trace::TraceLayer;

use super::handler::ProxyHandler;
use super::reprompt::RepromptEngine;
use crate::augment::AugmentBackend;
use crate::backends::{build_balancer_from_groups, build_balancer_from_single, preflight, GroupedLoadBalancer, LoadBalancer};
use crate::config::AppConfig;
use crate::exporters::ExporterManager;
use crate::fixes::FixRegistry;

/// Shared state for the proxy
#[derive(Clone)]
pub struct ProxyState {
    pub config: Arc<AppConfig>,
    pub load_balancer: Arc<dyn LoadBalancer>,
    pub fix_registry: Arc<FixRegistry>,
    pub exporter_manager: Arc<ExporterManager>,
    pub augment_backend: Option<Arc<AugmentBackend>>,
    pub reprompt_engine: Option<Arc<RepromptEngine>>,
    pub hide_requests: bool,
    pub log_augmented_request_text: bool,
    pub dump_path: Option<Arc<std::path::PathBuf>>,
    pub concurrent_requests: Arc<AtomicUsize>,

    /// Counter for when backend returns streaming despite stream:false
    pub backend_streaming_fallback_hits: Arc<AtomicUsize>,

    /// Requests routed to the backend with stream:true intact (passthrough decision).
    /// Counted at the routing decision so it moves even with stats.enabled: false -
    /// unlike the passthrough_* ratios, which need accumulation to be computed.
    pub openai_stream_passthrough_total: Arc<AtomicU64>,

    /// Passthrough asked the backend for stream:true and got a JSON body back instead
    /// of text/event-stream. Names the backend as the limiter, not the proxy.
    pub backend_nonsse_when_streamed_for: Arc<AtomicU64>,

    /// Anthropic /v1/messages responses served buffered + synthesized while the mode is
    /// passthrough. The per-request notice fires once per process, so this is the only
    /// signal left after the first one.
    pub anthropic_buffered_responses_total: Arc<AtomicU64>,

    /// Fires the "passthrough does not apply here" notice on the first /v1/messages only.
    pub anthropic_buffered_notice_once: Arc<AtomicBool>,

    /// Counter for rejected requests at capacity
    pub rejected_requests: Arc<AtomicUsize>,

    /// Semaphore for enforcing concurrent request limit
    pub concurrent_semaphore: Option<Arc<Semaphore>>,
}

/// Run the proxy server
pub async fn run_server(
    config: AppConfig,
    fix_registry: FixRegistry,
    exporter_manager: ExporterManager,
    hide_requests: bool,
    log_augmented_request_text: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = config;

    // Build load balancer from configuration
    let load_balancer = if let Some(ref mut backends) = config.backends {
        // Multi-backend mode with named groups
        tracing::info!("Using multi-backend mode with {} group(s)", backends.len());

        for (name, group) in backends.iter() {
            let mapping_str = if group.mappings.is_empty() {
                "catch-all".to_string()
            } else {
                format!("{:?}", group.mappings)
            };
            tracing::info!(
                group = %name,
                mappings = %mapping_str,
                strategy = %group.strategy,
                node_count = group.nodes.len(),
                "Backend group configured"
            );
        }

        // Run preflight: discover models, auto-set overrides, warn on ambiguity
        preflight::run_preflight_multi(backends).await;

        build_balancer_from_groups(backends.clone())?
    } else if let Some(ref backend) = config.backend {
        // Single backend mode (backward compatibility)
        tracing::info!(
            url = %backend.base_url(),
            timeout_seconds = backend.timeout_seconds,
            "Using single backend mode"
        );

        preflight::run_preflight_single(backend).await;

        build_balancer_from_single(
            backend.url.clone(),
            backend.timeout_seconds,
            backend.tls.as_ref(),
            backend.model.clone(),
            backend.api_key.clone(),
            backend.strip_path_prefix.clone(),
        )?
    } else {
        // F-H1: neither `backend:` nor `backends:` — start with a zero-group
        // grouped balancer so every completion request hits the task-4
        // NoMatchingBackend → 503 envelope instead of failing at startup.
        tracing::warn!(
            "No backend configured — every completion request will answer 503 \
             until `backend:` or `backends:` is set"
        );
        Arc::new(GroupedLoadBalancer::new(std::collections::HashMap::new())?)
    };

    tracing::info!(strategy = %load_balancer.strategy_name(), "Load balancing strategy");

    // Initialize augment backend if configured
    let augment_backend = if let Some(ref augment_config) = config.augment_backend {
        if augment_config.enabled {
            match AugmentBackend::from_config(augment_config) {
                Ok(backend) => {
                    tracing::info!(
                        url = %augment_config.url,
                        model = %augment_config.model,
                        prompt_file = %augment_config.prompt_file,
                        "Augment backend enabled"
                    );
                    Some(Arc::new(backend))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to initialize augment backend, will be disabled");
                    None
                }
            }
        } else {
            tracing::info!("Augment backend disabled via config");
            None
        }
    } else {
        None
    };

    // Initialize reprompt engine if configured
    let reprompt_engine = if let Some(ref cfg) = config.reprompt {
        if cfg.enabled {
            match RepromptEngine::from_config(cfg) {
                Ok(engine) => {
                    tracing::info!(
                        max_retries = cfg.max_retries,
                        done_sentinels = ?cfg.done_sentinels,
                        has_prompt_file = cfg.prompt_file.is_some(),
                        "Reprompt engine enabled"
                    );
                    Some(Arc::new(engine))
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to initialize reprompt engine — disabled");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    let state = ProxyState {
        config: Arc::new(config.clone()),
        load_balancer,
        fix_registry: Arc::new(fix_registry),
        exporter_manager: Arc::new(exporter_manager),
        augment_backend,
        reprompt_engine,
        hide_requests,
        log_augmented_request_text,
        dump_path: if config.dump.enabled && !config.dump.path.is_empty() {
            Some(Arc::new(std::path::PathBuf::from(&config.dump.path)))
        } else {
            None
        },
        concurrent_requests: Arc::new(AtomicUsize::new(0)),
        backend_streaming_fallback_hits: Arc::new(AtomicUsize::new(0)),
        openai_stream_passthrough_total: Arc::new(AtomicU64::new(0)),
        backend_nonsse_when_streamed_for: Arc::new(AtomicU64::new(0)),
        anthropic_buffered_responses_total: Arc::new(AtomicU64::new(0)),
        anthropic_buffered_notice_once: Arc::new(AtomicBool::new(false)),
        rejected_requests: Arc::new(AtomicUsize::new(0)),
        concurrent_semaphore: if config.server.max_concurrent_requests > 0 {
            Some(Arc::new(Semaphore::new(config.server.max_concurrent_requests)))
        } else {
            None // Unlimited
        },
    };

    // Build the router
    let app = Router::new()
        // Health check
        .route("/health", get(health_handler))
        // Catch-all proxy routes
        .route("/v1/*path", any(proxy_handler))
        .route("/*path", any(proxy_handler))
        .fallback(proxy_handler_fallback)
        .layer(build_cors_layer(&config.server.allowed_origins))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = resolve_bind_addr(&config.server.host, config.server.port)?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("llama-proxy listening on {}", addr);

    GracefulServe {
        listener,
        app,
        shutdown: shutdown_signal(),
        grace: GRACEFUL_SHUTDOWN_GRACE,
        drain_started: None,
    }
    .run()
    .await;
    Ok(())
}

/// Health check endpoint
async fn health_handler() -> &'static str {
    "OK"
}

/// Resolve `server.host` + `server.port` into the bind address, bracket-
/// tolerant for IPv6.
///
/// Operators copy both shapes from URLs: `::1` and `[::1]`. The `SocketAddr`
/// parser only accepts the bracketed form once a port is appended, so a bare
/// IPv6 host is re-bracketed here; the config loader (task 71) already accepts
/// both, and whatever it accepts must bind. Zone ids (`fe80::1%eth0`) never
/// pass the loader and are refused here again rather than silently bound to
/// some other address.
fn resolve_bind_addr(host: &str, port: u16) -> Result<SocketAddr, std::net::AddrParseError> {
    let bare = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    let bracketed = match bare.parse::<std::net::Ipv6Addr>() {
        Ok(ipv6) => format!("[{ipv6}]"),
        Err(_) => bare.to_string(),
    };
    format!("{bracketed}:{port}").parse()
}

/// Cap on the post-signal drain (D11). A hung client cannot extend it: when
/// the grace expires the remaining connections are force-closed, so the
/// process exits within `grace` of the signal, always.
pub(crate) const GRACEFUL_SHUTDOWN_GRACE: Duration = Duration::from_secs(30);

/// The serve loop with task 78's three-tier shutdown:
/// 1. signal → stop accepting; the listener is dropped, so new connections are
///    REFUSED (backlog RST, later SYNs refused), not merely unanswered;
/// 2. every live connection is asked to stop after its current response
///    (hyper graceful: in-flight bodies COMPLETE, idle keep-alives close now);
/// 3. when the grace expires the stragglers are force-closed.
///
/// This replaces a bare `axum::serve`, whose SIGTERM behavior was instant
/// process death: a streamed client died mid-body (RED: curl exit 18).
struct GracefulServe<S> {
    listener: tokio::net::TcpListener,
    app: Router,
    shutdown: S,
    grace: Duration,
    /// Fired once the listener has stopped accepting. Test seam for observing
    /// drain-start without sleeps; production passes `None`.
    drain_started: Option<tokio::sync::oneshot::Sender<()>>,
}

impl<S: Future<Output = ()>> GracefulServe<S> {
    async fn run(self) {
        let Self {
            listener,
            app,
            shutdown,
            grace,
            drain_started,
        } = self;
        tokio::pin!(shutdown);
        // Live connections learn the drain started when the sender is dropped.
        let (drain_tx, drain_rx) = tokio::sync::watch::channel(());
        let mut conns: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();

        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown => break,
                Some(_) = conns.join_next(), if !conns.is_empty() => {}
                accepted = listener.accept() => match accepted {
                    Ok((sock, peer)) => {
                        let app = app.clone();
                        let mut drained = drain_rx.clone();
                        conns.spawn(async move {
                            let svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                let router = app.clone();
                                async move {
                                    tower::ServiceExt::oneshot(router, req.map(axum::body::Body::new)).await
                                }
                            });
                            let builder = hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new());
                            let conn = builder.serve_connection_with_upgrades(hyper_util::rt::TokioIo::new(sock), svc);
                            tokio::pin!(conn);
                            tokio::select! {
                                biased;
                                r = &mut conn => {
                                    if let Err(e) = r {
                                        tracing::debug!(%peer, error = %e, "connection ended with error");
                                    }
                                }
                                _ = drained.changed() => {
                                    conn.as_mut().graceful_shutdown();
                                    if let Err(e) = conn.await {
                                        tracing::debug!(%peer, error = %e, "connection closed during drain");
                                    }
                                }
                            }
                        });
                    }
                    Err(e) => tracing::warn!(error = %e, "accept failed; continuing"),
                }
            }
        }

        // Drain mode: refusing new connections starts HERE, synchronously.
        drop(listener);
        // In-flight bodies finish; idle keep-alive connections close now.
        drop(drain_tx);
        if let Some(tx) = drain_started {
            let _ = tx.send(());
        }
        if conns.is_empty() {
            return;
        }
        tracing::info!(
            connections = conns.len(),
            ?grace,
            "shutdown signal: draining in-flight connections"
        );
        let deadline = tokio::time::sleep(grace);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                biased;
                Some(_) = conns.join_next() => {
                    if conns.is_empty() {
                        tracing::info!("all in-flight connections drained");
                        return;
                    }
                }
                _ = &mut deadline => {
                    tracing::warn!(remaining = conns.len(), "grace exhausted: force-closing the remaining in-flight connections");
                    drop(conns);
                    return;
                }
            }
        }
    }
}

/// Waits for the deploy-stop signal: SIGTERM (systemd/docker stop) or Ctrl-C.
/// A handler that could not be installed is reported and degrades to the
/// remaining signals instead of pretending shutdown will work.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %e, "Ctrl-C handler unavailable; Ctrl-C will not shut down");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "SIGTERM handler unavailable; SIGTERM will not shut down");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("Ctrl-C received"),
        _ = terminate => tracing::info!("SIGTERM received"),
    }
}

/// Build the CORS layer from `server.allowed_origins`.
///
/// `None` keeps the historical permissive default: `Access-Control-Allow-Origin: *`
/// for every origin. `Some(list)` echoes back only exact matches and nothing at
/// all for any other origin. The loader validated the entries; this still drops
/// (with a WARN) any entry that fails `HeaderValue` parsing or is `*` —
/// `AllowOrigin::list` panics on a wildcard, and a directly-constructed config
/// must not be able to crash the server. Dropping is fail-CLOSED: fewer echoed
/// origins, never more.
fn build_cors_layer(allowed_origins: &Option<Vec<String>>) -> CorsLayer {
    let layer = CorsLayer::new().allow_methods(Any).allow_headers(Any);
    match allowed_origins {
        None => layer.allow_origin(Any),
        Some(list) => {
            let mut values: Vec<HeaderValue> = Vec::with_capacity(list.len());
            for entry in list {
                if entry == "*" {
                    tracing::warn!("allowed_origins entry '*' ignored: exact-match list cannot carry a wildcard (omit allowed_origins to allow all)");
                    continue;
                }
                match HeaderValue::from_str(entry) {
                    Ok(value) => values.push(value),
                    Err(e) => {
                        tracing::warn!(entry = ?entry, error = %e, "allowed_origins entry is not a valid header value; ignored")
                    }
                }
            }
            layer.allow_origin(AllowOrigin::list(values))
        }
    }
}

/// Main proxy handler for matched routes
async fn proxy_handler(State(state): State<ProxyState>, req: axum::extract::Request) -> axum::response::Response {
    let handler = ProxyHandler::new(state);
    handler.handle(req).await
}

/// Fallback handler for unmatched routes
async fn proxy_handler_fallback(State(state): State<ProxyState>, req: axum::extract::Request) -> axum::response::Response {
    let handler = ProxyHandler::new(state);
    handler.handle(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// Serves the SAME router shape the proxy builds (health route + the CORS layer
    /// under test) on an ephemeral port, so header behavior is proven on the wire.
    async fn spawn_cors_server(
        allowed: Option<Vec<String>>,
    ) -> (reqwest::Client, std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let app = Router::new()
            .route("/health", get(health_handler))
            .layer(build_cors_layer(&allowed));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        (reqwest::Client::new(), addr, handle)
    }

    async fn get_with_origin(client: &reqwest::Client, addr: std::net::SocketAddr, origin: &str) -> reqwest::Response {
        client
            .get(format!("http://{addr}/health"))
            .header("Origin", origin)
            .send()
            .await
            .expect("request")
    }

    #[tokio::test]
    async fn t74_none_keeps_permissive_wildcard_verbatim() {
        // Given: allowed_origins absent (every config written before task 74)
        let (client, addr, handle) = spawn_cors_server(None).await;
        // When: any origin asks
        let resp = get_with_origin(&client, addr, "https://evil.example").await;
        // Then: the historical `*` echo is unchanged, byte for byte
        assert_eq!(
            resp.headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("*")
        );
        let pre = client
            .request(reqwest::Method::OPTIONS, format!("http://{addr}/health"))
            .header("Origin", "https://anything.at.all")
            .header("Access-Control-Request-Method", "POST")
            .send()
            .await
            .expect("preflight");
        assert_eq!(
            pre.headers().get("access-control-allow-origin").and_then(|v| v.to_str().ok()),
            Some("*")
        );
        assert_eq!(
            pre.headers()
                .get("access-control-allow-methods")
                .and_then(|v| v.to_str().ok()),
            Some("*")
        );
        assert_eq!(
            pre.headers()
                .get("access-control-allow-headers")
                .and_then(|v| v.to_str().ok()),
            Some("*")
        );
        handle.abort();
    }

    #[tokio::test]
    async fn t74_listed_origin_gets_exact_echo() {
        let (client, addr, handle) =
            spawn_cors_server(Some(vec!["https://app.one".to_string(), "http://localhost:3000".to_string()])).await;
        let resp = get_with_origin(&client, addr, "https://app.one").await;
        assert_eq!(
            resp.headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("https://app.one"),
            "listed origin must be echoed exactly, not as `*`"
        );
        let other = get_with_origin(&client, addr, "http://localhost:3000").await;
        assert_eq!(
            other
                .headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("http://localhost:3000")
        );
        handle.abort();
    }

    #[tokio::test]
    async fn t74_unlisted_origin_gets_no_echo_at_all() {
        let (client, addr, handle) = spawn_cors_server(Some(vec!["https://app.one".to_string()])).await;
        let resp = get_with_origin(&client, addr, "https://evil.example").await;
        assert!(
            resp.headers().get("access-control-allow-origin").is_none(),
            "non-listed origin must receive NO Access-Control-Allow-Origin, got {:?}",
            resp.headers()
        );
        // The body still serves (200) - CORS governs browser cross-origin reads, not the response itself.
        assert!(resp.status().is_success());
        handle.abort();
    }

    #[tokio::test]
    async fn t74_wildcard_entry_never_matches_even_a_literal_star_origin() {
        // Given: '*' smuggled past the loader (direct struct construction) - the layer
        // must neither panic (AllowOrigin::list does) nor treat '*' as match-everything.
        let (client, addr, handle) = spawn_cors_server(Some(vec!["*".to_string(), "https://app.one".to_string()])).await;
        let star = get_with_origin(&client, addr, "*").await;
        assert!(
            star.headers().get("access-control-allow-origin").is_none(),
            "'*' must not act as a wildcard"
        );
        let listed = get_with_origin(&client, addr, "https://app.one").await;
        assert_eq!(
            listed
                .headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("https://app.one"),
            "sibling entries survive the dropped wildcard"
        );
        handle.abort();
    }

    #[tokio::test]
    async fn t74_crlf_entry_is_dropped_not_splitted() {
        // Given: a response-splitting payload as a list entry (bypassing the loader gate)
        let (client, addr, handle) = spawn_cors_server(Some(vec!["https://ok.example\r\nX-Injected: yes".to_string()])).await;
        // When: a request carries the entry's PRONOUNCED origin (the prefix before CRLF)
        let resp = get_with_origin(&client, addr, "https://ok.example").await;
        // Then: nothing echoes and nothing was injected - the bad entry was dropped whole.
        assert!(resp.headers().get("access-control-allow-origin").is_none());
        assert!(
            resp.headers().get("x-injected").is_none(),
            "header injection must be impossible"
        );
        handle.abort();
    }

    #[tokio::test]
    async fn t74_preflight_listed_origin_exact_echo() {
        let (client, addr, handle) = spawn_cors_server(Some(vec!["https://app.one".to_string()])).await;
        let pre = client
            .request(reqwest::Method::OPTIONS, format!("http://{addr}/health"))
            .header("Origin", "https://app.one")
            .header("Access-Control-Request-Method", "POST")
            .send()
            .await
            .expect("preflight");
        assert_eq!(
            pre.headers().get("access-control-allow-origin").and_then(|v| v.to_str().ok()),
            Some("https://app.one")
        );
        let evil = client
            .request(reqwest::Method::OPTIONS, format!("http://{addr}/health"))
            .header("Origin", "https://evil.example")
            .header("Access-Control-Request-Method", "POST")
            .send()
            .await
            .expect("preflight");
        assert!(evil.headers().get("access-control-allow-origin").is_none());
        handle.abort();
    }

    #[test]
    fn t79_resolve_bind_addr_pin_table() {
        use std::net::{Ipv4Addr, Ipv6Addr};
        let v6 = |ip: Ipv6Addr, port: u16| SocketAddr::new(std::net::IpAddr::V6(ip), port);

        // IPv6, both operator spellings, must resolve to the SAME address.
        let expected = v6(Ipv6Addr::LOCALHOST, 8066);
        assert_eq!(resolve_bind_addr("::1", 8066).expect("bare ::1"), expected);
        assert_eq!(resolve_bind_addr("[::1]", 8066).expect("bracketed [::1]"), expected);
        assert_eq!(
            resolve_bind_addr("fe80::1", 9).expect("bare fe80::1"),
            v6("fe80::1".parse().expect("fixture"), 9)
        );
        // v4-mapped parses as the IPv6 it is (bind-able), bracket-normalized.
        assert_eq!(
            resolve_bind_addr("::ffff:127.0.0.1", 5).expect("v4-mapped"),
            v6("::ffff:127.0.0.1".parse().expect("fixture"), 5)
        );
        // IPv4 unchanged.
        assert_eq!(
            resolve_bind_addr("0.0.0.0", 8066).expect("v4"),
            SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8066)
        );
        assert_eq!(
            resolve_bind_addr("[127.0.0.1]", 1).expect("bracketed v4"),
            SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 1)
        );
        // Refusals: zone ids, half-brackets, names, junk, empty.
        for bad in ["fe80::1%eth0", "[::1", "::1]", "localhost", "not a host!!", ""] {
            assert!(resolve_bind_addr(bad, 8066).is_err(), "{bad} must be refused");
        }
    }

    #[tokio::test]
    async fn t79_ipv6_loopback_actually_binds_and_accepts() {
        // The acceptance is a live socket, not a parse: resolve, bind, connect over ::1.
        let addr = resolve_bind_addr("::1", 0).expect("bare ::1 resolves");
        let listener = tokio::net::TcpListener::bind(addr).await.expect("bind ::1");
        let bound = listener.local_addr().expect("local_addr");
        assert_eq!(bound.ip(), std::net::Ipv6Addr::LOCALHOST, "must listen ON ::1, got {bound}");
        let stream = tokio::time::timeout(std::time::Duration::from_secs(5), tokio::net::TcpStream::connect(bound))
            .await
            .expect("connect ::1 (no hang)")
            .expect("TCP accept");
        drop(stream);
    }

    use futures::StreamExt;

    /// `/stream` emits `data: one`, blocks on the semaphore (acquire queues, so
    /// a later `add_permits` always wakes it - no lost-wakeup race), then emits
    /// the tail + `[DONE]`. A zero-permit semaphore that nobody ever releases
    /// is the hung client.
    fn drain_app(gate: Arc<tokio::sync::Semaphore>) -> Router {
        Router::new()
            .route(
                "/stream",
                get(move || {
                    let gate = Arc::clone(&gate);
                    async move {
                        axum::body::Body::from_stream(futures::stream::unfold(0u8, move |st| {
                            let gate = Arc::clone(&gate);
                            async move {
                                match st {
                                    0 => Some((Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"data: one\n\n")), 1)),
                                    1 => {
                                        let Ok(_permit) = gate.acquire().await else { return None };
                                        drop(_permit);
                                        Some((Ok(bytes::Bytes::from_static(b"data: two\n\ndata: [DONE]\n\n")), 2))
                                    }
                                    _ => None,
                                }
                            }
                        }))
                    }
                }),
            )
            .route("/health", get(health_handler))
    }

    #[tokio::test]
    async fn t78_stream_in_flight_completes_and_new_connections_are_refused_during_drain() {
        // Given: a stream mid-body (chunk one already delivered to the client)
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let (trigger_tx, trigger_rx) = tokio::sync::oneshot::channel::<()>();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let app = drain_app(Arc::clone(&gate));
        let server = tokio::spawn(async move {
            GracefulServe {
                listener,
                app,
                shutdown: async {
                    trigger_rx.await.ok();
                },
                grace: std::time::Duration::from_secs(60),
                drain_started: Some(started_tx),
            }
            .run()
            .await;
        });
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://{addr}/stream"))
            .send()
            .await
            .expect("stream request");
        let mut body = resp.bytes_stream();
        let first = tokio::time::timeout(std::time::Duration::from_secs(5), body.next())
            .await
            .expect("chunk one (no hang)")
            .expect("stream alive")
            .expect("chunk ok");
        assert_eq!(first, b"data: one\n\n" as &[u8]);

        // When: shutdown is signalled and drain-start is OBSERVED (channel, not sleep)
        trigger_tx.send(()).expect("trigger");
        tokio::time::timeout(std::time::Duration::from_secs(5), started_rx)
            .await
            .expect("drain starts (no hang)")
            .expect("hook sender alive");

        // Then 1: a NEW connection during the drain is refused at TCP level.
        let refused = tokio::time::timeout(std::time::Duration::from_secs(5), tokio::net::TcpStream::connect(addr))
            .await
            .expect("connect attempt resolves (no hang)");
        assert!(
            refused.is_err(),
            "new connections must be refused during drain, got {refused:?}"
        );

        // Then 2: the IN-FLIGHT stream still completes end to end.
        gate.add_permits(1);
        let second = tokio::time::timeout(std::time::Duration::from_secs(5), body.next())
            .await
            .expect("tail chunk arrives after drain signal (survives!)")
            .expect("stream alive")
            .expect("tail ok");
        assert_eq!(second, b"data: two\n\ndata: [DONE]\n\n" as &[u8]);
        assert!(tokio::time::timeout(std::time::Duration::from_secs(5), body.next())
            .await
            .expect("stream end (no hang)")
            .is_none());

        // Then 3: with everything drained, the server returns without burning the 60s grace.
        tokio::time::timeout(std::time::Duration::from_secs(5), server)
            .await
            .expect("serve returns promptly once drained")
            .expect("serve task ok");

        // Then 4: after the server is gone, connects stay refused.
        assert!(tokio::net::TcpStream::connect(addr).await.is_err());
    }

    #[tokio::test]
    async fn t78_hung_stream_cannot_block_shutdown_past_the_grace() {
        // Given: a client whose body can never finish (gate never releases)
        let (trigger_tx, trigger_rx) = tokio::sync::oneshot::channel::<()>();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let app = drain_app(Arc::new(tokio::sync::Semaphore::new(0)));
        let grace = std::time::Duration::from_millis(300);
        let server = tokio::spawn(async move {
            GracefulServe {
                listener,
                app,
                shutdown: async {
                    trigger_rx.await.ok();
                },
                grace,
                drain_started: Some(started_tx),
            }
            .run()
            .await;
        });
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://{addr}/stream"))
            .send()
            .await
            .expect("stream request");
        let mut body = resp.bytes_stream();
        let first = tokio::time::timeout(std::time::Duration::from_secs(5), body.next())
            .await
            .expect("chunk one")
            .expect("stream alive")
            .expect("chunk ok");
        assert_eq!(first, b"data: one\n\n" as &[u8]);

        // When/Then: the hung connection cannot hold the process open - the
        // grace force-close terminates BOTH the client body and the serve loop.
        trigger_tx.send(()).expect("trigger");
        tokio::time::timeout(std::time::Duration::from_secs(5), started_rx)
            .await
            .expect("drain starts")
            .expect("hook sender alive");
        let hung_read = tokio::time::timeout(std::time::Duration::from_secs(5), body.next())
            .await
            .expect("hung body MUST terminate (force-close), not hang forever");
        match hung_read {
            None | Some(Err(_)) => {}
            Some(Ok(bytes)) => panic!("hung stream must be truncated, got extra body {bytes:?}"),
        }
        tokio::time::timeout(std::time::Duration::from_secs(5), server)
            .await
            .expect("serve must return at the grace cap, never later")
            .expect("serve task ok");
    }

    #[test]
    fn t78_production_grace_is_bounded() {
        // The D11 cap is a CONTRACT (deploys size their stop-timeout on it):
        // long enough for real generations to finish, finite so no client can
        // make it infinite.
        assert!(
            GRACEFUL_SHUTDOWN_GRACE >= Duration::from_secs(5) && GRACEFUL_SHUTDOWN_GRACE <= Duration::from_secs(120),
            "grace {GRACEFUL_SHUTDOWN_GRACE:?} must stay in the 5s..=120s deploy band"
        );
    }
}
