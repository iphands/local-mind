//! Main proxy server implementation

use axum::{
    extract::State,
    http::HeaderValue,
    routing::{any, get},
    Router,
};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use std::sync::Arc;
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

    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("llama-proxy listening on {}", addr);

    Ok(axum::serve(listener, app).await?)
}

/// Health check endpoint
async fn health_handler() -> &'static str {
    "OK"
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
}
