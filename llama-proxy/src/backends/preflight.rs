//! Startup preflight checks for backend nodes
//!
//! Queries /v1/models to:
//! 1. Discover available models
//! 2. Detect backend type (vLLM vs llama.cpp)
//! 3. Pre-populate context size cache to avoid /props 404s on vLLM backends

use crate::backends::node::build_node_client;
use crate::config::{BackendConfig, BackendsConfig};
use crate::proxy::cache_context_from_preflight;

/// Preflight never waits longer than this on one node, however large the
/// configured request timeout is.
const PREFLIGHT_TIMEOUT_CAP_SECS: u64 = 10;

/// Run preflight checks for all backend nodes in multi-backend mode.
/// Queries /v1/models from each node, logs available models, and auto-sets
/// the model override if the node serves exactly one model.
/// Never aborts startup — all failures are logged as warnings.
pub async fn run_preflight_multi(backends: &mut BackendsConfig) {
    tracing::info!("Running backend preflight checks...");

    for (group_name, group) in backends.iter_mut() {
        for node_cfg in group.nodes.iter_mut() {
            let base_url = node_cfg.url.trim_end_matches('/').to_string();
            let models_path = if let Some(ref prefix) = node_cfg.strip_path_prefix {
                "/v1/models".strip_prefix(prefix.as_str()).unwrap_or("/v1/models")
            } else {
                "/v1/models"
            };
            let models_url = format!("{}{}", base_url, models_path);

            let client = match build_preflight_client(node_cfg.timeout_seconds, node_cfg.tls.as_ref()) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(
                        group = %group_name,
                        url = %base_url,
                        error = %e,
                        "Preflight: failed to build HTTP client for node"
                    );
                    continue;
                }
            };

            let req = crate::backends::node::with_auth(client.get(&models_url), node_cfg.api_key.as_deref());

            let models_result = match fetch_models(req).await {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(
                        group = %group_name,
                        url = %base_url,
                        error = %e,
                        "Preflight: could not reach node (skipping)"
                    );
                    continue;
                }
            };

            tracing::info!(
                group = %group_name,
                url = %base_url,
                models = ?models_result.model_ids,
                is_llama_cpp = models_result.is_llama_cpp,
                "Preflight: node models"
            );

            // Cache context using the already-fetched models info — avoids redundant HTTP calls
            let probe = ContextProbe {
                base_url: base_url.clone(),
                is_llama_cpp: models_result.is_llama_cpp,
                max_model_len: models_result.max_model_len,
                api_key: node_cfg.api_key.clone(),
            };
            let context_total = cache_context_from_preflight(&client, &probe).await;
            if let Some(ctx) = context_total {
                tracing::info!(
                    group = %group_name,
                    url = %base_url,
                    context_size = ctx,
                    "Preflight: cached context size (backend type detected)"
                );
            }

            match models_result.model_ids.len() {
                0 => {
                    tracing::warn!(
                        group = %group_name,
                        url = %base_url,
                        "Preflight: node returned 0 models"
                    );
                }
                1 if node_cfg.model.is_none() => {
                    let discovered = models_result.model_ids.into_iter().next().unwrap();
                    tracing::info!(
                        group = %group_name,
                        url = %base_url,
                        model = %discovered,
                        "Preflight: auto-mapped model (single model detected)"
                    );
                    node_cfg.model = Some(discovered);
                }
                1 => {
                    // Already has a model override; just confirm it
                    tracing::info!(
                        group = %group_name,
                        url = %base_url,
                        configured_model = %node_cfg.model.as_deref().unwrap_or(""),
                        "Preflight: node model override already configured"
                    );
                }
                n if node_cfg.model.is_none() => {
                    tracing::warn!(
                        group = %group_name,
                        url = %base_url,
                        model_count = n,
                        "Preflight: node has multiple models but no 'model:' override — \
                        add 'model: <name>' to this node's config; requests will pass the client model name unchanged"
                    );
                }
                _ => {
                    // Multiple models, override already set — nothing to do
                }
            }
        }
    }
}

/// Run preflight check for a single-backend configuration.
pub async fn run_preflight_single(backend: &BackendConfig) {
    let base_url = backend.base_url().to_string();
    let models_path = if let Some(ref prefix) = backend.strip_path_prefix {
        "/v1/models".strip_prefix(prefix.as_str()).unwrap_or("/v1/models")
    } else {
        "/v1/models"
    };
    let models_url = format!("{}{}", base_url, models_path);

    let client = match build_preflight_client(backend.timeout_seconds, backend.tls.as_ref()) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(url = %base_url, error = %e, "Preflight: failed to build HTTP client");
            return;
        }
    };

    let req = crate::backends::node::with_auth(client.get(&models_url), backend.api_key.as_deref());

    let (is_llama_cpp, max_model_len) = match fetch_models(req).await {
        Ok(result) => {
            tracing::info!(
                url = %base_url,
                models = ?result.model_ids,
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
async fn fetch_models(req: reqwest::RequestBuilder) -> Result<ModelsResult, Box<dyn std::error::Error>> {
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

    let body: serde_json::Value = resp.json().await?;

    let (model_ids, max_model_len) = if let Some(arr) = body.get("data").and_then(|d| d.as_array()) {
        let ids = arr
            .iter()
            .filter_map(|m| m.get("id").and_then(|id| id.as_str()).map(|s| s.to_string()))
            .collect();
        let max_len = arr.first().and_then(|m| m.get("max_model_len")).and_then(|v| v.as_u64());
        (ids, max_len)
    } else {
        (vec![], None)
    };

    Ok(ModelsResult {
        model_ids,
        max_model_len,
        is_llama_cpp,
    })
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
}
