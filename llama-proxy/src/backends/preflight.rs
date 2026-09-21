//! Startup preflight checks for backend nodes
//!
//! Queries /v1/models to:
//! 1. Discover available models
//! 2. Detect backend type (vLLM vs llama.cpp)
//! 3. Pre-populate context size cache to avoid /props 404s on vLLM backends

use super::node::{build_node_client, node_url, with_auth};
use crate::config::{BackendConfig, BackendNodeConfig, BackendsConfig};
use crate::proxy::cache_context_from_preflight;

/// Preflight never waits longer than this on one node, however large the
/// configured request timeout is.
const PREFLIGHT_TIMEOUT_CAP_SECS: u64 = 10;

/// Hard cap on a /v1/models payload: 2 MiB. Bigger is not a model list worth
/// parsing at startup; the byte count is checked BEFORE any parse (E-L7).
const MODELS_MAX_BYTES: usize = 2 * 1024 * 1024;

/// Run preflight checks for all backend nodes in multi-backend mode.
/// Queries /v1/models from each node, logs a model summary, and auto-sets
/// the model override if the node serves exactly one model.
/// Never aborts startup — all failures are logged as warnings.
///
/// E-M4: every node probe runs CONCURRENTLY (join_all) — per-node work is
/// independent, so three slow nodes cost ~max(t), not Σ(t). Phase 1 probes
/// under an immutable borrow; phase 2 applies the single mutable effect
/// (model auto-mapping) once all futures are done.
pub async fn run_preflight_multi(backends: &mut BackendsConfig) {
    tracing::info!("Running backend preflight checks...");

    let probes: Vec<_> = backends
        .iter()
        .flat_map(|(group_name, group)| {
            group
                .nodes
                .iter()
                .enumerate()
                .map(move |(node_idx, node_cfg)| (group_name.clone(), node_idx, node_cfg.clone()))
        })
        .map(|(group, node_idx, cfg)| probe_node(group, node_idx, cfg))
        .collect();

    for outcome in futures::future::join_all(probes).await.into_iter().flatten() {
        if let Some(discovered) = outcome.auto_model {
            tracing::info!(
                group = %outcome.group,
                model = %discovered,
                "Preflight: auto-mapped model (single model detected)"
            );
            if let Some(group) = backends.get_mut(&outcome.group) {
                group.nodes[outcome.node_idx].model = Some(discovered);
            }
        }
    }
}

struct NodeProbeOutcome {
    group: String,
    node_idx: usize,
    /// Set when the node serves exactly one model and no override was configured.
    auto_model: Option<String>,
}

async fn probe_node(group: String, node_idx: usize, cfg: BackendNodeConfig) -> Option<NodeProbeOutcome> {
    let base_url = cfg.url.trim_end_matches('/').to_string();
    let models_url = node_url(&base_url, "/v1/models", cfg.strip_path_prefix.as_deref());

    let client = match build_preflight_client(cfg.timeout_seconds, cfg.tls.as_ref()) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                group = %group,
                url = %base_url,
                error = %e,
                "Preflight: failed to build HTTP client for node"
            );
            return None;
        }
    };

    let req = with_auth(client.get(&models_url), cfg.api_key.as_deref());
    let models_result = match fetch_models(req, cfg.model.as_deref()).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                group = %group,
                url = %base_url,
                error = %e,
                "Preflight: could not reach node (skipping)"
            );
            return None;
        }
    };

    tracing::info!(
        group = %group,
        url = %base_url,
        model_count = models_result.model_ids.len(),
        is_llama_cpp = models_result.is_llama_cpp,
        "Preflight: node models"
    );

    // Cache context using the already-fetched models info — avoids redundant HTTP calls
    let probe = ContextProbe {
        base_url: base_url.clone(),
        is_llama_cpp: models_result.is_llama_cpp,
        max_model_len: models_result.max_model_len,
        api_key: cfg.api_key.clone(),
    };
    if let Some(ctx) = cache_context_from_preflight(&client, &probe).await {
        tracing::info!(
            group = %group,
            url = %base_url,
            context_size = ctx,
            "Preflight: cached context size (backend type detected)"
        );
    }

    let auto_model = match models_result.model_ids.len() {
        0 => {
            tracing::warn!(
                group = %group,
                url = %base_url,
                "Preflight: node returned 0 models"
            );
            None
        }
        1 if cfg.model.is_none() => models_result.model_ids.into_iter().next(),
        1 => {
            tracing::info!(
                group = %group,
                url = %base_url,
                configured_model = %cfg.model.as_deref().unwrap_or(""),
                "Preflight: node model override already configured"
            );
            None
        }
        n if cfg.model.is_none() => {
            tracing::warn!(
                group = %group,
                url = %base_url,
                model_count = n,
                "Preflight: node has multiple models but no 'model:' override — \
                add 'model: <name>' to this node's config; requests will pass the client model name unchanged"
            );
            None
        }
        _ => None,
    };

    Some(NodeProbeOutcome {
        group,
        node_idx,
        auto_model,
    })
}

/// Run preflight check for a single-backend configuration.
pub async fn run_preflight_single(backend: &BackendConfig) {
    let base_url = backend.base_url().to_string();
    let models_url = node_url(&base_url, "/v1/models", backend.strip_path_prefix.as_deref());

    let client = match build_preflight_client(backend.timeout_seconds, backend.tls.as_ref()) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(url = %base_url, error = %e, "Preflight: failed to build HTTP client");
            return;
        }
    };

    let req = with_auth(client.get(&models_url), backend.api_key.as_deref());

    let (is_llama_cpp, max_model_len) = match fetch_models(req, backend.model.as_deref()).await {
        Ok(result) => {
            tracing::info!(
                url = %base_url,
                model_count = result.model_ids.len(),
                is_llama_cpp = result.is_llama_cpp,
                "Preflight: single backend models"
            );
            (result.is_llama_cpp, result.max_model_len)
        }
        Err(e) => {
            tracing::warn!(url = %base_url, error = %e, "Preflight: could not query /v1/models (skipping)");
            (false, None)
        }
    };

    // Cache context using the already-fetched models info — avoids redundant HTTP calls
    let probe = ContextProbe {
        base_url: base_url.clone(),
        is_llama_cpp,
        max_model_len,
        api_key: backend.api_key.clone(),
    };
    let context_total = cache_context_from_preflight(&client, &probe).await;
    if let Some(ctx) = context_total {
        tracing::info!(
            url = %base_url,
            context_size = ctx,
            "Preflight: cached context size (backend type detected)"
        );
    }
}

#[derive(Debug)]
struct ModelsResult {
    model_ids: Vec<String>,
    max_model_len: Option<u64>,
    is_llama_cpp: bool,
}

/// What a node's /v1/models probe learned, plus what a follow-up /props fetch
/// needs to talk to that node. Built by preflight, consumed by the context cache.
#[derive(Debug, Clone)]
pub struct ContextProbe {
    /// Backend base URL (trailing slash stripped) — also the context-cache key.
    pub base_url: String,
    /// The `Server` response header identified llama.cpp: fetch the runtime
    /// n_ctx from /props (max_model_len there is the training context, not -c).
    pub is_llama_cpp: bool,
    /// `max_model_len` extracted from the probe response body.
    pub max_model_len: Option<u64>,
    /// The node's api_key. /props must present it: an auth'd backend 401s a
    /// bare request and the miss would be silent (big-fix E-M3).
    pub api_key: Option<String>,
}

/// Fetch model IDs from a /v1/models request builder.
/// Also captures the Server header and max_model_len for backend type detection.
/// The raw payload is capped at MODELS_MAX_BYTES and checked as bytes BEFORE
/// parse; full body goes to debug!, info! gets only the summary (E-L7).
async fn fetch_models(
    req: reqwest::RequestBuilder,
    model_name: Option<&str>,
) -> Result<ModelsResult, Box<dyn std::error::Error>> {
    let resp = req.send().await?;
    if !resp.status().is_success() {
        return Err(format!("/v1/models returned HTTP {}", resp.status()).into());
    }

    // Detect llama.cpp from Server header before consuming body
    let is_llama_cpp = resp
        .headers()
        .get("server")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("llama.cpp"))
        .unwrap_or(false);

    let bytes = resp.bytes().await?;
    if bytes.len() > MODELS_MAX_BYTES {
        return Err(format!(
            "/v1/models payload is {} bytes, over the {}-byte cap",
            bytes.len(),
            MODELS_MAX_BYTES
        )
        .into());
    }
    tracing::debug!("Preflight: /v1/models payload: {}", String::from_utf8_lossy(&bytes));
    let body: serde_json::Value = serde_json::from_slice(&bytes)?;

    let (model_ids, max_model_len) = if let Some(arr) = body.get("data").and_then(|d| d.as_array()) {
        let ids = arr
            .iter()
            .filter_map(|m| m.get("id").and_then(|id| id.as_str()).map(|s| s.to_string()))
            .collect();
        (ids, select_max_model_len(arr, model_name))
    } else {
        (vec![], None)
    };

    Ok(ModelsResult {
        model_ids,
        max_model_len,
        is_llama_cpp,
    })
}

/// Pick the context size to cache from a /v1/models `data` array.
///
/// With a configured model name: the entry whose `id` LONGEST-matches it —
/// a match means either side is a prefix of the other (id "qwen3-14b" answers
/// name "qwen3" and vice versa); longest id wins, ties break to the larger
/// max_model_len. With no name, or no matching entry: the MAX over entries
/// carrying BOTH a string id and a numeric max_model_len — never the bare
/// first entry, which on a multi-model node is not necessarily the served one
/// (E-L8). Entries lacking either field are skipped honestly.
fn select_max_model_len(entries: &[serde_json::Value], model_name: Option<&str>) -> Option<u64> {
    let scored = entries.iter().filter_map(|e| {
        let id = e.get("id").and_then(|i| i.as_str())?;
        let len = e.get("max_model_len").and_then(|v| v.as_u64())?;
        Some((id, len))
    });
    if let Some(name) = model_name {
        let mut best: Option<(usize, u64)> = None;
        for (id, len) in scored.clone() {
            if id.starts_with(name) || name.starts_with(id) {
                let candidate = (id.len(), len);
                if best.is_none_or(|b| candidate > b) {
                    best = Some(candidate);
                }
            }
        }
        if let Some((_, len)) = best {
            return Some(len);
        }
    }
    scored.map(|(_, len)| len).max()
}

fn build_preflight_client(
    timeout_seconds: u64,
    tls: Option<&crate::config::TlsConfig>,
) -> Result<reqwest::Client, Box<dyn std::error::Error>> {
    // ONE factory with the forwarding path (big-fix E-M5): full TLS handling
    // (CA pinning, mTLS, danger flag) and the connect timeout come for free;
    // preflight only caps the request timeout (0 stays 0 = no timeout, as before).
    let preflight_timeout = timeout_seconds.min(PREFLIGHT_TIMEOUT_CAP_SECS);
    build_node_client(preflight_timeout, tls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TlsConfig;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;

    #[test]
    fn preflight_client_factory_propagates_node_tls_errors() {
        // Baseline build_preflight_client DROPPED every TLS setting but
        // accept_invalid_certs and happily returned Ok for a broken CA path.
        let tls = TlsConfig {
            accept_invalid_certs: false,
            ca_cert_path: Some("/nonexistent/ca.pem".to_string()),
            client_cert_path: None,
            client_key_path: None,
        };
        assert!(
            build_preflight_client(300, Some(&tls)).is_err(),
            "the node factory's CA handling must not be silently dropped (E-M5)"
        );
    }

    #[tokio::test]
    async fn stalled_node_cannot_wedge_the_preflight_client() {
        // The shared factory's request timeout must abort a black-holed probe:
        // configured 1s stays 1s (below the 10s cap). The connect-timeout half
        // of the factory is pinned in node.rs (build_node_client_applies_connect_timeout).
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let stall = tokio::spawn(async move {
            let _accepted = listener.accept().await;
            std::future::pending::<()>().await;
        });
        let client = build_preflight_client(1, None).expect("client");

        let start = std::time::Instant::now();
        let result = client.get(format!("http://127.0.0.1:{port}/v1/models")).send().await;
        let elapsed = start.elapsed();

        assert!(result.is_err(), "a stalled node must surface a request error, got Ok");
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "the ~1s request timeout must fire, took {elapsed:?}"
        );
        stall.abort();
    }

    /// Records (uri, bearer-present) per request; /v1/models answers one model
    /// with max_model_len and a NON-llama.cpp Server header (no follow-up /props).
    async fn spawn_models_listener() -> (String, mpsc::Receiver<(String, bool)>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (tx, rx) = mpsc::channel::<(String, bool)>(16);
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let mut head: Vec<u8> = Vec::new();
                let mut tmp = [0u8; 1024];
                loop {
                    match sock.read(&mut tmp).await {
                        Ok(0) => break,
                        Ok(n) => {
                            head.extend_from_slice(&tmp[..n]);
                            if head.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let text = String::from_utf8_lossy(&head).to_string();
                let uri = text.lines().next().unwrap_or("").split(' ').nth(1).unwrap_or("").to_string();
                let authed = text.to_ascii_lowercase().contains("authorization: bearer secret");
                if tx.send((uri.clone(), authed)).await.is_err() {
                    break;
                }
                let body = r#"{"object":"list","data":[{"id":"solo","max_model_len":4096}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nserver: probe\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        (format!("http://127.0.0.1:{port}"), rx, handle)
    }

    #[tokio::test]
    async fn models_probe_presents_the_nodes_api_key() {
        let (base, mut rx, server) = spawn_models_listener().await;
        let backend = BackendConfig {
            url: base,
            timeout_seconds: 300,
            tls: None,
            model: Some("solo".to_string()),
            api_key: Some("secret".to_string()),
            strip_path_prefix: None,
        };

        run_preflight_single(&backend).await;

        let (uri, authed) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("request within 5s")
            .expect("channel open");
        assert_eq!(uri, "/v1/models");
        assert!(authed, "the /v1/models probe must carry the node's bearer token (E-M3)");
        server.abort();
    }

    use crate::config::{BackendGroupConfig, BackendNodeConfig};

    fn node(url: String) -> BackendNodeConfig {
        BackendNodeConfig {
            url,
            timeout_seconds: 300,
            tls: None,
            model: Some("solo".to_string()),
            api_key: None,
            strip_path_prefix: None,
            temperature: None,
        }
    }

    fn group(nodes: Vec<BackendNodeConfig>) -> BackendGroupConfig {
        BackendGroupConfig {
            mappings: vec![],
            strategy: "round_robin".to_string(),
            failure_cooldown_secs: 0,
            nodes,
        }
    }

    fn solo_body() -> Vec<u8> {
        br#"{"object":"list","data":[{"id":"solo","max_model_len":4096}]}"#.to_vec()
    }

    /// Fake node: answers every request with `body` (Server: probe, so no
    /// follow-up /props) after `delay_ms`, recording each request URI.
    async fn spawn_served_node(body: Vec<u8>, delay_ms: u64) -> (String, mpsc::Receiver<String>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (tx, rx) = mpsc::channel::<String>(32);
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let mut head: Vec<u8> = Vec::new();
                let mut tmp = [0u8; 1024];
                loop {
                    match sock.read(&mut tmp).await {
                        Ok(0) => break,
                        Ok(n) => {
                            head.extend_from_slice(&tmp[..n]);
                            if head.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let uri = String::from_utf8_lossy(&head)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .split(' ')
                    .nth(1)
                    .unwrap_or("")
                    .to_string();
                if tx.send(uri).await.is_err() {
                    break;
                }
                if delay_ms > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nserver: probe\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.write_all(&body).await;
                let _ = sock.shutdown().await;
            }
        });
        (format!("http://127.0.0.1:{port}"), rx, handle)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn multi_preflight_probes_all_nodes_at_once() {
        // Sequential baseline measured at 57b2d2b: 300/200/100ms nodes cost Σ=607.8ms.
        for round in 0..2 {
            let slow = spawn_served_node(solo_body(), 300).await;
            let mid = spawn_served_node(solo_body(), 200).await;
            let fast = spawn_served_node(solo_body(), 100).await;
            let mut backends = BackendsConfig::new();
            backends.insert("g".to_string(), group(vec![node(slow.0), node(mid.0), node(fast.0)]));

            let start = std::time::Instant::now();
            run_preflight_multi(&mut backends).await;
            let elapsed = start.elapsed();

            for mut rx in [slow.1, mid.1, fast.1] {
                assert_eq!(rx.recv().await.expect("probe"), "/v1/models", "round {round}");
            }
            assert!(
                elapsed < std::time::Duration::from_millis(500),
                "round {round}: concurrent probes cost ~max(t)=300ms, not Σ(t)=600ms — took {elapsed:?}"
            );
            assert!(
                elapsed >= std::time::Duration::from_millis(250),
                "round {round}: under 250ms means the delays were bypassed, not overlapped — took {elapsed:?}"
            );
            slow.2.abort();
            mid.2.abort();
            fast.2.abort();
        }
    }

    #[tokio::test]
    async fn models_payload_cap_gates_at_exactly_2mib() {
        let prefix = br##"{"data":[{"id":""##;
        let suffix = br#"","max_model_len":7}]}"#;
        let mut at_cap = Vec::new();
        at_cap.extend_from_slice(prefix);
        at_cap.extend(std::iter::repeat_n(b'a', MODELS_MAX_BYTES - prefix.len() - suffix.len()));
        at_cap.extend_from_slice(suffix);
        assert_eq!(at_cap.len(), MODELS_MAX_BYTES);
        let mut over = at_cap.clone();
        over.push(b'a');

        let ok = spawn_served_node(at_cap, 0).await;
        let big = spawn_served_node(over, 0).await;
        let client = reqwest::Client::new();

        let result = fetch_models(client.get(format!("{}/v1/models", ok.0)), None)
            .await
            .expect("exactly 2 MiB is AT the cap, not over it");
        assert_eq!(result.max_model_len, Some(7));

        let err = fetch_models(client.get(format!("{}/v1/models", big.0)), None)
            .await
            .expect_err("2 MiB + 1 byte must be rejected BEFORE any parse");
        assert!(err.to_string().contains("over"), "honest cap message: {err}");
        ok.2.abort();
        big.2.abort();
    }

    #[test]
    fn max_model_len_takes_the_longest_name_match_then_the_max() {
        let mk = |pairs: &[(&str, u64)]| -> Vec<serde_json::Value> {
            pairs
                .iter()
                .map(|(id, len)| serde_json::json!({"id": id, "max_model_len": len}))
                .collect()
        };
        let arr = mk(&[("qwen3", 4096), ("qwen3-14b", 8192), ("llama3", 700000)]);
        assert_eq!(
            select_max_model_len(&arr, Some("qwen3")),
            Some(8192),
            "longest id wins, not first match"
        );
        assert_eq!(
            select_max_model_len(&arr, Some("qwen3-14b-q3")),
            Some(8192),
            "the name may extend the id too"
        );
        assert_eq!(
            select_max_model_len(&arr, Some("zzz")),
            Some(700000),
            "no match falls back to MAX, not first entry"
        );
        assert_eq!(select_max_model_len(&arr, None), Some(700000));
        let tied = mk(&[("qwen3", 1024), ("qwen4", 4096)]);
        assert_eq!(
            select_max_model_len(&tied, Some("qwen")),
            Some(4096),
            "id-length tie breaks to larger max_model_len"
        );
    }

    #[tokio::test]
    async fn entries_without_usable_id_or_len_are_skipped_honestly() {
        let body = br#"{"data":[{"id":123,"max_model_len":500000},{"id":"ok","max_model_len":4096},{"id":"no-len"}]}"#.to_vec();
        let srv = spawn_served_node(body, 0).await;
        let client = reqwest::Client::new();

        let result = fetch_models(client.get(format!("{}/v1/models", srv.0)), None)
            .await
            .expect("parses");
        assert_eq!(result.model_ids, vec!["ok".to_string(), "no-len".to_string()]);
        assert_eq!(
            result.max_model_len,
            Some(4096),
            "the numeric-id 500000 entry is unusable, not a fallback"
        );
        srv.2.abort();
    }

    #[tokio::test]
    async fn strip_prefix_probe_uses_the_shared_url_helper() {
        let mut srv = spawn_served_node(solo_body(), 0).await;
        let mut n = node(srv.0);
        n.strip_path_prefix = Some("/v1".to_string());
        let mut backends = BackendsConfig::new();
        backends.insert("g".to_string(), group(vec![n]));

        run_preflight_multi(&mut backends).await;

        assert_eq!(
            srv.1.recv().await.expect("probe"),
            "/models",
            "strip must apply exactly once, via node_url"
        );
        srv.2.abort();
    }

    #[tokio::test]
    async fn single_model_node_gets_auto_mapped_after_the_join() {
        let srv = spawn_served_node(solo_body(), 0).await;
        let mut n = node(srv.0);
        n.model = None;
        let mut backends = BackendsConfig::new();
        backends.insert("g".to_string(), group(vec![n]));

        run_preflight_multi(&mut backends).await;

        assert_eq!(backends["g"].nodes[0].model.as_deref(), Some("solo"));
        srv.2.abort();
    }
}
