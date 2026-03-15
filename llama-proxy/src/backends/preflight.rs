//! Startup preflight checks for backend nodes

use std::time::Duration;

use crate::config::{BackendsConfig, BackendConfig};

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

            let mut req = client.get(&models_url);
            if let Some(ref api_key) = node_cfg.api_key {
                req = req.header("Authorization", format!("Bearer {}", api_key));
            }

            let models = match fetch_models(req).await {
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
                models = ?models,
                "Preflight: node models"
            );

            match models.len() {
                0 => {
                    tracing::warn!(
                        group = %group_name,
                        url = %base_url,
                        "Preflight: node returned 0 models"
                    );
                }
                1 if node_cfg.model.is_none() => {
                    let discovered = models.into_iter().next().unwrap();
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

    let mut req = client.get(&models_url);
    if let Some(ref api_key) = backend.api_key {
        req = req.header("Authorization", format!("Bearer {}", api_key));
    }

    match fetch_models(req).await {
        Ok(models) => {
            tracing::info!(
                url = %base_url,
                models = ?models,
                "Preflight: single backend models"
            );
        }
        Err(e) => {
            tracing::warn!(url = %base_url, error = %e, "Preflight: could not query /v1/models (skipping)");
        }
    }
}

/// Fetch model IDs from a /v1/models request builder.
async fn fetch_models(req: reqwest::RequestBuilder) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let resp = req.send().await?;
    if !resp.status().is_success() {
        return Err(format!("/v1/models returned HTTP {}", resp.status()).into());
    }
    let body: serde_json::Value = resp.json().await?;
    let ids = body
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|id| id.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Ok(ids)
}

fn build_preflight_client(
    timeout_seconds: u64,
    tls: Option<&crate::config::TlsConfig>,
) -> Result<reqwest::Client, Box<dyn std::error::Error>> {
    // Use short timeout for preflight (max 10s, but respect configured timeout)
    let preflight_timeout = timeout_seconds.min(10);
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(preflight_timeout));

    if let Some(tls_cfg) = tls {
        if tls_cfg.accept_invalid_certs {
            builder = builder.danger_accept_invalid_certs(true);
        }
    }

    Ok(builder.build()?)
}
