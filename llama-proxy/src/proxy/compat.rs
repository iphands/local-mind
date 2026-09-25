//! Client-management-endpoint shims.
//!
//! This module is the home for the small, client-specific HTTP endpoints that a plain
//! llama.cpp backend does not implement and that the proxy therefore answers ITSELF
//! instead of proxying a 404. Today that is exactly one endpoint — AnythingLLM's
//! `POST /api/models/vram-estimate` — and this file owns its three primitives:
//!
//! * [`is_vram_estimate_path`] — which request path the shim owns (routing itself is
//!   `handler.rs`'s job, plan todo 6),
//! * [`resolve_context_length`] — the backend's own advertised context window, and
//! * [`kv_cache_info`] — the backend's advertised KV-cache configuration, scraped once
//!   and cached,
//!
//! plus [`build_vram_estimate_body`], which renders the honest response body out of
//! those two. The pure label grammar lives in `kv_labels.rs`; the pure path predicate
//! and the pure body builder need no I/O, so only the two probes touch the network.
//!
//! # Honesty is the contract
//! The client reads ONE number out of this body: `context_length`, in tokens
//! (anything-llm @ `58ae6fee08e10e649ca13d0cfc55598d90422cf8`,
//! `server/utils/AiProviders/localAi/index.js:94-102`). Everything here is either a
//! value the backend itself advertised or absent. No VRAM figure is ever invented —
//! see the omission note on [`build_vram_estimate_body`].
//!
//! # Every probe is bounded
//! Both probes cap themselves at a couple of seconds. A management endpoint must not
//! be able to stall a client because a backend is wedged mid engine-profiling, and the
//! node's own client timeout is 300s by default (`src/config/mod.rs:134-136`), which is
//! a completion timeout, not a probe timeout.
//!
//! # Callers
//! `handler::route()` intercepts the path and owns the response; the four `pub(crate)`
//! items below are its only moving parts, and `kv_cache_info_for_test` is the test seam
//! for the retry window. Nothing else in the proxy may answer this path — forwarding it is
//! the 404 this module exists to remove.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use futures::StreamExt;
use tokio::sync::RwLock;

use super::context::fetch_context_total;
use super::kv_labels::{parse_cache_config_info, KvCacheInfo};
use crate::backends::{node_url, with_auth, BackendNode};

/// The ONE path this shim owns.
const SHIM_PATH: &str = "/api/models/vram-estimate";

/// Total budget for the context-window probe, `/props` and `/v1/models` together.
const CONTEXT_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Total budget for one `/metrics` scrape: connect through last byte.
const KV_SCRAPE_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a failed `/metrics` attempt is remembered before it may be retried.
///
/// This is a `const`, not a tunable: the value only has to be long enough that a
/// backend wedged in engine profiling is not hammered per prompt, and short enough
/// that a vLLM which starts AFTER the proxy stops being missing from the body for long.
/// Tests inject their own window through [`kv_cache_info_for_test`] instead of mutating
/// this (a mutable global would cross-contaminate the parallel suite, and `Instant`
/// cannot be moved backwards, so there is no clock to fake either).
const KV_RETRY_BACKOFF: Duration = Duration::from_secs(30);

/// Hard ceiling on the bytes scanned out of one `/metrics` stream. Past that the stream
/// is treated as untrustworthy rather than searched forever — a metrics endpoint that
/// answers with a hundred megabytes is not a KV cache report.
const KV_SCAN_CEILING_BYTES: usize = 16 * 1024 * 1024;

/// The `context_note` the client renders. Source-agnostic ON PURPOSE: the context cache
/// stores a bare `u64` with no source tag (the write-only source enum was deleted on
/// purpose, `context.rs:16-17`), so this body MUST NOT claim which endpoint the number
/// came from.
const CONTEXT_NOTE: &str = "reported by llama-proxy from the backend's own advertised context window";

/// The labels surfaced under `vllm`.
///
/// vLLM renders EVERY `CacheConfig` attribute as a label (`metrics_info()` is
/// `{k: str(v) for k, v in self.__dict__.items()}`, `vllm/config/cache.py:311-313`), and
/// that set includes byte-count attributes such as `cpu_kv_cache_size`. This shim MUST
/// NOT emit a VRAM/byte figure, so it projects the seven memory-relevant attributes
/// rather than dumping the whole label set, and each surviving value passes through
/// verbatim as a string.
const MEMORY_LABELS: [&str; 7] = [
    "num_gpu_blocks",
    "block_size",
    "kv_cache_size_tokens",
    "kv_cache_max_concurrency",
    "gpu_memory_utilization",
    "cache_dtype",
    "enable_prefix_caching",
];

/// Cached outcome of the `/metrics` scrape, keyed by `BackendNode::base_url()`.
enum KvLookup {
    /// The scrape completed with a 2xx stream. `None` means the stream was clean and the
    /// gauge was simply not there — a llama.cpp backend — and is a real, permanent answer.
    Known(Option<KvCacheInfo>),
    /// The scrape could not be trusted (transport error, non-2xx, timeout, truncated
    /// stream). Timestamped so the retry is rate-limited. NEVER collapses into `Known(None)`:
    /// a proxy that booted before vLLM finished engine profiling would otherwise lose the
    /// `vllm` object for the rest of its life.
    Failed(Instant),
}

/// What one scrape attempt produced, before it becomes a [`KvLookup`].
enum Scrape {
    Found(KvCacheInfo),
    /// A clean 2xx stream that never carried the gauge.
    Absent,
    /// An attempt that must not be remembered as an answer. The `&'static str` is the
    /// category for the warn-once line; the underlying error goes to `debug!`.
    Failed(&'static str),
}

/// Cache of scrape outcomes, plus the set of backends already warned about.
static KV_INFO: OnceLock<RwLock<HashMap<String, KvLookup>>> = OnceLock::new();
static KV_WARNED: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();

/// Whether `path` is the endpoint this shim answers.
///
/// EXACT-segment match on [`SHIM_PATH`], with or without one trailing slash. Never
/// `starts_with`, never `ends_with`, and specifically **not** a suffix match: a client
/// hitting `/gw/api/models/vram-estimate` against a node configured with
/// `strip_path_prefix: "/gw"` is already served `routed == /api/models/vram-estimate`,
/// because [`BackendNode::effective_path`] strips the prefix at a segment boundary
/// (`src/backends/node.rs:124-129`) and `route()` checks BOTH views. Making this
/// predicate suffix-match would let the proxy locally answer paths the operator
/// deliberately routed elsewhere — an over-match that silently swallows a real endpoint.
pub(crate) fn is_vram_estimate_path(path: &str) -> bool {
    path.strip_suffix('/').unwrap_or(path) == SHIM_PATH
}

/// The backend's own advertised context window, in tokens.
///
/// Delegates to the context monitor's existing two-endpoint probe (`/props`, then
/// `/v1/models`) — same cache, same backends, no second implementation — but with a
/// probe client of the shim's OWN: [`node.http_client`] carries the node's
/// `timeout_seconds` (300 by default), which is a completion budget and would let a
/// wedged backend stall this management request for five minutes.
///
/// `_strip_path_prefix` is `None` because monitoring is backend-native: a request-path
/// prefix a gateway needs must not rewrite `/props` or `/v1/models` (`context.rs:48-49`).
pub(crate) async fn resolve_context_length(node: &Arc<BackendNode>) -> Option<u64> {
    let client = probe_client()?;
    // The 2s client timeout bounds ONE request; this bound is what bounds the WHOLE
    // probe, because the fallback path is two requests (`/props` then `/v1/models`) and
    // a backend that accepts and never answers would otherwise cost 2s + 2s.
    tokio::time::timeout(
        CONTEXT_PROBE_TIMEOUT,
        fetch_context_total(client, node.base_url(), None),
    )
    .await
    .ok()
    .flatten()
}

/// The shim's probe client: 2s total, 2s connect, built once.
///
/// `Option` rather than an `expect`: a client that cannot be built is a startup-level
/// impossibility, but the response to it is "the shim reports no context", not a panic
/// on a request path.
fn probe_client() -> Option<&'static reqwest::Client> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(CONTEXT_PROBE_TIMEOUT)
                .connect_timeout(CONTEXT_PROBE_TIMEOUT)
                .build()
                .inspect_err(|error| {
                    tracing::error!(%error, "cannot build the shim's context-probe client; vram-estimate will report no context")
                })
                .ok()
        })
        .as_ref()
}

/// The backend's advertised KV-cache configuration, or `None` when it has none.
///
/// Cached for the life of the process: `vllm:cache_config_info` is a gauge that never
/// changes after engine profiling, and scraping `/metrics` on every prompt would make
/// the shim a per-prompt round trip to a backend that answers it with a megabyte of
/// counters. See [`KvLookup`] for why a failure is never cached as an answer.
pub(crate) async fn kv_cache_info(node: &Arc<BackendNode>) -> Option<KvCacheInfo> {
    kv_cache_info_with_backoff(node, KV_RETRY_BACKOFF).await
}

/// Test seam for the retry window. Production always uses [`KV_RETRY_BACKOFF`].
#[cfg(test)]
pub(crate) async fn kv_cache_info_for_test(node: &Arc<BackendNode>, retry_backoff: Duration) -> Option<KvCacheInfo> {
    kv_cache_info_with_backoff(node, retry_backoff).await
}

async fn kv_cache_info_with_backoff(node: &Arc<BackendNode>, retry_backoff: Duration) -> Option<KvCacheInfo> {
    let cache = KV_INFO.get_or_init(|| RwLock::new(HashMap::new()));
    let base_url = node.base_url();

    {
        let guard = cache.read().await;
        match guard.get(base_url) {
            Some(KvLookup::Known(info)) => return info.clone(),
            Some(KvLookup::Failed(last_attempt)) => {
                // `checked_duration_since` because a wall-clock adjustment must never
                // panic on a request path; "clock went backwards" means "retry now".
                let waited = Instant::now().checked_duration_since(*last_attempt).unwrap_or(Duration::ZERO);
                if waited < retry_backoff {
                    return None;
                }
            }
            None => {}
        }
    }

    let lookup = match scrape_kv_cache_info(node).await {
        Scrape::Found(info) => KvLookup::Known(Some(info.clone())),
        Scrape::Absent => KvLookup::Known(None),
        Scrape::Failed(reason) => {
            warn_kv_scrape_failed_once(base_url, reason).await;
            KvLookup::Failed(Instant::now())
        }
    };
    let returned = match &lookup {
        KvLookup::Known(info) => info.clone(),
        KvLookup::Failed(_) => None,
    };

    // `.write().await`, deliberately NOT context.rs's `try_write`-and-serve-stale
    // (`context.rs:190-202`). That pattern is tuned for a stats refresh that will simply
    // happen again on the next request, so dropping one write costs a stale window. This
    // is a WRITE-ONCE capability entry: a lost write is not a stale entry, it is the entry
    // NEVER BEING INSTALLED, and the next request pays for the whole scrape again — or,
    // for `Known(None)`, the once-per-process negative silently never lands. Waiting on
    // the lock here is correct because there is exactly one writer per backend per
    // lifetime (plus one retry per backoff window); there is nothing to shed.
    cache.write().await.insert(base_url.to_string(), lookup);
    returned
}

/// `GET /metrics`, bounded end to end, and scan it for the gauge.
async fn scrape_kv_cache_info(node: &Arc<BackendNode>) -> Scrape {
    match tokio::time::timeout(KV_SCRAPE_TIMEOUT, scrape_kv_stream(node)).await {
        Ok(scrape) => scrape,
        Err(_) => {
            tracing::debug!(backend_url = %node.base_url(), "vllm:cache_config_info scrape exceeded its 2s cap");
            Scrape::Failed("timeout")
        }
    }
}

/// Drive one `/metrics` response as a byte stream, line by line.
///
/// The stream is walked rather than buffered because the answer sits in the first
/// kilobyte of a body that can be megabytes large: reading it to the end would hold a
/// full copy of a backend's entire metrics surface in the proxy per scrape. Splitting on
/// `\n` and stopping at the first line the pure parser accepts is the whole point.
async fn scrape_kv_stream(node: &Arc<BackendNode>) -> Scrape {
    let url = node_url(node.base_url(), "/metrics", None);
    let request = with_auth(node.http_client.get(&url), node.api_key.as_deref());
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            tracing::debug!(backend_url = %node.base_url(), %url, %error, "could not request the backend's /metrics");
            return Scrape::Failed("transport error");
        }
    };
    if !response.status().is_success() {
        tracing::debug!(backend_url = %node.base_url(), status = %response.status(), "backend /metrics did not answer 2xx");
        return Scrape::Failed("non-2xx response");
    }

    let mut stream = response.bytes_stream();
    let mut pending: Vec<u8> = Vec::new();
    let mut scanned: usize = 0;
    loop {
        let chunk = match stream.next().await {
            Some(Ok(chunk)) => chunk,
            // The stream completed cleanly and the gauge never appeared: the honest
            // negative, and the ONLY path that earns a `Known(None)`.
            None => return Scrape::Absent,
            Some(Err(error)) => {
                tracing::debug!(backend_url = %node.base_url(), %error, "backend /metrics stream failed mid-body");
                return Scrape::Failed("stream error");
            }
        };
        scanned = scanned.saturating_add(chunk.len());
        if scanned > KV_SCAN_CEILING_BYTES {
            tracing::debug!(backend_url = %node.base_url(), scanned, "backend /metrics exceeded the scan ceiling before the gauge appeared");
            return Scrape::Failed("scan ceiling exceeded");
        }
        pending.extend_from_slice(&chunk);
        while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = pending.drain(..=newline).collect();
            // A complete line cannot straddle a chunk boundary once `\n` terminated it,
            // so the lossy decode below only ever sees bytes that were actually sent.
            if let Some(info) = parse_cache_config_info(String::from_utf8_lossy(&line).as_ref()) {
                return Scrape::Found(info);
            }
        }
    }
}

/// Warn at most once per backend that its `/metrics` could not be read.
///
/// Mirrors `warn_context_fetch_failed_once` (`context.rs:204-239`): the decision is the
/// return value of `HashSet::insert` taken under the write lock, so racing scrapes log
/// once between them. Unlike that function there is no read-lock fast path, because this
/// one is already rate-limited by [`KV_RETRY_BACKOFF`] — at most one attempt per backend
/// per window, not one per prompt.
async fn warn_kv_scrape_failed_once(base_url: &str, reason: &str) {
    let warned = KV_WARNED.get_or_init(|| RwLock::new(HashSet::new()));
    let mut guard = warned.write().await;
    if guard.insert(base_url.to_string()) {
        tracing::warn!(
            backend_url = %base_url,
            reason,
            "Could not read vllm:cache_config_info from the backend's /metrics endpoint. \
             The vram-estimate response will carry the context window but no `vllm` object. \
             This is expected for a backend that is not vLLM; for a vLLM backend it means the \
             endpoint is unreachable, unauthorised, or the engine had not finished profiling \
             when the probe ran. The scrape will be retried after the backoff window."
        );
    }
}

/// The response body for `POST /api/models/vram-estimate`.
///
/// Exactly four top-level keys, plus `vllm` when the backend advertised a KV cache.
///
/// `size_bytes` / `size_display` / `vram_bytes` / `vram_display` are ABSENT, and that is
/// deliberate and honest, not an oversight: they are not obtainable over vLLM's HTTP
/// surface at all — the KV-cache memory figure is printed to vLLM's stdout log during
/// engine profiling and never served by `/metrics`, `/props` or any other endpoint — and
/// AnythingLLM never reads them: it takes ONLY `context_length` out of this body
/// (anything-llm @ `58ae6fee08e10e649ca13d0cfc55598d90422cf8`,
/// `server/utils/AiProviders/localAi/index.js:94-102`). So there is no number to
/// fabricate, no `nvidia-smi`/NVML call to make, and no stdout to parse; the field list
/// LocalAI's `pkg/vram/types.go:46-58` declares is a superset this backend simply cannot
/// fill, and a proxy that invented one would be presenting a guess as a measurement.
pub(crate) fn build_vram_estimate_body(model: &str, context_length: u64, kv: Option<&KvCacheInfo>) -> serde_json::Value {
    // No `id` key, ever: the client does `{ id, ...est }`, so an `id` here would
    // OVERWRITE the one it already bound from `/v1/models`.
    let mut body = serde_json::Map::new();
    body.insert("model".to_string(), serde_json::Value::String(model.to_string()));
    body.insert("context_length".to_string(), serde_json::json!(context_length));
    body.insert("model_max_context".to_string(), serde_json::json!(context_length));
    body.insert("context_note".to_string(), serde_json::Value::String(CONTEXT_NOTE.to_string()));
    if let Some(kv) = kv {
        let mut vllm = serde_json::Map::new();
        for label in MEMORY_LABELS {
            if let Some(value) = kv.labels().get(label) {
                // `Value::String`, never `json!(value)`: vLLM prints every CacheConfig
                // attribute through `str()`, and a "5080" that arrives as a number would
                // be a value the backend never advertised.
                vllm.insert(label.to_string(), serde_json::Value::String(value.clone()));
            }
        }
        body.insert("vllm".to_string(), serde_json::Value::Object(vllm));
    }
    serde_json::Value::Object(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::BackendNode;
    use crate::proxy::kv_labels::parse_cache_config_info;
    use crate::proxy::test_support::{json_response, scripted_backend, scripted_response};
    use serde_json::{json, Value};
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::mpsc;

    /// A llama.cpp-shaped `/metrics` body: a clean 200 carrying no vLLM gauge at all.
    const LLAMA_METRICS_BODY: &str = concat!(
        "# HELP llama_cpp_prompt_tokens_total Total number of prompt tokens processed\n",
        "# TYPE llama_cpp_prompt_tokens_total counter\n",
        "llama_cpp_prompt_tokens_total{model=\"qwen3\"} 1.234567e+06\n",
        "llama_cpp_predicted_tokens_total{model=\"qwen3\"} 9.876543e+05\n",
    );

    /// The gauge as vLLM prints it: the seven memory-relevant labels plus `engine`,
    /// every value a quoted string, followed by an unrelated family.
    const VLLM_METRICS_BODY: &str = concat!(
        "# HELP vllm:cache_config_info Information of the LLMEngine CacheConfig\n",
        "# TYPE vllm:cache_config_info gauge\n",
        r#"vllm:cache_config_info{block_size="16",cache_dtype="auto",enable_prefix_caching="True",engine="0",gpu_memory_utilization="0.9",kv_cache_max_concurrency="42",kv_cache_size_tokens="81280",num_gpu_blocks="5080"} 1"#,
        "\n",
        r#"vllm:prompt_tokens_total{engine="0",model_name="qwen3"} 1.0e+04"#,
        "\n",
    );

    const PROPS_4096_BODY: &str = r#"{"default_generation_settings":{"n_ctx":4096}}"#;
    const MODELS_262144_BODY: &str = r#"{"data":[{"id":"qwen3","max_model_len":262144}]}"#;

    /// A node pointed at `port`, carrying the PRODUCTION default 300s request timeout
    /// so the tests prove the shim's own probe cap dominates the node's client.
    fn node_for(base_url: &str, api_key: Option<&str>) -> Arc<BackendNode> {
        Arc::new(
            BackendNode::from_config(
                base_url.to_string(),
                300,
                None,
                None,
                api_key.map(str::to_string),
                None,
                None,
            )
            .expect("a plain http node with no TLS config always builds"),
        )
    }

    fn url_for(port: u16) -> String {
        format!("http://127.0.0.1:{port}")
    }

    /// Build the per-path route table `scripted_backend` consumes.
    fn routes(pairs: Vec<(&str, Vec<Vec<u8>>)>) -> HashMap<String, Vec<Vec<u8>>> {
        pairs.into_iter().map(|(path, queue)| (path.to_string(), queue)).collect()
    }

    /// Every object key at every depth of a JSON body.
    fn collect_keys(value: &Value, out: &mut Vec<String>) {
        if let Some(map) = value.as_object() {
            for (key, child) in map {
                out.push(key.clone());
                collect_keys(child, out);
            }
        }
    }

    /// One per-connection decision made by [`spawn_compat_listener`].
    #[derive(Clone)]
    enum Reply {
        /// Write a complete HTTP/1.1 frame, then close.
        Frame(Vec<u8>),
        /// Read the request head, then NEVER answer: the client has to give up.
        Hang,
        /// Read the request head, then close WITHOUT any response: a transport error.
        CloseSilent,
    }

    /// A minimal listener the KV/context tests drive directly, because the todo-1
    /// scripted seam can only ever answer with a well-formed frame and these tests
    /// need the transport-error and hang classes too. Each connection consumes the next
    /// reply; running out panics in the task (loud, and it frees the port).
    ///
    /// `require_auth` answers 401 for any request that does not carry
    /// `Authorization: Bearer secret` — such a request never consumes a reply, so a
    /// shim that dropped the node's api_key gets a non-2xx scrape instead of the gauge.
    async fn spawn_compat_listener(replies: Vec<Reply>, require_auth: bool) -> CompatListener {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral port");
        let port = listener.local_addr().expect("local_addr").port();
        let accepts = Arc::new(AtomicUsize::new(0));
        let (tx, seen) = mpsc::unbounded_channel();
        let task_accepts = Arc::clone(&accepts);
        let mut queue: VecDeque<Reply> = replies.into();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                task_accepts.fetch_add(1, Ordering::SeqCst);
                let Some((method, uri, authed)) = read_request_head(&mut sock).await else {
                    continue;
                };
                let _ = tx.send((format!("{method} {uri}"), authed));
                let frame = if require_auth && !authed {
                    Some(json_response("401 Unauthorized", r#"{"error":"unauthorized"}"#))
                } else {
                    match queue.pop_front() {
                        Some(Reply::Frame(frame)) => Some(frame),
                        Some(Reply::Hang) => {
                            tokio::time::sleep(Duration::from_secs(30)).await;
                            None
                        }
                        Some(Reply::CloseSilent) => None,
                        None => panic!("compat listener ran out of replies for {method} {uri}"),
                    }
                };
                if let Some(frame) = frame {
                    let _ = sock.write_all(&frame).await;
                    let _ = sock.flush().await;
                }
                let _ = sock.shutdown().await;
            }
        });
        CompatListener {
            base_url: url_for(port),
            accepts,
            seen,
            task,
        }
    }

    struct CompatListener {
        base_url: String,
        accepts: Arc<AtomicUsize>,
        seen: mpsc::UnboundedReceiver<(String, bool)>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for CompatListener {
        fn drop(&mut self) {
            // Deterministic teardown: the aborted task drops the listener, so the
            // ephemeral port is back before the next test asks the OS for one.
            self.task.abort();
        }
    }

    /// Read the request head; return `(method, request-target, carries_bearer_secret)`.
    async fn read_request_head(sock: &mut TcpStream) -> Option<(String, String, bool)> {
        let mut head: Vec<u8> = Vec::new();
        let mut buf = [0u8; 512];
        loop {
            let Ok(Ok(read)) = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut buf)).await else {
                return None;
            };
            if read == 0 {
                return None;
            }
            head.extend_from_slice(&buf[..read]);
            if head.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let text = String::from_utf8_lossy(&head).to_string();
        let mut request_line = text.lines().next()?.split_whitespace();
        let method = request_line.next()?.to_string();
        let uri = request_line.next()?.to_string();
        let authed = text.to_ascii_lowercase().contains("authorization: bearer secret");
        Some((method, uri, authed))
    }

    /// Bounded recv so a missing request fails the test instead of hanging it.
    async fn next_seen(listener: &mut CompatListener) -> (String, bool) {
        tokio::time::timeout(Duration::from_secs(5), listener.seen.recv())
            .await
            .expect("listener request within 5s")
            .expect("record channel open")
    }

    fn vllm_metrics_frame() -> Vec<u8> {
        scripted_response(
            "200 OK",
            &[("content-type", "text/plain; version=0.0.4")],
            VLLM_METRICS_BODY.as_bytes(),
        )
    }

    // ---------------------------------------------------------------- path predicate

    #[test]
    fn path_predicate_matches_only_the_shim_path() {
        // Then: the bare path and its trailing-slash spelling are the ONLY two the shim
        // claims (AnythingLLM posts the bare form; a proxy in front of a slash-normalising
        // gateway can present either).
        assert!(is_vram_estimate_path("/api/models/vram-estimate"));
        assert!(is_vram_estimate_path("/api/models/vram-estimate/"));
    }

    #[test]
    fn path_predicate_rejects_prefixed_and_lookalike_paths() {
        // Given: the gateway spelling is NOT the shim's. `effective_path` already strips
        // the operator's `strip_path_prefix` at a segment boundary, so a `/gw/...` request
        // reaches `route()` as BOTH the original path and the stripped one; matching the
        // suffix here would silently swallow an endpoint the operator routed elsewhere.
        assert!(!is_vram_estimate_path("/gw/api/models/vram-estimate"));
        assert!(!is_vram_estimate_path("/api/models/vram-estimate/extra"));
        assert!(!is_vram_estimate_path("/api/models/vram-estimate-x"));
        assert!(!is_vram_estimate_path("/api/models/vram_estimated"));
        assert!(!is_vram_estimate_path("/api/models/"));
        assert!(!is_vram_estimate_path("/"));
        assert!(!is_vram_estimate_path(""));
    }

    // ------------------------------------------------------------------ body builder

    #[test]
    fn build_body_omits_vram_and_id_keys() {
        // Given: a real vLLM label set (values are strings, `num_gpu_blocks` included).
        let kv = parse_cache_config_info(VLLM_METRICS_BODY).expect("the fixture carries the gauge");

        // When: the shim body is built for a resolved context length.
        let body = build_vram_estimate_body("qwen3", 262144, Some(&kv));
        println!("vLLM-shaped shim body = {body}");

        // Then: no `id` — AnythingLLM does `{ id, ...est }`, so an `id` here would
        // OVERWRITE the one it already bound from /v1/models.
        for forbidden in ["id", "vram_bytes", "vram_display", "size_bytes", "size_display"] {
            assert!(body.get(forbidden).is_none(), "`{forbidden}` must not exist: {body}");
        }
        assert!(body["context_length"].is_number(), "the one field the client reads must be a number: {body}");

        // Then: no byte/VRAM-flavoured key at ANY depth, including inside `vllm`.
        let mut keys = Vec::new();
        collect_keys(&body, &mut keys);
        for key in &keys {
            assert!(
                !key.contains("vram") && !key.contains("byte"),
                "byte/VRAM key `{key}` appeared in the body: {body}"
            );
        }

        // Then: the nested note carries the gauge's values VERBATIM as strings.
        assert_eq!(body["vllm"]["num_gpu_blocks"], json!("5080"), "a label must not be coerced to a number");
        assert_eq!(body["vllm"]["kv_cache_size_tokens"], json!("81280"));
        assert_eq!(body["vllm"]["enable_prefix_caching"], json!("True"));
        assert_eq!(
            body["vllm"].as_object().expect("vllm is an object").len(),
            7,
            "exactly the seven memory-relevant labels — `engine` selects the series, it is not a memory figure"
        );
    }

    #[test]
    fn build_body_omits_vllm_object_when_kv_absent() {
        // When: the backend advertised no KV cache (a llama.cpp node).
        let body = build_vram_estimate_body("qwen3", 4096, None);
        println!("llama.cpp-shaped shim body = {body}");

        // Then: the object is ABSENT, not empty — an empty `vllm` object would claim a
        // scrape happened and found nothing worth reporting.
        assert!(body.get("vllm").is_none(), "no kv means no vllm object: {body}");
        assert_eq!(body["model"], json!("qwen3"));
        assert_eq!(body["context_length"], json!(4096));
        assert_eq!(body["model_max_context"], json!(4096), "same number, LocalAI's field name");
        assert_eq!(body.as_object().expect("object").len(), 4, "exactly the four documented keys: {body}");

        // Then: the note stays SOURCE-AGNOSTIC. The context cache stores a bare u64 with
        // no source tag (deliberately deleted, context.rs:16-17), so the body must not
        // claim which endpoint the number came from.
        let note = body["context_note"].as_str().expect("context_note is a string");
        assert!(!note.contains("/props") && !note.contains("max_model_len"), "note names an endpoint it cannot know: {note}");
        assert!(!note.is_empty());
    }

    // ---------------------------------------------------------- context length probe

    #[tokio::test]
    async fn resolve_context_length_reads_props_then_models() {
        // Given: a llama.cpp-shaped backend (its own port — CONTEXT_CACHE is global).
        let (props_port, props_state) = scripted_backend(routes(vec![(
            "/props",
            vec![json_response("200 OK", PROPS_4096_BODY)],
        )]))
        .await;

        // When: the shim resolves the context window.
        let from_props = resolve_context_length(&node_for(&url_for(props_port), None)).await;

        // Then: llama.cpp's /props answers first and nothing else is probed.
        assert_eq!(from_props, Some(4096));
        assert_eq!(props_state.requests(), vec![("GET".to_string(), "/props".to_string())]);

        // Given: a vLLM-shaped backend, where /props is a 404.
        let (models_port, models_state) = scripted_backend(routes(vec![
            ("/props", vec![json_response("404 Not Found", r#"{"detail":"Not Found"}"#)]),
            ("/v1/models", vec![json_response("200 OK", MODELS_262144_BODY)]),
        ]))
        .await;

        // When:
        let from_models = resolve_context_length(&node_for(&url_for(models_port), None)).await;

        // Then: the fallback yields the advertised window.
        assert_eq!(from_models, Some(262144));
        assert_eq!(
            models_state.requests(),
            vec![
                ("GET".to_string(), "/props".to_string()),
                ("GET".to_string(), "/v1/models".to_string()),
            ],
            "props is tried first, then the OpenAI-compatible fallback"
        );
    }

    #[tokio::test]
    async fn resolve_context_length_gives_up_within_the_probe_cap() {
        // Given: a backend that accepts connections and never answers.
        let stuck = spawn_compat_listener(vec![Reply::Hang; 4], false).await;
        let node = node_for(&stuck.base_url, None);

        // When: the shim probes it (node client timeout is 300s, so ONLY the shim's own
        // cap can bound this).
        let started = Instant::now();
        let resolved = resolve_context_length(&node).await;
        let elapsed = started.elapsed();
        println!("hung probe: result={resolved:?} after {elapsed:?}, accepts={}", stuck.accepts.load(Ordering::SeqCst));

        // Then: an honest None, fast — a wedged backend cannot stall the shim.
        assert_eq!(resolved, None, "a hung backend must yield None, not a hang");
        assert!(elapsed < Duration::from_secs(3), "the whole probe must fit the 2s cap, took {elapsed:?}");
        assert!(
            stuck.accepts.load(Ordering::SeqCst) >= 1,
            "the probe must actually have reached the backend"
        );
    }

    // -------------------------------------------------------------------- KV scrape

    #[tokio::test]
    async fn kv_cache_info_scrapes_once_and_caches_absence() {
        // Given: a llama.cpp backend whose /metrics is a clean 200 with no vLLM gauge,
        // scripted with EXACTLY one response (a second scrape would panic on the
        // drained queue).
        let (port, state) = scripted_backend(routes(vec![(
            "/metrics",
            vec![scripted_response(
                "200 OK",
                &[("content-type", "text/plain; version=0.0.4")],
                LLAMA_METRICS_BODY.as_bytes(),
            )],
        )]))
        .await;
        let node = node_for(&url_for(port), None);

        // When: the endpoint is asked for twice.
        let first = kv_cache_info(&node).await;
        let accepts_after_first = state.accepts();
        let second = kv_cache_info(&node).await;

        // Then: absence is a KNOWN answer, cached once per process — this is what stops
        // a llama.cpp backend being scraped on every single prompt.
        assert_eq!(first, None, "no gauge means no KV info");
        assert_eq!(second, None);
        assert_eq!(accepts_after_first, 1, "the first call is one scrape");
        println!(
            "absence cache: accepts after call 1 = {accepts_after_first}, after call 2 = {} (delta {})",
            state.accepts(),
            state.accepts() as isize - accepts_after_first as isize
        );
        assert_eq!(state.accepts(), accepts_after_first, "Known(None) must not re-scrape");
        assert_eq!(state.requests(), vec![("GET".to_string(), "/metrics".to_string())]);
        assert_eq!(state.remaining(), 0);
    }

    #[tokio::test]
    async fn kv_cache_info_retries_after_transport_failure() {
        // Given: a backend whose first /metrics connection dies mid-flight (the peer
        // closes without a response — the same transport-error class as ECONNREFUSED;
        // a genuine ECONNREFUSED would mean releasing the port and racing the parallel
        // suite for it).
        let mut fixture = spawn_compat_listener(vec![Reply::CloseSilent, Reply::Frame(vllm_metrics_frame())], false).await;
        let node = node_for(&fixture.base_url, None);

        // When: the production entry point runs against it.
        let failed = kv_cache_info(&node).await;
        assert_eq!(failed, None, "a transport error yields nothing");
        assert_eq!(fixture.accepts.load(Ordering::SeqCst), 1);

        // Then: the SAME entry point, immediately, still says nothing AND does not
        // re-scrape. This is the assertion that proves the 30s retry backoff is real.
        let during_backoff = kv_cache_info(&node).await;
        assert_eq!(during_backoff, None, "the backoff window must not retry");
        println!(
            "retry backoff: accepts after the failed attempt = 1, after the in-window call = {} (delta {})",
            fixture.accepts.load(Ordering::SeqCst),
            fixture.accepts.load(Ordering::SeqCst) as isize - 1
        );
        assert_eq!(
            fixture.accepts.load(Ordering::SeqCst),
            1,
            "the Failed entry must short-circuit BEFORE the second scrape"
        );

        // Then: once the backoff elapses (the test seam passes Duration::ZERO; the clock
        // itself is not faked — Instant cannot run backwards) the failure is NOT poisoned
        // into a permanent miss.
        let recovered = kv_cache_info_for_test(&node, Duration::ZERO).await;
        let recovered = recovered.expect("a transient transport error must not become a permanent miss");
        assert_eq!(
            recovered.labels().get("num_gpu_blocks").map(String::as_str),
            Some("5080"),
            "the retry must actually read the gauge"
        );
        assert_eq!(fixture.accepts.load(Ordering::SeqCst), 2, "one scrape per allowed attempt");
        let (_, authed) = next_seen(&mut fixture).await;
        assert!(!authed, "a node with no api_key sends no Authorization");
    }

    #[tokio::test]
    async fn kv_cache_info_presents_the_node_api_key() {
        // Given: a backend that 401s anything without `Authorization: Bearer secret`.
        let mut fixture = spawn_compat_listener(vec![Reply::Frame(vllm_metrics_frame())], true).await;
        let node = node_for(&fixture.base_url, Some("secret"));

        // When: the scrape runs.
        let info = kv_cache_info(&node).await.expect("an auth'd /metrics must not be a silent miss");

        // Then: the gauge came back, and the request that fetched it carried the token
        // (E-M3: a dropped api_key is a "successful connection, 401 body" miss).
        assert_eq!(info.labels().get("num_gpu_blocks").map(String::as_str), Some("5080"));
        let (seen, authed) = next_seen(&mut fixture).await;
        assert_eq!(seen, "GET /metrics");
        assert!(authed, "the /metrics request must carry the node's bearer token (E-M3)");
    }
}
