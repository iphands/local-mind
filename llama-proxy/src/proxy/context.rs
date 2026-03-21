//! Context size fetching and caching from backend endpoints
//!
//! Supports multiple backend types:
//! - llama.cpp: Uses `/props` endpoint with `default_generation_settings.n_ctx`
//! - vLLM/OpenAI-compatible: Uses `/v1/models` endpoint with `data[0].max_model_len`

use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::RwLock;

// Global cache: backend_url -> (context_size, backend_type)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendType {
    LlamaCpp,
    Vllm,
}

static CONTEXT_CACHE: OnceLock<RwLock<HashMap<String, (u64, BackendType)>>> = OnceLock::new();

/// Fetch context total from backend with caching
///
/// Tries multiple endpoints to support different backend types:
/// 1. `/props` (llama.cpp) - extracts `default_generation_settings.n_ctx`
/// 2. `/v1/models` (vLLM, OpenAI-compatible) - extracts `data[0].max_model_len`
///
/// The cache is permanent for the lifetime of the application since context
/// size is a static server configuration.
///
/// # Arguments
/// * `client` - The HTTP client to use for the request
/// * `backend_url` - The base URL of the backend server
///
/// # Returns
/// * `Some(u64)` - The context size if successfully fetched
/// * `None` - If all fetch attempts failed or responses were malformed
pub async fn fetch_context_total(client: &reqwest::Client, backend_url: &str, strip_path_prefix: Option<&str>) -> Option<u64> {
    let cache = CONTEXT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));

    // Check cache first
    {
        let read_guard = cache.read().await;
        if let Some(&(ctx, _)) = read_guard.get(backend_url) {
            return Some(ctx);
        }
    }

    // Try llama.cpp /props endpoint first
    if let Some(n_ctx) = fetch_from_props(client, backend_url, strip_path_prefix).await {
        cache_result(cache, backend_url, n_ctx, BackendType::LlamaCpp);
        return Some(n_ctx);
    }

    // Fallback to vLLM/OpenAI-compatible /v1/models endpoint
    if let Some(max_model_len) = fetch_from_models(client, backend_url, strip_path_prefix).await {
        cache_result(cache, backend_url, max_model_len, BackendType::Vllm);
        return Some(max_model_len);
    }

    None
}

/// Cache context size from preflight data, avoiding redundant HTTP calls.
///
/// Called by preflight after it has already fetched /v1/models and inspected
/// the `Server` response header to determine backend type.
///
/// - llama.cpp (`is_llama_cpp = true`): fetches `/props` for the actual configured n_ctx
///   (max_model_len from /v1/models is the training context, not the server's -c setting)
/// - Other backends: uses `max_model_len` already extracted from the /v1/models response
pub async fn cache_context_from_preflight(
    client: &reqwest::Client,
    backend_url: &str,
    strip_path_prefix: Option<&str>,
    is_llama_cpp: bool,
    max_model_len: Option<u64>,
) -> Option<u64> {
    let cache = CONTEXT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));

    // Check cache first (shouldn't be populated yet during preflight, but be safe)
    {
        let read_guard = cache.read().await;
        if let Some(&(ctx, _)) = read_guard.get(backend_url) {
            return Some(ctx);
        }
    }

    if is_llama_cpp {
        // Need /props for the actual runtime n_ctx (distinct from model's n_ctx_train)
        if let Some(n_ctx) = fetch_from_props(client, backend_url, strip_path_prefix).await {
            cache_result(cache, backend_url, n_ctx, BackendType::LlamaCpp);
            return Some(n_ctx);
        }
        None
    } else if let Some(ctx) = max_model_len {
        cache_result(cache, backend_url, ctx, BackendType::Vllm);
        Some(ctx)
    } else {
        None
    }
}

/// Fetch context size from llama.cpp `/props` endpoint
async fn fetch_from_props(client: &reqwest::Client, backend_url: &str, strip_path_prefix: Option<&str>) -> Option<u64> {
    let path = if let Some(prefix) = strip_path_prefix {
        "/props".strip_prefix(prefix).unwrap_or("/props")
    } else {
        "/props"
    };
    let props_url = format!("{}{}", backend_url, path);
    match client.get(&props_url).send().await {
        Ok(resp) => {
            if let Ok(props) = resp.json::<serde_json::Value>().await {
                if let Some(n_ctx) = props
                    .get("default_generation_settings")
                    .and_then(|s| s.get("n_ctx"))
                    .and_then(|n| n.as_u64())
                {
                    tracing::debug!("Fetched context size from /props: {}", n_ctx);
                    return Some(n_ctx);
                }
            }
            // Props endpoint exists but didn't return expected data - might be vLLM
            None
        }
        Err(e) => {
            tracing::debug!("Failed to fetch context size from {}: {}", props_url, e);
            None
        }
    }
}

/// Fetch context size from vLLM/OpenAI-compatible `/v1/models` endpoint
async fn fetch_from_models(client: &reqwest::Client, backend_url: &str, strip_path_prefix: Option<&str>) -> Option<u64> {
    let path = if let Some(prefix) = strip_path_prefix {
        "/v1/models".strip_prefix(prefix).unwrap_or("/v1/models")
    } else {
        "/v1/models"
    };
    let models_url = format!("{}{}", backend_url, path);
    match client.get(&models_url).send().await {
        Ok(resp) => {
            if let Ok(models) = resp.json::<serde_json::Value>().await {
                // Extract max_model_len from first model: data[0].max_model_len
                if let Some(max_model_len) = models
                    .get("data")
                    .and_then(|d| d.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|model| model.get("max_model_len"))
                    .and_then(|m| m.as_u64())
                {
                    tracing::debug!("Fetched context size from /v1/models: {}", max_model_len);
                    return Some(max_model_len);
                }
            }
        }
        Err(e) => {
            tracing::debug!("Failed to fetch context size from {}: {}", models_url, e);
        }
    }
    None
}

/// Cache the result for future requests
fn cache_result(cache: &RwLock<HashMap<String, (u64, BackendType)>>, backend_url: &str, value: u64, backend_type: BackendType) {
    if let Ok(mut write_guard) = cache.try_write() {
        write_guard.insert(backend_url.to_string(), (value, backend_type));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_context_total_caching() {
        // This test verifies the cache works, but can't test actual fetching
        // without a mock server. In real use, the function will be tested
        // through integration tests.
        let cache = CONTEXT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));

        // Pre-populate cache
        {
            let mut write_guard = cache.write().await;
            write_guard.insert("http://test".to_string(), (4096, BackendType::LlamaCpp));
        }

        // Verify cache read works
        {
            let read_guard = cache.read().await;
            assert_eq!(read_guard.get("http://test"), Some(&(4096, BackendType::LlamaCpp)));
        }
    }
}
