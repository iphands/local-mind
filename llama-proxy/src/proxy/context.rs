//! Context size fetching and caching from backend endpoints
//!
//! Supports multiple backend types:
//! - llama.cpp: Uses `/props` endpoint with `default_generation_settings.n_ctx`
//! - vLLM/OpenAI-compatible: Uses `/v1/models` endpoint with `data[0].max_model_len`

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use tokio::sync::RwLock;

use crate::backends::node_url;
use crate::backends::preflight::ContextProbe;
use crate::backends::with_auth;

// Global cache: backend_url -> (context_size, backend_type)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendType {
    LlamaCpp,
    Vllm,
}

static CONTEXT_CACHE: OnceLock<RwLock<HashMap<String, (u64, BackendType)>>> = OnceLock::new();

// Track which backends we've already warned about to avoid log spam
static WARNED_BACKENDS: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();

/// Number of context-cache refresh writes that were skipped because the write lock was
/// held when a fresh value arrived (see the staleness policy on `cache_result`).
/// A successful refresh decrements nothing - this counts skips, not the stale window.
static CONTEXT_CACHE_STALE_SKIPS: AtomicU64 = AtomicU64::new(0);

/// Read-only view of the skipped-refresh counter, for the stats line / metrics surface.
///
/// `mod context` is private to `proxy`, so `crate::proxy::context::context_cache_stale_skips()`
/// resolves from `handler.rs`, which renders it on `/proxy/metrics` (big-fix 94 carry). The
/// per-request stats line stays with the stats owners.
pub fn context_cache_stale_skips() -> u64 {
    CONTEXT_CACHE_STALE_SKIPS.load(Ordering::Relaxed)
}

/// Fetch context total from backend with caching
///
/// Tries multiple endpoints to support different backend types:
/// 1. `/props` (llama.cpp) - extracts `default_generation_settings.n_ctx`
/// 2. `/v1/models` (vLLM, OpenAI-compatible) - extracts `data[0].max_model_len`
///
/// The cache is permanent for the lifetime of the application since context
/// size is a static server configuration. When a refresh is attempted and the
/// write lock is held, the accepted staleness window runs until one successful
/// refresh lands - see the policy documented on `cache_result` (big-fix 94).
///
/// Monitoring always targets the backend-native `/props` and `/v1/models`
/// paths; a configured request-path prefix must not alter these endpoints.
///
/// # Arguments
/// * `client` - The HTTP client to use for the request
/// * `backend_url` - The base URL of the backend server
/// * `_strip_path_prefix` - Accepted for caller compatibility, ignored
///
/// # Returns
/// * `Some(u64)` - The context size if successfully fetched
/// * `None` - If all fetch attempts failed or responses were malformed
pub async fn fetch_context_total(client: &reqwest::Client, backend_url: &str, _strip_path_prefix: Option<&str>) -> Option<u64> {
    let cache = CONTEXT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));

    // Check cache first
    {
        let read_guard = cache.read().await;
        if let Some(&(ctx, _)) = read_guard.get(backend_url) {
            return Some(ctx);
        }
    }

    // Try llama.cpp /props endpoint first
    if let Some(n_ctx) = fetch_from_props(client, backend_url, None).await {
        cache_result(cache, backend_url, n_ctx, BackendType::LlamaCpp);
        return Some(n_ctx);
    }

    // Fallback to vLLM/OpenAI-compatible /v1/models endpoint
    if let Some(max_model_len) = fetch_from_models(client, backend_url).await {
        cache_result(cache, backend_url, max_model_len, BackendType::Vllm);
        return Some(max_model_len);
    }

    None
}

/// Cache context size from preflight data, avoiding redundant HTTP calls.
///
/// Called by preflight after it has already fetched /v1/models and inspected
/// the `Server` response header to determine backend type. The probe carries
/// the node's api_key: llama.cpp `/props` is fetched WITH auth, so auth'd
/// backends stop 401-ing the fetch into a silent miss (big-fix E-M3).
///
/// - llama.cpp (`is_llama_cpp = true`): fetches `/props` for the actual configured n_ctx
///   (max_model_len from /v1/models is the training context, not the server's -c setting)
/// - Other backends: uses `max_model_len` already extracted from the /v1/models response
pub async fn cache_context_from_preflight(client: &reqwest::Client, probe: &ContextProbe) -> Option<u64> {
    let cache = CONTEXT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));

    // Check cache first (shouldn't be populated yet during preflight, but be safe)
    {
        let read_guard = cache.read().await;
        if let Some(&(ctx, _)) = read_guard.get(&probe.base_url) {
            return Some(ctx);
        }
    }

    if probe.is_llama_cpp {
        // Need /props for the actual runtime n_ctx (distinct from model's n_ctx_train)
        if let Some(n_ctx) = fetch_from_props(client, &probe.base_url, probe.api_key.as_deref()).await {
            cache_result(cache, &probe.base_url, n_ctx, BackendType::LlamaCpp);
            return Some(n_ctx);
        }
        None
    } else if let Some(ctx) = probe.max_model_len {
        cache_result(cache, &probe.base_url, ctx, BackendType::Vllm);
        Some(ctx)
    } else {
        None
    }
}

/// Fetch context size from llama.cpp `/props` endpoint.
/// `api_key` is presented when the caller's backend requires auth (E-M3).
async fn fetch_from_props(client: &reqwest::Client, backend_url: &str, api_key: Option<&str>) -> Option<u64> {
    // prefix: None on purpose — monitoring targets backend-native paths (task 15);
    // the shared builder is used so no site can re-invent the strip (E-L6).
    let props_url = node_url(backend_url, "/props", None);
    match with_auth(client.get(&props_url), api_key).send().await {
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
async fn fetch_from_models(client: &reqwest::Client, backend_url: &str) -> Option<u64> {
    let models_url = node_url(backend_url, "/v1/models", None);
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

/// Cache the result for future requests.
///
/// # Staleness policy (big-fix 94, C-M9)
/// The write uses `try_write` on purpose: a stats-path refresh must never queue behind
/// cache readers or make the request path wait for the lock. On contention we **serve
/// stale**: the fresh value is dropped and the cache keeps its previous entry, while the
/// fetching caller still returns its own freshly fetched value (both callers hand the
/// caller the fetch result, not the cache). Every dropped write increments
/// `CONTEXT_CACHE_STALE_SKIPS` and logs at debug, so a skipped refresh is observable,
/// never a silent discard.
///
/// **Accepted staleness window: until one successful refresh.** The skipped write is not
/// lost policy-wise - the next cache-miss fetch re-attempts it, and the first `try_write`
/// that lands (lock free, as in the common case) installs the fresh value and closes the
/// window.
fn cache_result(cache: &RwLock<HashMap<String, (u64, BackendType)>>, backend_url: &str, value: u64, backend_type: BackendType) {
    if let Ok(mut write_guard) = cache.try_write() {
        write_guard.insert(backend_url.to_string(), (value, backend_type));
    } else {
        let stale_skips_total = CONTEXT_CACHE_STALE_SKIPS.fetch_add(1, Ordering::Relaxed) + 1;
        tracing::debug!(
            backend_url = %backend_url,
            dropped_context = value,
            stale_skips_total,
            "Context-cache refresh skipped under write-lock contention; serving the cached value until one successful refresh"
        );
    }
}

/// Warn at most once per backend URL that context size could not be determined.
///
/// This prevents log spam when a backend doesn't support `/props` or `/v1/models` endpoints.
/// The warning is logged exactly once per backend URL, not on every request.
///
/// # Concurrency
/// Exactly-once holds even under concurrent callers: the read lock is only a fast path
/// for backends already warned about, and the decision to log is made from the return
/// value of `HashSet::insert` while holding the write lock. Racing callers therefore
/// serialize on that lock and only the one whose insert returns `true` logs.
pub async fn warn_context_fetch_failed_once(backend_url: &str, model: &str) {
    let warned = WARNED_BACKENDS.get_or_init(|| RwLock::new(HashSet::new()));

    // Check first with read lock (fast path for already-warned backends)
    {
        let read_guard = warned.read().await;
        if read_guard.contains(backend_url) {
            return;
        }
    }

    // Upgrade to write lock - check again and insert atomically
    let mut write_guard = warned.write().await;
    if write_guard.insert(backend_url.to_string()) {
        // We're the first to warn about this backend
        tracing::warn!(
            backend_url = %backend_url,
            model = %model,
            "Could not determine context size from backend (neither /props nor /v1/models returned usable data). \
             This backend may not support context size reporting. \
             Context window metrics will be incomplete for this backend. \
             Supported backends: llama.cpp (/props), vLLM (/v1/models with max_model_len)"
        );
    }
    // If insert() returned false, another task already warned - just return
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;
    use tracing_subscriber::fmt::MakeWriter;

    const PROPS_BODY: &str = r#"{"default_generation_settings":{"n_ctx":4096}}"#;
    const MODELS_BODY: &str = r#"{"data":[{"max_model_len":8192}]}"#;

    /// Spawn a minimal HTTP/1.1 listener on an ephemeral 127.0.0.1 port that records
    /// every request URI and answers with the JSON body the monitoring fetch expects.
    ///
    /// - `serve_props = true`:  `/props` -> 200 props JSON, anything else -> 404
    /// - `serve_props = false`: `/props` -> 404 (force `/v1/models` fallback), anything else -> 200 models JSON
    ///
    /// Returns (base_url, uri_receiver, task handle). Caller aborts the handle to shut down.
    async fn spawn_monitor_listener(serve_props: bool) -> (String, mpsc::Receiver<String>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral port");
        let port = listener.local_addr().expect("local_addr").port();
        let (tx, rx) = mpsc::channel::<String>(16);
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let mut head: Vec<u8> = Vec::new();
                let mut tmp = [0u8; 512];
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
                let uri = text
                    .lines()
                    .next()
                    .and_then(|line| line.split(' ').nth(1))
                    .unwrap_or("")
                    .to_string();
                if tx.send(uri.clone()).await.is_err() {
                    break;
                }
                let (status, body): (u16, String) = if uri == "/props" {
                    if serve_props {
                        (200, PROPS_BODY.to_string())
                    } else {
                        (404, "{}".to_string())
                    }
                } else if serve_props {
                    (404, "{}".to_string())
                } else {
                    (200, MODELS_BODY.to_string())
                };
                let reason = if status == 200 { "OK" } else { "NOT FOUND" };
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        (format!("http://127.0.0.1:{port}"), rx, handle)
    }

    /// Bounded recv so a missing request fails the test instead of hanging it.
    async fn next_uri(rx: &mut mpsc::Receiver<String>) -> String {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("listener request within 5s")
            .expect("uri channel open")
    }

    #[tokio::test]
    async fn test_monitoring_fetch_uses_backend_native_props_with_path_prefix() {
        let _serial = policy_lock().lock().await;
        // (module lock: see POLICY_TEST_SERIAL - queued write().await waiters here
        // dirty tokio's RwLock drop-handoff state and WouldBlock sibling try_write calls)
        let (base, mut rx, server) = spawn_monitor_listener(true).await;
        let client = reqwest::Client::new();

        let ctx = fetch_context_total(&client, &base, Some("/completions")).await;

        assert_eq!(ctx, Some(4096));
        let uri = next_uri(&mut rx).await;
        assert_eq!(
            uri, "/props",
            "monitoring must request backend-native /props even when path_prefix is configured"
        );
        server.abort();

        // Adversarial edge: trailing-slash prefix must not alter the monitoring path either.
        let (base2, mut rx2, server2) = spawn_monitor_listener(true).await;
        let ctx2 = fetch_context_total(&client, &base2, Some("/completions/")).await;
        assert_eq!(ctx2, Some(4096));
        assert_eq!(next_uri(&mut rx2).await, "/props");
        server2.abort();
    }

    #[tokio::test]
    async fn test_monitoring_fetch_uses_backend_native_v1_models_with_v1_prefix() {
        let _serial = policy_lock().lock().await;
        // (module lock: see POLICY_TEST_SERIAL - queued write().await waiters here
        // dirty tokio's RwLock drop-handoff state and WouldBlock sibling try_write calls)
        let (base, mut rx, server) = spawn_monitor_listener(false).await;
        let client = reqwest::Client::new();

        let ctx = fetch_context_total(&client, &base, Some("/v1")).await;

        assert_eq!(ctx, Some(8192), "/v1/models fallback must succeed");
        assert_eq!(next_uri(&mut rx).await, "/props", "props is tried first");
        let models_uri = next_uri(&mut rx).await;
        assert_eq!(
            models_uri, "/v1/models",
            "monitoring must request backend-native /v1/models, not the prefix-stripped path"
        );
        server.abort();
    }

    /// Auth-gated listener: /props answers 401 without `Authorization: Bearer secret`,
    /// 200 props JSON with it. Records (uri, authed) per request.
    async fn spawn_auth_props_listener() -> (String, mpsc::Receiver<(String, bool)>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral port");
        let port = listener.local_addr().expect("local_addr").port();
        let (tx, rx) = mpsc::channel::<(String, bool)>(16);
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let mut head: Vec<u8> = Vec::new();
                let mut tmp = [0u8; 512];
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
                let uri = text
                    .lines()
                    .next()
                    .and_then(|line| line.split(' ').nth(1))
                    .unwrap_or("")
                    .to_string();
                let authed = text.to_ascii_lowercase().contains("authorization: bearer secret");
                if tx.send((uri.clone(), authed)).await.is_err() {
                    break;
                }
                let props_ok = uri == "/props" && authed;
                let (status, body): (&str, &str) = if props_ok {
                    ("200 OK", PROPS_BODY)
                } else if uri == "/props" {
                    ("401 UNAUTHORIZED", r#"{"error":"unauthorized"}"#)
                } else {
                    ("404 NOT FOUND", "{}")
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        (format!("http://127.0.0.1:{port}"), rx, handle)
    }

    async fn next_recorded(rx: &mut mpsc::Receiver<(String, bool)>) -> (String, bool) {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("listener request within 5s")
            .expect("record channel open")
    }

    #[tokio::test]
    async fn preflight_props_fetch_presents_the_nodes_api_key() {
        let _serial = policy_lock().lock().await;
        // (module lock: see POLICY_TEST_SERIAL - queued write().await waiters here
        // dirty tokio's RwLock drop-handoff state and WouldBlock sibling try_write calls)
        let (base, mut rx, server) = spawn_auth_props_listener().await;
        let client = reqwest::Client::new();
        let probe = ContextProbe {
            base_url: base.clone(),
            is_llama_cpp: true,
            max_model_len: None,
            api_key: Some("secret".to_string()),
        };

        let ctx = cache_context_from_preflight(&client, &probe).await;

        assert_eq!(ctx, Some(4096), "an auth'd backend's /props must not be a silent miss");
        let (uri, authed) = next_recorded(&mut rx).await;
        assert_eq!(uri, "/props");
        assert!(authed, "the /props request must carry the node's bearer token (E-M3)");
        server.abort();
    }

    #[tokio::test]
    async fn auth_required_props_without_a_key_is_a_clean_miss() {
        let _serial = policy_lock().lock().await;
        // (module lock: see POLICY_TEST_SERIAL - queued write().await waiters here
        // dirty tokio's RwLock drop-handoff state and WouldBlock sibling try_write calls)
        let (base, mut rx, server) = spawn_auth_props_listener().await;
        let client = reqwest::Client::new();
        let probe = ContextProbe {
            base_url: base.clone(),
            is_llama_cpp: true,
            max_model_len: None,
            api_key: None,
        };

        let ctx = cache_context_from_preflight(&client, &probe).await;

        assert_eq!(ctx, None, "a 401 /props must degrade to None, not panic");
        let (uri, authed) = next_recorded(&mut rx).await;
        assert_eq!(
            (uri.as_str(), authed),
            ("/props", false),
            "no key configured -> no header sent"
        );
        server.abort();
    }

    // ---- big-fix 94: context_total staleness policy ----

    /// Capture writer (repo convention, 3rd instance: fixes/registry.rs, augment.rs):
    /// thread-local `set_default` + `flavor = "current_thread"` per the task-48 lesson.
    #[derive(Clone)]
    struct CaptureWriter(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("capture lock").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> MakeWriter<'a> for CaptureWriter {
        type Writer = Self;
        fn make_writer(&self) -> Self {
            self.clone()
        }
    }

    /// The five staleness-policy tests share the process-global CONTEXT_CACHE. T1 holds a
    /// read guard ACROSS an HTTP fetch (the only macroscopic lock hold in the suite), so
    /// a sibling test's try_write would be skipped by it (observed in red runs). Serializing
    /// the five makes every cache-state and log assertion race-free against THIS module.
    /// The residual cross-module actor (backends/preflight.rs tests reach cache_result via
    /// cache_context_from_preflight and cannot take this module-private lock) is handled at
    /// the assertion level: a refresh either LANDS or its skip is COUNTED - both branches
    /// assert the C-M9 policy; only silent loss fails the test.
    static POLICY_TEST_SERIAL: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

    fn policy_lock() -> &'static tokio::sync::Mutex<()> {
        POLICY_TEST_SERIAL.get_or_init(|| tokio::sync::Mutex::const_new(()))
    }

    fn install_debug_capture() -> (Arc<Mutex<Vec<u8>>>, tracing::subscriber::DefaultGuard) {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let guard = tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_ansi(false)
                .with_max_level(tracing::Level::DEBUG)
                .with_writer(CaptureWriter(buf.clone()))
                .finish(),
        );
        (buf, guard)
    }

    /// `/props` answers 200 with `props_body`; every other URI answers 200 with
    /// `models_body`. Both bodies can be malformed to drive the parse-failure paths.
    async fn spawn_two_endpoint_listener(
        props_body: &'static str,
        models_body: &'static str,
    ) -> (String, mpsc::Receiver<String>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral port");
        let port = listener.local_addr().expect("local_addr").port();
        let (tx, rx) = mpsc::channel::<String>(16);
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let mut head: Vec<u8> = Vec::new();
                let mut tmp = [0u8; 512];
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
                let uri = text
                    .lines()
                    .next()
                    .and_then(|line| line.split(' ').nth(1))
                    .unwrap_or("")
                    .to_string();
                if tx.send(uri.clone()).await.is_err() {
                    break;
                }
                let body = if uri == "/props" { props_body } else { models_body };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        (format!("http://127.0.0.1:{port}"), rx, handle)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_skipped_under_contention_is_counted_and_logged() {
        let _serial = policy_lock().lock().await;
        // Deterministic contention (no timing races): the test holds ONE read guard, so
        // the request-path refresh must fail its try_write.
        let (base, mut rx, server) = spawn_monitor_listener(true).await;
        let client = reqwest::Client::new();
        let (buf, _sub) = install_debug_capture();
        let cache = CONTEXT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));

        // Prewarm the skip callsite UNDER the capture subscriber: tracing Interest is
        // cached process-wide at first hit (task-48 lesson), so the first else-branch hit
        // must happen here, on this thread, with a live DEBUG subscriber. The read guard
        // below is dropped before the real window, so only this test's guard can ever
        // create contention for this callsite in the whole suite.
        {
            let _warm_reader = cache.read().await;
            cache_result(cache, "http://prewarm.contention.test", 1, BackendType::LlamaCpp);
        }
        let before = context_cache_stale_skips();

        {
            let _reader = cache.read().await;
            let served = fetch_context_total(&client, &base, None).await;
            assert_eq!(served, Some(4096), "the fetching caller still receives the fresh value");
            assert!(!cache.read().await.contains_key(&base), "the contended write must not land");
            assert!(
                context_cache_stale_skips() > before,
                "the skip must be counted (>= one; the global counter may also see rare foreign collisions with this guard window)"
            );
            assert_eq!(
                next_uri(&mut rx).await,
                "/props",
                "the refresh fetch really ran before the write was dropped"
            );
        }

        // Exact attribution, read BEFORE the phase-2 fetch: the thread-local capture sees
        // only this thread's events - the prewarm skip + exactly one skip naming this url.
        let log = String::from_utf8_lossy(&buf.lock().expect("capture lock").clone()).to_string();
        let skip_lines: Vec<&str> = log
            .lines()
            .filter(|line| line.contains("Context-cache refresh skipped"))
            .collect();
        assert_eq!(
            skip_lines.iter().filter(|line| line.contains(&base)).count(),
            1,
            "the skip must be logged at debug exactly once for the contended backend, never silent. Captured:\n{log}"
        );
        assert_eq!(
            skip_lines.len(),
            2,
            "exactly prewarm + this test's skip on this thread:\n{log}"
        );
        assert!(
            skip_lines.iter().any(|line| line.contains("dropped_context=4096")),
            "the log must name the dropped value:\n{log}"
        );
        assert!(
            skip_lines.iter().any(|line| line.contains("http://prewarm.contention.test")),
            "prewarm line must anchor the callsite in this capture:\n{log}"
        );

        // Accepted window ends at ONE successful refresh: the next cache-miss fetch either
        // LANDS the fresh value, or its try_write contends (the C-M9 class - possibly with
        // a cross-module refresh this module lock cannot fence) and that skip is COUNTED,
        // after which a further in-window fetch lands it. Silent loss is the only outcome
        // this branch rejects.
        let before2 = context_cache_stale_skips();
        let served = fetch_context_total(&client, &base, None).await;
        assert_eq!(served, Some(4096));
        if cache.read().await.get(&base).is_none() {
            assert!(
                context_cache_stale_skips() > before2,
                "a refresh that does not land must be counted, never silently lost"
            );
            let served2 = fetch_context_total(&client, &base, None).await;
            assert_eq!(served2, Some(4096), "the retried in-window fetch must serve the fresh value");
        }
        assert_eq!(
            cache.read().await.get(&base),
            Some(&(4096, BackendType::LlamaCpp)),
            "one successful refresh within the window must end it"
        );
        server.abort();
    }

    #[tokio::test]
    async fn cached_context_is_served_within_the_accepted_window_then_refreshes_on_miss() {
        let _serial = policy_lock().lock().await;
        // Given a cache entry older than the backend's live value (llama-server restarted
        // with a different -c): the accepted-window policy serves the cached value and
        // does NOT refetch on the hit path.
        let (base, mut rx, server) = spawn_monitor_listener(false).await; // live backend: 8192 via /v1/models
        let client = reqwest::Client::new();
        let cache = CONTEXT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
        cache.write().await.insert(base.clone(), (4096, BackendType::LlamaCpp));

        let served = fetch_context_total(&client, &base, None).await;
        assert_eq!(
            served,
            Some(4096),
            "within the window the cached value is served, not refetched"
        );
        assert!(rx.try_recv().is_err(), "a cache hit must not touch the backend");

        // The window ends at one successful refresh: once the entry is gone (the miss
        // state the policy relies on), the next fetch reads the LIVE backend. Its write
        // lands, or (C-M9 contention class, e.g. a cross-module preflight refresh) the
        // skip is counted and a further in-window fetch lands it - never silent.
        cache.write().await.remove(&base);
        let before_b = context_cache_stale_skips();
        let served = fetch_context_total(&client, &base, None).await;
        assert_eq!(served, Some(8192), "post-window refresh must serve the fresh value");
        assert_eq!(next_uri(&mut rx).await, "/props", "props is tried first on the miss");
        assert_eq!(next_uri(&mut rx).await, "/v1/models");
        if cache.read().await.get(&base).is_none() {
            assert!(
                context_cache_stale_skips() > before_b,
                "a refresh that does not land must be counted, never silently lost"
            );
            let served2 = fetch_context_total(&client, &base, None).await;
            assert_eq!(served2, Some(8192), "the retried in-window fetch must serve the fresh value");
        }
        assert_eq!(cache.read().await.get(&base), Some(&(8192, BackendType::Vllm)));
        server.abort();
    }

    #[tokio::test]
    async fn stalled_backend_fetch_is_bounded_by_the_injected_client_timeout() {
        let _serial = policy_lock().lock().await;
        // A stalled backend must not hang the fetch forever: the budget is the
        // caller-injected reqwest client timeout (context.rs holds no lock across awaits).
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral port");
        let port = listener.local_addr().expect("local_addr").port();
        let stall = tokio::spawn(async move {
            let mut held: Vec<tokio::net::TcpStream> = Vec::new();
            loop {
                let Ok((sock, _)) = listener.accept().await else { break };
                held.push(sock); // accept, never respond, never drop: every request stalls
            }
        });
        let base = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(300))
            .build()
            .expect("timeout client");

        let started = std::time::Instant::now();
        let ctx = tokio::time::timeout(Duration::from_secs(10), fetch_context_total(&client, &base, None))
            .await
            .expect("fetch must return within 10s, never hang");
        let elapsed = started.elapsed();

        assert_eq!(ctx, None, "a stalled backend must degrade to None");
        assert!(
            elapsed >= Duration::from_millis(550),
            "both endpoints must consume their 300ms budgets: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "must finish far inside the hang bound: {elapsed:?}"
        );

        let cache = CONTEXT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
        assert!(
            !cache.read().await.contains_key(&base),
            "a failed fetch must not poison the cache"
        );
        stall.abort();
    }

    #[tokio::test]
    async fn malformed_props_body_falls_back_to_models_without_poisoning() {
        let (base, mut rx, server) =
            spawn_two_endpoint_listener(r#"{"default_generation_settings":{"n_ctx":"four-k"}}"#, MODELS_BODY).await;
        let _serial = policy_lock().lock().await;
        let client = reqwest::Client::new();
        let before_a = context_cache_stale_skips();

        let ctx = fetch_context_total(&client, &base, None).await;

        assert_eq!(
            ctx,
            Some(8192),
            "a type-malformed n_ctx must fall through to /v1/models, honestly"
        );
        assert_eq!(next_uri(&mut rx).await, "/props");
        assert_eq!(next_uri(&mut rx).await, "/v1/models");
        let cache = CONTEXT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
        if cache.read().await.get(&base).is_none() {
            assert!(
                context_cache_stale_skips() > before_a,
                "a fallback refresh that does not land must be counted, never silently lost"
            );
            let served2 = fetch_context_total(&client, &base, None).await;
            assert_eq!(served2, Some(8192), "the retried in-window fetch must serve the fresh value");
        }
        assert_eq!(
            cache.read().await.get(&base),
            Some(&(8192, BackendType::Vllm)),
            "only the parsed value is cached - never a zero or the malformed shape"
        );
        server.abort();
    }

    #[tokio::test]
    async fn garbage_on_both_endpoints_yields_none_without_poisoning() {
        let (base, mut rx, server) = spawn_two_endpoint_listener("not json at all", "also not json").await;
        let _serial = policy_lock().lock().await;
        let client = reqwest::Client::new();

        let ctx = fetch_context_total(&client, &base, None).await;

        assert_eq!(ctx, None, "both endpoints garbage -> honest None, no fabricated context size");
        assert_eq!(next_uri(&mut rx).await, "/props");
        assert_eq!(next_uri(&mut rx).await, "/v1/models");
        let cache = CONTEXT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
        assert!(!cache.read().await.contains_key(&base), "nothing is cached on total failure");
        server.abort();
    }

    #[tokio::test]
    async fn test_fetch_context_total_caching() {
        let _serial = policy_lock().lock().await;
        // (module lock: see POLICY_TEST_SERIAL - queued write().await waiters here
        // dirty tokio's RwLock drop-handoff state and WouldBlock sibling try_write calls)
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
