//! Context size fetching and caching from llama.cpp /props endpoint

use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::RwLock;

// Global cache: backend_url -> context_size
static CONTEXT_CACHE: OnceLock<RwLock<HashMap<String, u64>>> = OnceLock::new();

/// Fetch context total from backend /props endpoint with caching
///
/// This function fetches the total context size (n_ctx) from the llama.cpp
/// `/props` endpoint and caches it per backend URL. The cache is permanent
/// for the lifetime of the application since context size is a static server
/// configuration.
///
/// # Arguments
/// * `client` - The HTTP client to use for the request
/// * `backend_url` - The base URL of the llama.cpp backend
///
/// # Returns
/// * `Some(u64)` - The context size if successfully fetched
/// * `None` - If the fetch failed or the response was malformed
pub async fn fetch_context_total(client: &reqwest::Client, backend_url: &str) -> Option<u64> {
    let cache = CONTEXT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));

    // Check cache first
    {
        let read_guard = cache.read().await;
        if let Some(&ctx) = read_guard.get(backend_url) {
            return Some(ctx);
        }
    }

    // Fetch from /props endpoint
    let props_url = format!("{}/props", backend_url);
    match client.get(&props_url).send().await {
        Ok(resp) => {
            if let Ok(props) = resp.json::<serde_json::Value>().await {
                if let Some(n_ctx) = props
                    .get("default_generation_settings")
                    .and_then(|s| s.get("n_ctx"))
                    .and_then(|n| n.as_u64())
                {
                    // Cache for future requests
                    let mut write_guard = cache.write().await;
                    write_guard.insert(backend_url.to_string(), n_ctx);
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
