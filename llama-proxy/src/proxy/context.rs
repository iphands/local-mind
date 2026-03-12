//! Context size fetching and caching from backend endpoints
//!
//! Supports multiple backend types:
//! - llama.cpp: Uses `/props` endpoint with `default_generation_settings.n_ctx`
//! - vLLM/OpenAI-compatible: Uses `/v1/models` endpoint with `data[0].max_model_len`

use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::RwLock;

// Global cache: backend_url -> context_size
static CONTEXT_CACHE: OnceLock<RwLock<HashMap<String, u64>>> = OnceLock::new();

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
pub async fn fetch_context_total(client: &reqwest::Client, backend_url: &str) -> Option<u64> {
    let cache = CONTEXT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));

    // Check cache first
    {
        let read_guard = cache.read().await;
        if let Some(&ctx) = read_guard.get(backend_url) {
            return Some(ctx);
        }
    }

    // Try llama.cpp /props endpoint first
    if let Some(n_ctx) = fetch_from_props(client, backend_url).await {
        cache_result(cache, backend_url, n_ctx);
        return Some(n_ctx);
    }

    // Fallback to vLLM/OpenAI-compatible /v1/models endpoint
    if let Some(max_model_len) = fetch_from_models(client, backend_url).await {
        cache_result(cache, backend_url, max_model_len);
        return Some(max_model_len);
    }

    None
}

/// Fetch context size from llama.cpp `/props` endpoint
async fn fetch_from_props(client: &reqwest::Client, backend_url: &str) -> Option<u64> {
    let props_url = format!("{}/props", backend_url);
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
        }
        Err(e) => {
            tracing::debug!("Failed to fetch context size from {}: {}", props_url, e);
        }
    }
    None
}

/// Fetch context size from vLLM/OpenAI-compatible `/v1/models` endpoint
async fn fetch_from_models(client: &reqwest::Client, backend_url: &str) -> Option<u64> {
    let models_url = format!("{}/v1/models", backend_url);
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
fn cache_result(cache: &RwLock<HashMap<String, u64>>, backend_url: &str, value: u64) {
    if let Ok(mut write_guard) = cache.try_write() {
        write_guard.insert(backend_url.to_string(), value);
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
            write_guard.insert("http://test".to_string(), 4096);
        }

        // Verify cache read works
        {
            let read_guard = cache.read().await;
            assert_eq!(read_guard.get("http://test"), Some(&4096));
        }
    }
}
