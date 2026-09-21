//! Request/response handler for the proxy

use axum::{
    body::{to_bytes, Body},
    http::{header, HeaderMap, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use std::io::Read;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::OwnedSemaphorePermit;

use super::server::ProxyState;
use super::streaming::handle_streaming_response;
use super::{
    synthesize_anthropic_openai_format_response, synthesize_anthropic_streaming_response, synthesize_streaming_response,
};
use crate::api::{AnthropicMessage, ChatCompletionRequest};
use crate::augment::{extract_user_content_from_json, inject_augmentation};
use crate::backends::BackendNode;
use crate::proxy::fetch_context_total;
use crate::stats::{format_request_log, RequestMetrics};

/// JSON error envelope for proxy-generated failures: `{"error":{"type":kind,"message":msg}}`.
fn err_envelope(kind: &str, msg: impl Into<String>) -> serde_json::Value {
    serde_json::json!({
        "error": {
            "type": kind,
            "message": msg.into(),
        }
    })
}

/// True when a body-read failure is axum's over-cap error. `to_bytes` surfaces
/// `http_body_util::LengthLimitError` boxed in a transparent `axum_core::Error`,
/// whose Display is exactly this lowercase string (http-body-util 0.1.3
/// limited.rs). Matching Display avoids a direct http-body-util dependency.
fn is_length_limit_error(e: &impl std::fmt::Display) -> bool {
    e.to_string() == "length limit exceeded"
}

/// Map a `to_bytes` failure: over the hard-coded cap -> 413 naming `cap_label`,
/// any other read/parse failure -> 400 invalid_request_json with the cause.
fn body_read_error(e: impl std::fmt::Display, cap_label: &str) -> Response {
    if is_length_limit_error(&e) {
        (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(err_envelope(
                "request_body_too_large",
                format!("Request body exceeds the {cap_label} limit"),
            )),
        )
            .into_response()
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(err_envelope(
                "invalid_request_json",
                format!("Failed to read request body: {e}"),
            )),
        )
            .into_response()
    }
}

/// Response when server is at capacity
fn at_capacity_response(max: usize) -> Response {
    tracing::warn!(max = max, "Server at capacity, rejecting request");
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, "5")],
        Json(serde_json::json!({
            "error": {
                "type": "too_many_requests",
                "message": format!(
                    "Server at capacity ({} concurrent requests). Increase max_concurrent_requests in config.yaml or scale with additional backend nodes",
                    max
                ),
                "retry_after": 5
            }
        })),
    )
        .into_response()
}

/// Dump utilities for request/response debugging
mod dump {
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::fs;
    use tokio::io::AsyncWriteExt;

    /// Get file extension based on Content-Type header
    pub fn get_extension(content_type: Option<&str>) -> &'static str {
        match content_type {
            Some(ct) => {
                let ct_lower = ct.to_lowercase();
                if ct_lower.contains("application/json") {
                    "json"
                } else if ct_lower.contains("application/xml") || ct_lower.contains("text/xml") {
                    "xml"
                } else {
                    "txt"
                }
            }
            None => "txt",
        }
    }

    /// Dump request/response pair to disk.
    ///
    /// `response_body` / `response_content_type` describe the ORIGINAL backend wire
    /// response as received - pre-decompression, pre-fix [C-M10]. When the proxy
    /// transformed the body, `decoded_variant` carries the transformed bytes and is
    /// written next to the original as `res.<ext>.decoded`, raw - the variant exists
    /// to be diffed byte-for-byte against what the client actually received.
    pub async fn dump_request_response(
        dump_path: &Arc<PathBuf>,
        request_method: &str,
        request_uri: &str,
        request_body: &[u8],
        request_content_type: Option<&str>,
        response_status: u16,
        response_body: &[u8],
        response_content_type: Option<&str>,
        decoded_variant: Option<&[u8]>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Create unique request directory
        let request_id = uuid::Uuid::new_v4().to_string();
        let request_dir = dump_path.join(&request_id);
        fs::create_dir_all(&request_dir).await?;

        // Determine file extensions
        let req_ext = get_extension(request_content_type);
        let res_ext = get_extension(response_content_type);

        // Write request file
        let req_path = request_dir.join(format!("req.{}", req_ext));
        let mut req_file = fs::File::create(&req_path).await?;
        // Write as JSON if possible, otherwise raw bytes
        if let Ok(json_val) = serde_json::from_slice::<serde_json::Value>(request_body) {
            let json_str = serde_json::to_string_pretty(&json_val)?;
            req_file.write_all(json_str.as_bytes()).await?;
            req_file.write_all(b"\n").await?;
        } else {
            req_file.write_all(request_body).await?;
        }
        req_file.flush().await?;

        // Write response file (ORIGINAL wire bytes; pretty-printed only if the
        // original itself is JSON, so a compressed original lands byte-equal)
        let res_path = request_dir.join(format!("res.{}", res_ext));
        let mut res_file = fs::File::create(&res_path).await?;
        if let Ok(json_val) = serde_json::from_slice::<serde_json::Value>(response_body) {
            let json_str = serde_json::to_string_pretty(&json_val)?;
            res_file.write_all(json_str.as_bytes()).await?;
            res_file.write_all(b"\n").await?;
        } else {
            res_file.write_all(response_body).await?;
        }
        res_file.flush().await?;

        if let Some(variant) = decoded_variant {
            let variant_path = request_dir.join(format!("res.{}.decoded", res_ext));
            let mut variant_file = fs::File::create(&variant_path).await?;
            variant_file.write_all(variant).await?;
            variant_file.flush().await?;
        }

        // Write metadata
        let meta_path = request_dir.join("meta.txt");
        let mut meta_file = fs::File::create(&meta_path).await?;
        let variant_note = match decoded_variant {
            Some(v) => format!("  Decoded Variant: res.{}.decoded ({} bytes)\n", res_ext, v.len()),
            None => String::new(),
        };
        let meta_str = format!(
            "Request:\n  Method: {}\n  URI: {}\n  Content-Type: {:?}\n  Body Size: {} bytes\n\nResponse:\n  Status: {}\n  Content-Type: {:?}\n  Body Size: {} bytes\n{}",
            request_method, request_uri, request_content_type, request_body.len(), response_status, response_content_type, response_body.len(), variant_note
        );
        meta_file.write_all(meta_str.as_bytes()).await?;
        meta_file.flush().await?;

        tracing::info!(request_id = %request_id, dump_path = %request_dir.display(), "Dumped request/response pair");

        Ok(())
    }
}

/// Create a preview of JSON with nested objects/arrays replaced by "[object]"
fn json_preview(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut preview_map = serde_json::Map::new();
            for (key, val) in map {
                match val {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        preview_map.insert(key.clone(), serde_json::Value::String("[object]".to_string()));
                    }
                    _ => {
                        preview_map.insert(key.clone(), val.clone());
                    }
                }
            }
            serde_json::to_string_pretty(&serde_json::Value::Object(preview_map))
                .unwrap_or_else(|_| "[failed to serialize]".to_string())
        }
        _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| "[failed to serialize]".to_string()),
    }
}

/// Outcome of `decompress_body`: whether the forwarded body still carries the
/// backend's Content-Encoding transform. Only `Decoded` bodies may have the
/// header stripped; anything handed on unchanged must keep its header, or the
/// client is told to expect a transform that has already been applied [C-H2].
#[derive(Debug)]
enum Decompressed {
    Passthrough(Vec<u8>),
    Decoded(Vec<u8>),
}

/// A compressed backend body may legitimately expand by orders of magnitude;
/// this is the ceiling the proxy is willing to materialize for one response.
/// 64 MiB is far above any real llama.cpp completion and far below what a
/// zip-bomb-class payload demands, so crossing it is itself the refusal signal.
const MAX_DECOMPRESSED_BYTES: usize = 64 * 1024 * 1024;

/// Why a compressed body could not be turned into bytes. The causes answer the
/// client differently (502 vs 413), so they stay separate variants rather than
/// a formatted string callers would have to pattern-match on text.
#[derive(Debug)]
enum DecompressError {
    /// The bytes are not (or no longer) a valid stream in the claimed encoding.
    Decode(String),
    /// The stream decoded but expanded past `MAX_DECOMPRESSED_BYTES`.
    PayloadTooLarge { limit_bytes: usize },
}

impl std::fmt::Display for DecompressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecompressError::Decode(message) => f.write_str(message),
            DecompressError::PayloadTooLarge { limit_bytes } => {
                write!(f, "decompressed body exceeds the {limit_bytes}-byte cap")
            }
        }
    }
}

/// Decompress response body based on Content-Encoding header. The codec chain is
/// pure CPU, so it runs on the blocking pool: a cap-sized brotli decode costs on
/// the order of a few hundred milliseconds, and inline on an async worker that
/// stalls every other connection pinned to the same thread [C-M5].
async fn decompress_body(body_bytes: Vec<u8>, content_encoding: Option<String>) -> Result<Decompressed, DecompressError> {
    tokio::task::spawn_blocking(move || decode_body_bytes(&body_bytes, content_encoding.as_deref()))
        .await
        .map_err(|e| DecompressError::Decode(format!("decompression worker task failed: {e}")))?
}

/// The synchronous codec core; unit tests pin this directly so the chain contract
/// survives the async wrapper. Every codec reads through a `take(cap + 1)` seam:
/// an over-cap stream stops at the cap plus one byte — enough to prove the breach
/// without ever materializing the full expansion — and codecs that error on the
/// resulting truncation are caught by the partial-buffer length on the error path.
fn decode_body_bytes(body_bytes: &[u8], content_encoding: Option<&str>) -> Result<Decompressed, DecompressError> {
    let encoding = match content_encoding {
        Some(enc) => enc,
        None => return Ok(Decompressed::Passthrough(body_bytes.to_vec())), // No compression
    };
    // +1 so a body exactly at the cap still decodes; only a true breach trips.
    let read_limit = MAX_DECOMPRESSED_BYTES as u64 + 1;

    match encoding.to_lowercase().as_str() {
        "gzip" => {
            use flate2::read::GzDecoder;
            let mut decompressed = Vec::new();
            match GzDecoder::new(body_bytes).take(read_limit).read_to_end(&mut decompressed) {
                Ok(_) => decoded_within_cap(body_bytes.len(), decompressed, "gzip"),
                Err(_) if decompressed.len() > MAX_DECOMPRESSED_BYTES => Err(cap_breached()),
                Err(e) => Err(DecompressError::Decode(format!("gzip decompression failed: {e}"))),
            }
        }
        "deflate" => {
            // RFC 9110 says the `deflate` token names zlib-format data; senders that
            // mean raw deflate still send `deflate`. Try zlib first, retry raw. A raw
            // stream that happened to pass the zlib decoder is already the bytes the
            // client expects, so the attempt order only shapes the error path.
            use flate2::read::{DeflateDecoder, ZlibDecoder};
            let mut zlib_out = Vec::new();
            match ZlibDecoder::new(body_bytes).take(read_limit).read_to_end(&mut zlib_out) {
                Ok(_) => decoded_within_cap(body_bytes.len(), zlib_out, "deflate (zlib format)"),
                Err(_) if zlib_out.len() > MAX_DECOMPRESSED_BYTES => Err(cap_breached()),
                Err(zlib_err) => {
                    let mut raw_out = Vec::new();
                    match DeflateDecoder::new(body_bytes).take(read_limit).read_to_end(&mut raw_out) {
                        Ok(_) => {
                            tracing::debug!(
                                original_size = body_bytes.len(),
                                zlib_error = %zlib_err,
                                "deflate body rejected by the zlib decoder, decoded as raw deflate"
                            );
                            decoded_within_cap(body_bytes.len(), raw_out, "deflate (raw)")
                        }
                        Err(_) if raw_out.len() > MAX_DECOMPRESSED_BYTES => Err(cap_breached()),
                        Err(raw_err) => Err(DecompressError::Decode(format!(
                            "deflate decompression failed: zlib: {zlib_err}; raw-deflate: {raw_err}"
                        ))),
                    }
                }
            }
        }
        "br" => {
            // Decompressor is the reader-shaped brotli entry point; the one-shot
            // BrotliDecompress writes straight into a Vec, leaving nowhere to mount
            // the take() seam the cap needs.
            let mut decompressed = Vec::new();
            match brotli::Decompressor::new(std::io::Cursor::new(body_bytes), 4096)
                .take(read_limit)
                .read_to_end(&mut decompressed)
            {
                Ok(_) => decoded_within_cap(body_bytes.len(), decompressed, "brotli"),
                Err(_) if decompressed.len() > MAX_DECOMPRESSED_BYTES => Err(cap_breached()),
                Err(e) => Err(DecompressError::Decode(format!("brotli decompression failed: {e}"))),
            }
        }
        "zstd" => {
            // Same reason as brotli: zstd::decode_all has no reader seam, the
            // streaming Decoder does — and it still rejects bogus frame headers at
            // construction, which is where the junk-body 502 path surfaces.
            let mut decompressed = Vec::new();
            match zstd::stream::read::Decoder::new(body_bytes) {
                Ok(decoder) => match decoder.take(read_limit).read_to_end(&mut decompressed) {
                    Ok(_) => decoded_within_cap(body_bytes.len(), decompressed, "zstd"),
                    Err(_) if decompressed.len() > MAX_DECOMPRESSED_BYTES => Err(cap_breached()),
                    Err(e) => Err(DecompressError::Decode(format!("zstd decompression failed: {e}"))),
                },
                Err(e) => Err(DecompressError::Decode(format!("zstd decompression failed: {e}"))),
            }
        }
        other => {
            tracing::warn!(encoding = other, "Unsupported Content-Encoding, returning original body");
            Ok(Decompressed::Passthrough(body_bytes.to_vec()))
        }
    }
}

/// Shared cap gate for every successful decode: a stream that stops exactly at the
/// `take` limit yields cap+1 bytes with no codec error, so the length check — not
/// the codec — owns the refusal. Passthrough bodies are NOT gated: nothing is
/// decoded there, and the request path already caps what the proxy will buffer.
fn decoded_within_cap(original_size: usize, decoded: Vec<u8>, codec: &str) -> Result<Decompressed, DecompressError> {
    if decoded.len() > MAX_DECOMPRESSED_BYTES {
        return Err(cap_breached());
    }
    tracing::debug!(
        original_size,
        decompressed_size = decoded.len(),
        codec,
        "Decompressed response body"
    );
    Ok(Decompressed::Decoded(decoded))
}

fn cap_breached() -> DecompressError {
    DecompressError::PayloadTooLarge {
        limit_bytes: MAX_DECOMPRESSED_BYTES,
    }
}

/// One error response for both decompression call sites: an over-cap expansion is a
/// deterministic property of the backend's payload (413, mirroring the request-side
/// body cap), corrupt bytes are a backend fault (502) — both in the standard
/// error envelope.
fn decompress_error_response(error: DecompressError, context: &str) -> Response {
    match error {
        DecompressError::PayloadTooLarge { limit_bytes } => {
            tracing::error!(limit_bytes, context, "Backend response exceeds the decompression cap");
            (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(err_envelope(
                    "backend_payload_too_large",
                    format!(
                        "Backend response expands beyond the {} MiB decompression cap",
                        limit_bytes / (1024 * 1024)
                    ),
                )),
            )
                .into_response()
        }
        DecompressError::Decode(message) => {
            tracing::error!(error = %message, context, "Failed to decompress backend response");
            (
                StatusCode::BAD_GATEWAY,
                Json(err_envelope(
                    "backend_decompress_error",
                    format!("Failed to decompress response: {message}"),
                )),
            )
                .into_response()
        }
    }
}

/// Hop-by-hop headers (RFC 9110 §7.6.1): scoped to one transport connection and
/// MUST NOT be forwarded by a proxy. This predicate is the only definition — the
/// extracted copy helper below is the only place that consults it, so the skip set
/// cannot drift between response-forwarding sites [C-H3].
fn is_hop_by_hop_header(name: &header::HeaderName) -> bool {
    name == header::CONNECTION
        || name == header::PROXY_AUTHENTICATE
        || name == header::PROXY_AUTHORIZATION
        || name == header::TE
        || name == header::TRAILER
        || name == header::UPGRADE
}

/// The single response-forwarding implementation: backend status + headers copied onto
/// the outgoing response with the (possibly fixed or decoded) body supplied by the
/// caller. Dropped: Content-Length and Transfer-Encoding (Axum recomputes them for the
/// body actually sent), the hop-by-hop set, and Content-Encoding exactly when the proxy
/// decoded the body [C-H2]. Both response-copy sites (buffered completion path and
/// monitoring pass-through) run through here; each used to hand-roll its own loop and
/// the skip sets had already begun to diverge.
fn forward_response(status: StatusCode, headers: &HeaderMap, body: Vec<u8>, body_was_decoded: bool) -> Response {
    let mut response = Response::builder().status(status);
    for (name, value) in headers {
        if name == header::CONTENT_LENGTH || name == header::TRANSFER_ENCODING || is_hop_by_hop_header(name) {
            continue;
        }
        if body_was_decoded && name == header::CONTENT_ENCODING {
            continue;
        }
        response = response.header(name.clone(), value.clone());
    }
    // Builder error is unreachable: header names/values come from an already-parsed
    // HeaderMap (validation happened at parse) and Body::from accepts any Vec [C-L7].
    response
        .body(Body::from(body))
        .expect("status + parsed-header values + Vec body are always a valid Response")
        .into_response()
}

/// [C-L8] Termination of a /v1/messages stream whose translation failed. The client
/// asked for Anthropic SSE, so the proxy still answers with a valid SSE stream: an
/// `error` event (which the Anthropic SDK raises on), then the `[DONE]` terminal data
/// frame, then the stream ends. The alternative - falling through to the JSON return -
/// handed the client an HTTP 200 whose body was the raw backend object, which for the
/// OpenAI-format default is a shape Claude Code cannot parse at all. Exact bytes are
/// pinned by `messages_stream_synthesis_failure_serves_sse_error_frame_not_openai_body`
/// (test-side literal, deliberately not a reference to this function).
fn anthropic_sse_error_response() -> Response {
    let frame = concat!(
        "event: error\n",
        "data: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",",
        "\"message\":\"The proxy failed to convert the backend response into ",
        "an Anthropic streaming response\"}}\n\n",
        "data: [DONE]\n\n",
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(frame))
        // Static, built once per failure: constant status, constant literal header,
        // constant body - the builder cannot reject this combination [C-L7].
        .expect("static SSE error frame is a valid response by construction")
        .into_response()
}

/// RAII decrement for the `concurrent_requests` gauge, incremented at the top of
/// `handle`. Semantics [C-L14]: the gauge counts requests whose handler is executing -
/// from body-read until `handle()` RETURNS, which for a stream means headers-ready, not
/// body-done. A streamed generation keeps running after this guard drops; the capacity
/// permit and the balancer's busy claim deliberately ride the response body for that
/// window (big-fix 18), so gauge > 0 does NOT mean every permit is held. `/proxy/metrics`
/// renders the gauge minus its own in-flight scrape, so an idle proxy reports 0.
struct ConcurrentGuard(Arc<std::sync::atomic::AtomicUsize>);
impl Drop for ConcurrentGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Proxy request handler
pub struct ProxyHandler {
    state: ProxyState,
}

impl ProxyHandler {
    pub fn new(state: ProxyState) -> Self {
        Self { state }
    }

    // REMOVED: should_stream() method
    // We now ALWAYS force non-streaming backend requests and synthesize streaming responses
    // when clients request them. This simplifies fix application significantly.

    /// Check if Content-Type indicates a JSON document.
    ///
    /// The media type is everything before the first ';' - parameters (charset, q=, even a
    /// JSON-looking one) never change classification [C-L9]. A type is JSON when it is
    /// `application/json` or carries the RFC 6839 `+json` structured suffix (this covers
    /// the vendor types the old `vnd.` check existed for: vnd.api+json, geo+json,
    /// json-patch+json). Substring matching was the bug: application/json-seq and
    /// application/jsonpath CONTAIN "application/json" and are not JSON documents.
    fn is_json_content_type(content_type: &str) -> bool {
        let media_type = content_type
            .split_once(';')
            .map_or(content_type, |(media, _params)| media)
            .trim()
            .to_ascii_lowercase();
        media_type == "application/json" || media_type.ends_with("+json")
    }

    /// Inject augmentation into Anthropic messages
    fn inject_into_anthropic(request: &mut crate::api::AnthropicMessageRequest, request_prompt: &str, augmentation: &str) {
        if augmentation.is_empty() {
            return;
        }

        let suffix = format!("\n\n{}\n\n{}", request_prompt, augmentation);

        // Find the last user message
        let mut last_user_index = None;
        for (i, msg) in request.messages.iter().enumerate() {
            if msg.role == "user" {
                last_user_index = Some(i);
            }
        }

        if let Some(idx) = last_user_index {
            // Add augmentation as a new text block at the end
            request.messages[idx]
                .content
                .push(crate::api::AnthropicContentBlock::Text { text: suffix });

            tracing::debug!(
                injected_into = idx,
                augmentation_length = augmentation.len(),
                "Injected augmentation into last Anthropic user message"
            );
        } else if !request.messages.is_empty() {
            // Fallback: prepend to first message
            request.messages[0]
                .content
                .insert(0, crate::api::AnthropicContentBlock::Text { text: suffix });

            tracing::debug!(
                injected_into = 0,
                augmentation_length = augmentation.len(),
                "Injected augmentation into first Anthropic message (no user message found)"
            );
        }
    }

    /// Cooldown window for the group a request was routed through: per-group
    /// `failure_cooldown_secs`, 30s when the group is unknown (single-backend mode,
    /// or a group renamed out from under a live guard).
    fn failure_cooldown(&self, group_name: Option<&str>) -> Duration {
        let secs = group_name
            .and_then(|name| self.state.config.backends.as_ref()?.get(name))
            .map_or(30, |group| group.failure_cooldown_secs);
        Duration::from_secs(secs)
    }

    /// Take the node out of load-balancer selection after a backend failure
    /// (big-fix E-M1). Wraps around the existing error envelopes; touches nothing else.
    fn mark_backend_failed(&self, node: &BackendNode, group_name: Option<&str>, reason: &str) {
        let cooldown = self.failure_cooldown(group_name);
        tracing::warn!(
            backend_url = %node.base_url(),
            reason,
            cooldown_secs = cooldown.as_secs(),
            "Backend failure marks the node out of selection"
        );
        node.mark_failed(cooldown);
    }

    /// Record one post-forward backend status on the node's runtime health:
    /// backend-originated 429/5xx cool the node down, 2xx proves it alive.
    /// The proxy's own at-capacity 429 never reaches here (it precedes selection).
    fn observe_backend_outcome(&self, node: &BackendNode, group_name: Option<&str>, status: StatusCode) {
        if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            self.mark_backend_failed(node, group_name, "backend_status");
        } else if status.is_success() {
            node.mark_healthy();
        }
    }

    /// Handle an incoming request
    pub async fn handle(&self, req: Request<Body>) -> Response {
        let start = Instant::now();
        self.state.concurrent_requests.fetch_add(1, Ordering::Relaxed);
        let _guard = ConcurrentGuard(Arc::clone(&self.state.concurrent_requests));

        let method = req.method().clone();
        let uri = req.uri().clone();
        let path = uri.path();
        let query = uri.query();

        tracing::debug!(method = %method, path = %path, query = ?query, "Processing request");

        // Save headers before consuming the request
        let headers = req.headers().clone();

        // Read request body FIRST (needed for model-based routing)
        let body_bytes = match to_bytes(req.into_body(), 1024 * 1024 * 100).await {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!(error = %e, "Failed to read request body");
                return body_read_error(e, "100 MiB");
            }
        };

        // Parse request for model extraction and stats (if JSON). A malformed body is a
        // legitimate class (GETs, opaque payloads), but when bytes were present and
        // unparseable the downstream consequences (no model routing signal, no stats,
        // augmentation silently skipped) must be attributable [C-L9].
        let request_json: Option<serde_json::Value> = match serde_json::from_slice(&body_bytes) {
            Ok(json) => Some(json),
            Err(e) => {
                if !body_bytes.is_empty() {
                    tracing::debug!(
                        error = %e,
                        body_size = body_bytes.len(),
                        "Request body is not valid JSON: model routing, stats and augmentation run without a parsed body"
                    );
                }
                None
            }
        };

        // Extract model for routing BEFORE selecting backend
        let requested_model = request_json.as_ref().and_then(|j| j.get("model")).and_then(|m| m.as_str());

        tracing::debug!(requested_model = ?requested_model, "Routing request");

        // NOW select backend based on model
        let backend = match self.state.load_balancer.select(requested_model) {
            Ok(guard) => guard,
            Err(crate::config::NoMatchingBackend { requested_model }) => {
                tracing::warn!(
                    requested_model = ?requested_model,
                    "No backend configured for model"
                );
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(err_envelope(
                        "no_backend",
                        format!("No backend for model: {:?}", requested_model),
                    )),
                )
                    .into_response();
            }
        };

        // [C-H4] A node mounted behind `strip_path_prefix` serves its API one level
        // deeper than the client's URL: the client's "/completions/v1/messages" is the
        // node's native "/v1/messages". Every routing decision below takes the
        // backend-native view. The API-format detection ALSO checks the original path,
        // because the client may speak /v1/messages while the node's native path
        // differs (e.g. prefix "/v1" turns "/v1/messages" into "/messages").
        // Forwarding keeps its own effective_path application unchanged, so the strip
        // happens exactly once.
        let routed = backend.node.effective_path(path);
        let is_completion_route = Self::completion_shaped_path(routed);
        let is_anthropic_api = path.starts_with("/v1/messages") || routed.starts_with("/v1/messages");
        tracing::debug!(
            is_anthropic_api = is_anthropic_api,
            routed = %routed,
            is_completion_route = is_completion_route,
            "Detected API format"
        );

        // Route specific endpoints to simple pass-through
        match (&method, routed) {
            // llama.cpp monitoring/status endpoints (simple pass-through)
            (&Method::GET, "/props")
            | (&Method::GET, "/slots")
            | (&Method::GET, "/v1/health")
            | (&Method::GET, "/v1/models")
            | (&Method::GET, "/metrics") => {
                // Reconstruct request for passthrough
                let req = Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::from(body_bytes))
                    // method/uri were accepted by the router from this same parsed
                    // request, and Body::from accepts Bytes unconditionally [C-L7].
                    .expect("method+uri of the request being handled are valid by construction");
                // Add headers back
                let mut req = req;
                for (name, value) in headers.iter() {
                    req.headers_mut().insert(name.clone(), value.clone());
                }
                return self
                    .proxy_passthrough(req, &backend.node, backend.group_name.as_deref())
                    .await;
            }

            // Proxy-local metrics endpoint (distinct from backend's /metrics pass-through)
            (&Method::GET, "/proxy/metrics") => {
                let fallback_hits = self.state.backend_streaming_fallback_hits.load(Ordering::Relaxed);
                let rejected = self.state.rejected_requests.load(Ordering::Relaxed);
                let routed_stream = self.state.openai_stream_passthrough_total.load(Ordering::Relaxed);
                let backend_no_stream = self.state.backend_nonsse_when_streamed_for.load(Ordering::Relaxed);
                let anthropic_buffered = self.state.anthropic_buffered_responses_total.load(Ordering::Relaxed);

                // This scrape is itself counted in concurrent_requests (incremented at the
                // top of handle()), so discount it - otherwise an idle proxy reports 1.
                let concurrent = self.state.concurrent_requests.load(Ordering::Relaxed).saturating_sub(1);

                use crate::proxy::streaming as stream_stats;
                let l = |c: &std::sync::atomic::AtomicU64| c.load(Ordering::Relaxed);
                let body = format!(
                    "# HELP llama_proxy_backend_streaming_fallback_total Times the backend streamed although the proxy requested stream:false. The expected path in passthrough mode is NOT counted here.\n\
                     # TYPE llama_proxy_backend_streaming_fallback_total counter\n\
                     llama_proxy_backend_streaming_fallback_total {}\n\
                     # HELP llama_proxy_concurrent_requests Current in-flight requests\n\
                     # TYPE llama_proxy_concurrent_requests gauge\n\
                     llama_proxy_concurrent_requests {}\n\
                     # HELP llama_proxy_rejected_requests_total Requests rejected at capacity\n\
                     # TYPE llama_proxy_rejected_requests_total counter\n\
                     llama_proxy_rejected_requests_total {}\n\
                     # HELP llama_proxy_openai_stream_passthrough_total OpenAI requests routed onward with stream:true intact. Counted at the routing decision, so it moves even with stats.enabled: false - use this as the passthrough mode probe.\n\
                     # TYPE llama_proxy_openai_stream_passthrough_total counter\n\
                     llama_proxy_openai_stream_passthrough_total {}\n\
                     # HELP llama_proxy_backend_nonsse_when_streamed_total Passthrough asked for stream:true and the backend answered 2xx with JSON instead of text/event-stream. The backend does not stream; the proxy is not at fault.\n\
                     # TYPE llama_proxy_backend_nonsse_when_streamed_total counter\n\
                     llama_proxy_backend_nonsse_when_streamed_total {}\n\
                     # HELP llama_proxy_anthropic_buffered_responses_total /v1/messages responses served buffered with synthesized SSE under streaming: passthrough. The log notice fires once per process; this keeps counting.\n\
                     # TYPE llama_proxy_anthropic_buffered_responses_total counter\n\
                     llama_proxy_anthropic_buffered_responses_total {}\n\
                     # HELP llama_proxy_passthrough_streams_total Pass-through SSE responses framed by the proxy; denominator for the ratios below. Requires accumulation (stats.enabled or dump), unlike openai_stream_passthrough_total. Excludes compressed responses.\n\
                     # TYPE llama_proxy_passthrough_streams_total counter\n\
                     llama_proxy_passthrough_streams_total {}\n\
                     # HELP llama_proxy_passthrough_sse_events_total SSE events framed on those streams. Counted only while accumulation is on (stats.enabled or dump).\n\
                     # TYPE llama_proxy_passthrough_sse_events_total counter\n\
                     llama_proxy_passthrough_sse_events_total {}\n\
                     # HELP llama_proxy_passthrough_sse_unparsed_events_total Framed SSE events whose data payload was not valid JSON, forwarded verbatim and unanalyzed. Denominator: passthrough_sse_events_total.\n\
                     # TYPE llama_proxy_passthrough_sse_unparsed_events_total counter\n\
                     llama_proxy_passthrough_sse_unparsed_events_total {}\n\
                     # HELP llama_proxy_passthrough_stream_truncated_total Streams where the backend ended without [DONE]/message_stop. Denominator: passthrough_streams_total.\n\
                     # TYPE llama_proxy_passthrough_stream_truncated_total counter\n\
                     llama_proxy_passthrough_stream_truncated_total {}\n\
                     # HELP llama_proxy_passthrough_stream_stalled_total Streams the stats observer gave up on after 90s without a chunk. The client transfer is not cut by this. Denominator: passthrough_streams_total.\n\
                     # TYPE llama_proxy_passthrough_stream_stalled_total counter\n\
                     llama_proxy_passthrough_stream_stalled_total {}\n\
                     # HELP llama_proxy_passthrough_stream_client_gone_total Streams where the client disconnected before completion. Normal, not a defect; counted so truncation can be read against real traffic.\n\
                     # TYPE llama_proxy_passthrough_stream_client_gone_total counter\n\
                     llama_proxy_passthrough_stream_client_gone_total {}\n\
                     # HELP llama_proxy_passthrough_fix_unrepaired_total Fix detections reported and NOT repaired. Detections, not responses - one response can add several. Denominator: passthrough_streams_total.\n\
                     # TYPE llama_proxy_passthrough_fix_unrepaired_total counter\n\
                     llama_proxy_passthrough_fix_unrepaired_total {}\n\
                     # HELP llama_proxy_passthrough_compressed_responses_total Responses bypassed entirely because Content-Encoding was not identity. Not counted in any other passthrough_* metric.\n\
                     # TYPE llama_proxy_passthrough_compressed_responses_total counter\n\
                     llama_proxy_passthrough_compressed_responses_total {}\n\
                     # HELP llama_proxy_metrics_export_skipped_total Metric samples excluded from every exporter because the backend reported no token count and no rate: a stream the client abandoned before usage/timings arrived, or a backend error body. They are logged at WARN instead. Spans the buffered and pass-through paths, and is NOT the same as passthrough_stream_client_gone_total - a client-gone stream that did carry usage is exported and is not counted here. Remote token totals under-count backend work by this amount.\n\
                      # TYPE llama_proxy_metrics_export_skipped_total counter\n\
                      llama_proxy_metrics_export_skipped_total {}\n\
                      # HELP llama_proxy_context_cache_stale_skips_total Context-cache refreshes skipped because the write lock was held when a fresh value arrived; the cache keeps its previous value, so context_total can read stale. Not a defect signal on its own - it explains context_percent jumps. Spans the buffered and pass-through paths.\n\
                      # TYPE llama_proxy_context_cache_stale_skips_total counter\n\
                      llama_proxy_context_cache_stale_skips_total {}\n",
                     fallback_hits,
                     concurrent,
                     rejected,
                     routed_stream,
                     backend_no_stream,
                     anthropic_buffered,
                     l(&stream_stats::PASSTHROUGH_STREAMS_TOTAL),
                     l(&stream_stats::SSE_EVENTS_TOTAL),
                     l(&stream_stats::SSE_UNPARSED_EVENTS_TOTAL),
                     l(&stream_stats::STREAM_TRUNCATED_TOTAL),
                     l(&stream_stats::STREAM_STALLED_TOTAL),
                     l(&stream_stats::STREAM_CLIENT_GONE_TOTAL),
                     l(&stream_stats::FIX_UNREPAIRED_TOTAL),
                     l(&stream_stats::STREAM_COMPRESSED_TOTAL),
                     l(&crate::exporters::EXPORTS_SKIPPED_TOTAL),
                     crate::proxy::context::context_cache_stale_skips(),
                 );

                return (StatusCode::OK, [(header::CONTENT_TYPE, "text/plain; version=0.0.4")], body).into_response();
            }

            // All other routes continue with existing logic
            _ => {}
        }

        // Check concurrent request limit for completion routes only (not monitoring endpoints).
        // This must be AFTER the pass-through match so health/status routes are never rejected.
        let max_concurrent = self.state.config.server.max_concurrent_requests;
        let mut permit: Option<OwnedSemaphorePermit> = None;
        if max_concurrent > 0 {
            // A `None` semaphore means limiting was not armed at startup; treat as unlimited.
            if let Some(semaphore) = self.state.concurrent_semaphore.as_ref() {
                match semaphore.clone().try_acquire_owned() {
                    Ok(p) => permit = Some(p),
                    Err(_) => {
                        // No permits available - shed the request rather than queue it.
                        self.state.rejected_requests.fetch_add(1, Ordering::Relaxed);
                        return at_capacity_response(max_concurrent);
                    }
                }
            }
        }
        // On the buffered path the permit drops when this function returns, which is
        // after the backend call completes. On the streaming path it is moved into the
        // response body instead - see the `permit.take()` at the streaming fallback.

        // Remember if client wants streaming (for synthesis later)
        let client_wants_streaming = request_json
            .as_ref()
            .and_then(|j| j.get("stream"))
            .and_then(|s| s.as_bool())
            .unwrap_or(false);

        // Check if augment-backend is enabled and extract user content
        let augmentation = if let Some(ref augment_backend) = self.state.augment_backend {
            if let Some(ref req_json) = request_json {
                // Detect API format
                let is_anthropic_format = is_anthropic_api;

                // Extract user content directly from raw JSON (works for both OpenAI and Anthropic)
                let user_content = extract_user_content_from_json(req_json);
                tracing::debug!(
                    is_anthropic = is_anthropic_format,
                    content_len = user_content.len(),
                    "Extracted user content for augmentation"
                );

                // Only call augment-backend if we have user content
                if !user_content.is_empty() {
                    tracing::info!(content_length = user_content.len(), "Calling augment-backend");
                    match augment_backend.get_augmentation(&user_content).await {
                        Ok(aug) if !aug.is_empty() => {
                            tracing::info!(augmentation_length = aug.len(), "Received augmentation");
                            let request_prompt = augment_backend.load_request_prompt().await.unwrap_or_else(|e| {
                                tracing::warn!(error = %e, "Failed to load request_prompt, using empty string");
                                String::new()
                            });
                            Some((aug, request_prompt, user_content))
                        }
                        Ok(_) => None,
                        Err(e) => {
                            tracing::error!(error = %e, "Augment backend failed, returning error");
                            return (
                                StatusCode::BAD_GATEWAY,
                                Json(err_envelope("augment_backend_error", format!("Augment backend error: {}", e))),
                            )
                                .into_response();
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Inject augmentation into request if we got one
        let enriched_body_bytes = if let Some((ref aug, ref request_prompt, _)) = augmentation {
            if let Some(req_json) = request_json.as_ref() {
                if is_anthropic_api {
                    // Inject into Anthropic format
                    match serde_json::from_value::<crate::api::AnthropicMessageRequest>(req_json.clone()) {
                        Ok(mut anthropic_req) => {
                            Self::inject_into_anthropic(&mut anthropic_req, request_prompt, aug);
                            match serde_json::to_vec(&anthropic_req) {
                                Ok(bytes) => bytes.into(),
                                Err(e) => {
                                    tracing::debug!(
                                        error = %e,
                                        "Augmentation discarded: injected Anthropic body could not be re-serialized, forwarding the original request bytes"
                                    );
                                    body_bytes.clone()
                                }
                            }
                        }
                        Err(e) => {
                            tracing::debug!(
                                error = %e,
                                "Augmentation injection skipped: request body is not an AnthropicMessageRequest, forwarding the original request bytes"
                            );
                            body_bytes.clone()
                        }
                    }
                } else {
                    // Inject into OpenAI format
                    match serde_json::from_value::<ChatCompletionRequest>(req_json.clone()) {
                        Ok(openai_req) => match inject_augmentation(openai_req, request_prompt, aug) {
                            Ok(enriched) => match serde_json::to_vec(&enriched) {
                                Ok(bytes) => bytes.into(),
                                Err(e) => {
                                    tracing::debug!(
                                        error = %e,
                                        "Augmentation discarded: injected body could not be re-serialized, forwarding the original request bytes"
                                    );
                                    body_bytes.clone()
                                }
                            },
                            Err(e) => {
                                tracing::error!(error = %e, "Failed to inject augmentation");
                                body_bytes.clone()
                            }
                        },
                        Err(e) => {
                            tracing::debug!(
                                error = %e,
                                "Augmentation injection skipped: request body is not a ChatCompletionRequest, forwarding the original request bytes"
                            );
                            body_bytes.clone()
                        }
                    }
                }
            } else {
                body_bytes.clone()
            }
        } else {
            body_bytes.clone()
        };

        // Log augmented request text if flag is set and augmentation was applied
        if self.state.log_augmented_request_text {
            if let Some((ref aug, ref request_prompt, ref original_content)) = augmentation {
                let concatenated = format!("{}\n\n{}\n\n{}", original_content, request_prompt, aug);
                tracing::info!("Augmented user message text:\n{}", concatenated);
            }
        }

        // Log request (unless hide_requests is set)
        if !self.state.hide_requests {
            if let Some(ref req_json) = request_json {
                tracing::info!("{}", format_request_log(req_json));
            }
        }

        // Build complete URL with query string as-is (don't parse/re-encode)
        let effective_path = backend.node.effective_path(path);
        let backend_url = if let Some(q) = query {
            format!("{}{}?{}", backend.node.base_url(), effective_path, q)
        } else {
            format!("{}{}", backend.node.base_url(), effective_path)
        };

        tracing::debug!(
            backend_url = %backend_url,
            has_query = query.is_some(),
            "Building backend request"
        );

        // Create request with complete URL (query string included). `method` was already
        // validated by the router's match on this same request, so clone it instead of
        // re-parsing the bytes and panicking on a theoretically-impossible re-reject [C-L7].
        let mut backend_req = backend.node.http_client.request(method.clone(), &backend_url);

        for (name, value) in headers.iter() {
            if !Self::forwards_to_backend(name) {
                continue;
            }

            backend_req = backend_req.header(name, value);
        }

        // Add Authorization header if api_key is configured
        if let Some(ref api_key) = backend.node.api_key {
            backend_req = backend_req.header(header::AUTHORIZATION, format!("Bearer {}", api_key));
        }

        // Passthrough leaves the client's stream:true on the wire, but only on the
        // OpenAI path: a /v1/messages client needs a complete object before the proxy
        // can emit Anthropic SSE, so that path stays buffered in every mode.
        let streaming_mode = self.state.config.streaming;
        let allow_stream = streaming_mode.is_passthrough() && !is_anthropic_api;

        if streaming_mode.is_passthrough() && is_anthropic_api {
            self.state.anthropic_buffered_responses_total.fetch_add(1, Ordering::Relaxed);
            // Startup logs scroll past in `just run`; this is the line that actually
            // reaches someone whose Claude Code is still buffering. Once per process.
            if !self.state.anthropic_buffered_notice_once.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    "Anthropic /v1/messages is served buffered with synthesized SSE even under \
                     streaming: passthrough - there is no OpenAI->Anthropic SSE translator yet. \
                     Passthrough applies to /v1/chat/completions with stream:true only."
                );
            }
        }

        // One call, always on enriched_body_bytes: in every non-injection path
        // enriched_body_bytes is a Bytes refcount-clone of body_bytes - identical content -
        // so the old `!=` branch compared two copies then fed the same bytes to the same
        // function either way [C-L12].
        let (final_body_bytes, sent_stream_true) =
            Self::apply_backend_overrides_bytes(&enriched_body_bytes, &backend.node, allow_stream, path);
        // The router must dispatch on what was actually sent, not on what the client
        // asked for - augmentation or a backend override could change it.
        let expected_streaming = allow_stream && sent_stream_true;
        if expected_streaming {
            self.state.openai_stream_passthrough_total.fetch_add(1, Ordering::Relaxed);
        }

        // Refcount-clone for the request body instead of a deep clone; the dump copy is a
        // deep copy only when a dump path is actually set - the old code paid a full
        // second copy on every request regardless [C-L12].
        let final_body = bytes::Bytes::from(final_body_bytes);
        backend_req = backend_req.body(final_body.clone());
        let backend_request_for_dump = self.state.dump_path.is_some().then(|| final_body.to_vec());

        let backend_response = match backend_req.send().await {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!(error = %e, "Failed to connect to backend");
                self.mark_backend_failed(&backend.node, backend.group_name.as_deref(), "connect_error");
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(err_envelope(
                        "backend_connect_error",
                        format!("Failed to connect to backend: {}", e),
                    )),
                )
                    .into_response();
            }
        };

        // Log backend response status and headers for debugging
        let backend_status = backend_response.status();
        self.observe_backend_outcome(&backend.node, backend.group_name.as_deref(), backend_status);
        tracing::debug!(
            status = %backend_status,
            headers = ?backend_response.headers(),
            "Received response from backend"
        );

        // Check if streaming response
        let is_streaming_response = backend_response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|ct| ct.to_str().ok())
            .map(|ct| ct.contains("text/event-stream"))
            .unwrap_or(false);

        if is_streaming_response {
            // Only a genuine contradiction is an anomaly. In passthrough mode streaming is
            // what we asked for, so counting/warning here would fire on every request and
            // read as a defect report on the mode the user just chose.
            if !expected_streaming {
                let fallback_count = self.state.backend_streaming_fallback_hits.fetch_add(1, Ordering::Relaxed) + 1;

                tracing::warn!(
                    backend_url = %backend.node.base_url(),
                    fallback_count = fallback_count,
                    "Backend returned streaming response despite stream:false request"
                );

                if fallback_count == 10 || fallback_count.is_multiple_of(100) {
                    tracing::error!(
                        fallback_count = fallback_count,
                        "Backend streaming fallback has triggered {} times - check backend configuration",
                        fallback_count
                    );
                }
            } else {
                tracing::debug!(
                    stream_mode = %streaming_mode,
                    "Forwarding backend SSE verbatim"
                );
            }

            let concurrent_snapshot = self.state.concurrent_requests.load(Ordering::Relaxed);
            // Streaming and buffered arms are exclusive and this is the arm's tail -
            // request_json can move here exactly like the else arm below moves it [C-L12].
            handle_streaming_response(
                backend_response,
                self.state.fix_registry.clone(),
                self.state.config.stats.enabled,
                self.state.config.stats.format,
                self.state.exporter_manager.clone(),
                request_json,
                start,
                backend.node.http_client.clone(),
                backend.node.base_url().to_string(),
                backend.group_name.clone(),
                backend.node.strip_path_prefix.clone(),
                self.state.dump_path.clone(),
                Some(method.to_string()),
                Some(uri.to_string()),
                // The bytes actually sent, so a dump cannot show a response (a usage
                // chunk) whose cause is absent from the request beside it.
                backend_request_for_dump,
                concurrent_snapshot,
                streaming_mode,
                // Hand the permit to the stream. This response body is lazy, so returning
                // from handle() only means the headers are ready - the generation itself
                // runs while the body is polled. Dropping the permit here would let the
                // limiter admit new work for a slot that is still busy.
                permit.take(),
                // Same contract for the balancer's busy claim: the node is still
                // generating until the body ends, so the guard rides the stream and
                // releases at body drop (completion or client disconnect).
                backend,
            )
            .await
        } else {
            // An error reply is JSON on every backend, streaming or not, so only a 2xx
            // says anything about whether this backend streams.
            if expected_streaming && backend_response.status().is_success() {
                let n = self.state.backend_nonsse_when_streamed_for.fetch_add(1, Ordering::Relaxed) + 1;
                if n == 1 || n.is_multiple_of(100) {
                    tracing::warn!(
                        count = n,
                        backend_url = %backend.node.base_url(),
                        "streaming: passthrough asked the backend for stream:true but got a JSON body, not \
                         text/event-stream - serving synthesized SSE instead. This backend does not stream, \
                         so passthrough behaves like fake for it."
                    );
                }
            }

            // Handle non-streaming response (expected path)
            self.handle_non_streaming_response(
                backend_response,
                request_json,
                client_wants_streaming,
                is_anthropic_api,
                start,
                &backend.node,
                backend.group_name.as_deref(),
                method.clone(),
                uri.clone(),
            )
            .await
        }
    }

    /// Client headers that must not reach the backend. `Accept-Encoding` is the client's
    /// preference addressed to *us*; forwarding it lets the backend answer with br/zstd,
    /// which reqwest cannot decode here (it decodes gzip only), and the streaming path
    /// would then forward an unsplit body and collect no stats for it.
    fn forwards_to_backend(name: &header::HeaderName) -> bool {
        name != header::HOST
            && name != header::CONTENT_LENGTH
            && name != header::AUTHORIZATION
            && name != header::ACCEPT_ENCODING
    }

    /// Path half of the completion-shape predicate, evaluated on the node-native
    /// (routed) path: the OpenAI chat route, the Anthropic messages route, or llama.cpp's
    /// native `/completions`. Single source of truth shared with the routing DEBUG field
    /// in `handle` (big-fix 13) and the [C-H5] body-injection gate below.
    fn completion_shaped_path(path: &str) -> bool {
        path.starts_with("/completions") || path.contains("/chat/completions") || path.contains("/v1/messages")
    }

    /// [C-H5] Whether the body may receive completion-pipeline rewrites: a completion-
    /// shaped routed path, or any JSON body carrying a non-empty `messages` array (the
    /// one-element array `[{}]` counts as non-empty; `[]` alone is not completion-shaped).
    fn looks_like_completion_request(body: &serde_json::Value, routed_path: &str) -> bool {
        Self::completion_shaped_path(routed_path)
            || body
                .get("messages")
                .and_then(|m| m.as_array())
                .is_some_and(|msgs| !msgs.is_empty())
    }

    /// Rewrites the outgoing backend body for the resolved streaming mode and returns
    /// it together with the `stream` flag the backend will actually see.
    ///
    /// All four rewrites (forced `stream:false`, `stream_options` strip, model override,
    /// temperature override) run ONLY on completion-shaped requests: a body forwarded to
    /// a non-completion passthrough route (/tokenize, /embedding, /descriptions...)
    /// leaves byte-identical, because those fields mean nothing downstream and silently
    /// mutating a body the proxy does not own breaks opaque backends.
    ///
    /// When `allow_stream` is set (passthrough, OpenAI path) the client's `stream:true`
    /// is preserved so the backend's own SSE reaches the client. Otherwise `stream:false`
    /// is forced and `stream_options` stripped, because the proxy answers with
    /// synthesized SSE built from one complete JSON body.
    fn apply_backend_overrides_bytes(body: &[u8], backend: &BackendNode, allow_stream: bool, path: &str) -> (Vec<u8>, bool) {
        let Ok(mut json) = serde_json::from_slice::<serde_json::Value>(body) else {
            // A completion-shaped route carrying an unparseable body gets verbatim
            // forwarding: legitimate for GETs and opaque payloads, but on a completion
            // route it must be attributable - the stream rewrite silently didn't happen [C-L9].
            if !body.is_empty() {
                tracing::debug!(
                    body_size = body.len(),
                    "Request rewrites skipped: completion-shaped request body is not valid JSON, forwarding it verbatim"
                );
            }
            return (body.to_vec(), false);
        };
        if !Self::looks_like_completion_request(&json, backend.effective_path(path)) {
            return (body.to_vec(), false);
        }

        let client_wants_stream = json.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
        let keep_stream = allow_stream && client_wants_stream;

        if keep_stream {
            Self::ensure_usage_on_stream(&mut json);
        } else {
            json["stream"] = serde_json::Value::Bool(false);
            if let Some(obj) = json.as_object_mut() {
                if obj.remove("stream_options").is_some() {
                    tracing::debug!("Stripped stream_options from backend request");
                }
            }
        }

        if let Some(ref model) = backend.model {
            json["model"] = serde_json::Value::String(model.clone());
        }
        if let Some(temp) = backend.temperature {
            json["temperature"] = serde_json::Value::from(temp);
        }

        let rewritten = serde_json::to_vec(&json).unwrap_or_else(|e| {
            // Unreachable in practice (a Value from a parse round-trips); if it ever fires,
            // the client's stream flag rode the ORIGINAL bytes and this names why [C-L9].
            tracing::debug!(error = %e, "Rewritten request body failed to re-serialize; forwarding the original bytes");
            body.to_vec()
        });
        (rewritten, keep_stream)
    }

    /// A stream carries no `usage` block unless the backend is told to emit one, so
    /// without this every token count on a passthrough response is silently zero.
    /// An explicit client `stream_options` always wins, including `include_usage: false`
    /// — losing the counts is then the client's own choice, and logged as such.
    fn ensure_usage_on_stream(json: &mut serde_json::Value) {
        use serde_json::map::Entry;
        let Some(obj) = json.as_object_mut() else { return };

        let injected = match obj.entry("stream_options") {
            Entry::Vacant(e) => {
                e.insert(serde_json::json!({ "include_usage": true }));
                true
            }
            Entry::Occupied(mut e) => match e.get_mut().as_object_mut() {
                Some(opts) if !opts.contains_key("include_usage") => {
                    opts.insert("include_usage".to_string(), serde_json::Value::Bool(true));
                    true
                }
                Some(opts) => {
                    if opts.get("include_usage").and_then(|v| v.as_bool()) == Some(false) {
                        tracing::debug!("client set stream_options.include_usage=false; streaming token counts will be absent");
                    }
                    false
                }
                // A `stream_options` that is not an object is forwarded untouched.
                // Rewriting a value we do not understand would be a guess, and the
                // indexing sugar would silently destroy it.
                None => false,
            },
        };

        if injected {
            tracing::debug!("injected stream_options.include_usage=true; streams carry no usage block without it");
        }
    }

    /// Handle a non-streaming response
    async fn handle_non_streaming_response(
        &self,
        backend_response: reqwest::Response,
        request_json: Option<serde_json::Value>,
        client_wants_streaming: bool,
        is_anthropic_api: bool,
        start: Instant,
        backend: &Arc<BackendNode>,
        group_name: Option<&str>,
        request_method: Method,
        request_uri: axum::http::Uri,
    ) -> Response {
        let status = backend_response.status();
        let headers = backend_response.headers().clone();
        // [C-M10] Content-Type as the backend sent it, captured at header-receive -
        // before decompression or fixes touch the body. The dump must never have to
        // re-derive it from a post-transform state.
        let original_response_content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // Read response body
        let raw_body_bytes = match backend_response.bytes().await {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!(error = %e, "Failed to read backend response");
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(err_envelope(
                        "backend_read_error",
                        format!("Failed to read backend response: {}", e),
                    )),
                )
                    .into_response();
            }
        };

        // Check for Content-Encoding and decompress if needed
        let content_encoding = headers.get(header::CONTENT_ENCODING).and_then(|ce| ce.to_str().ok());

        let (body_bytes, body_decoded) =
            match decompress_body(raw_body_bytes.to_vec(), content_encoding.map(str::to_string)).await {
                Ok(Decompressed::Decoded(decompressed)) => (decompressed, true),
                Ok(Decompressed::Passthrough(body)) => (body, false),
                Err(error) => return decompress_error_response(error, "completion"),
            };

        // If error status, log the full response body for debugging
        if status.is_client_error() || status.is_server_error() {
            let error_body = String::from_utf8_lossy(&body_bytes);
            tracing::error!(
                status = %status,
                url = %backend.base_url(),
                error_body = %error_body,
                "Backend returned error response"
            );
        }

        // Debug: Log received response details
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|ct| ct.to_str().ok())
            .unwrap_or("unknown");
        let body_preview = String::from_utf8_lossy(&body_bytes[..body_bytes.len().min(500)]);
        tracing::debug!(
            backend_status = %status,
            body_size = body_bytes.len(),
            content_type = %content_type,
            content_encoding = ?content_encoding,
            body_preview = %body_preview,
            "Received non-streaming response from backend"
        );

        // Only try to parse as JSON if Content-Type indicates JSON
        let is_json_response = Self::is_json_content_type(content_type);

        // Try to parse as JSON (only if Content-Type is JSON). Fixes and metrics deliberately
        // live BELOW the reprompt block (big-fix 68): both must see the FINAL body. Reprompt
        // judging the raw parsed body is safe - the fix layer rewrites tool-call args and
        // indices, never the stop-vs-continuation shape reprompt reads.
        let parsed = if is_json_response {
            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
                tracing::debug!("Response parsed as JSON successfully");
                Some(json)
            } else {
                // Content-Type says JSON but parsing failed - log warning
                tracing::warn!(
                    body_size = body_bytes.len(),
                    body_preview = %String::from_utf8_lossy(&body_bytes[..body_bytes.len().min(200)]),
                    "Content-Type indicates JSON but parsing failed - returning original body unchanged"
                );
                None
            }
        } else {
            // Content-Type is not JSON - this is expected, just passthrough
            tracing::debug!(
                content_type = %content_type,
                "Non-JSON content type, passing through unchanged"
            );
            None
        };

        // Apply reprompt engine if enabled (OpenAI API path only)
        let json_value = if !is_anthropic_api {
            match (&self.state.reprompt_engine, &request_json, parsed) {
                (Some(engine), Some(req_json), Some(current_json)) => {
                    let path_and_query = request_uri.path_and_query().map_or("/", |pq| pq.as_str());
                    let result = engine.maybe_reprompt(current_json, req_json, path_and_query, backend).await;
                    Some(result)
                }
                (_, _, jv) => jv,
            }
        } else {
            parsed
        };

        // Fixes, then metrics, both on the final (possibly merged) body: the ONE log/export
        // line below carries merged token totals, and its duration spans the reprompt rounds
        // because the start instant comes from handle().
        let (json_value, mut metrics) = if let Some(json) = json_value {
            // Apply fixes with request context if available
            let original_json = json.clone();
            let json = if let Some(ref req_json) = request_json {
                self.state.fix_registry.apply_fixes_with_context(json, req_json)
            } else {
                self.state.fix_registry.apply_fixes(json)
            };
            if json != original_json {
                tracing::debug!("Fixes applied to non-streaming response");
            } else {
                tracing::debug!("No fixes applied to response");
            }

            // Collect stats if enabled
            let mut metrics = if self.state.config.stats.enabled {
                if let Some(ref req_json) = request_json {
                    let mut m = RequestMetrics::from_response(
                        &json,
                        req_json,
                        false, // We forced non-streaming
                        start.elapsed().as_millis() as f64,
                    );
                    // Set group name if we're in multi-backend mode
                    m.group_name = group_name.map(|s| s.to_string());
                    Some(m)
                } else {
                    None
                }
            } else {
                None
            };

            // Fetch and set context_total, gated on the same
            // has_throughput_signal the export gate uses: with no token count
            // there is nothing for context_percent to divide. This also skips
            // warn_context_fetch_failed_once for those samples.
            if let Some(m) = metrics.as_mut().filter(|m| m.has_throughput_signal()) {
                match fetch_context_total(&backend.http_client, backend.base_url(), backend.strip_path_prefix.as_deref()).await
                {
                    Some(ctx_total) => {
                        m.context_total = Some(ctx_total);
                        m.calculate_context_percent();
                    }
                    None => {
                        // Warn once per backend URL, not per request
                        crate::proxy::warn_context_fetch_failed_once(backend.base_url(), &m.model).await;
                        // Continue without context metrics - the request still succeeds
                    }
                }
            }

            (Some(json), metrics)
        } else {
            (None, None)
        };

        // The gate logs the sample and decides whether it is fit to export.
        if let Some(ref mut m) = metrics {
            m.concurrent_requests = Some(self.state.concurrent_requests.load(Ordering::Relaxed));
            if crate::exporters::log_sample_and_should_export(m, self.state.config.stats.format) {
                // Export to remote systems
                let exporters = self.state.exporter_manager.clone();
                let metrics_clone = m.clone();
                tokio::spawn(async move {
                    exporters.export_all(&metrics_clone).await;
                });
            }
        } else {
            // Debug: Log why stats weren't collected
            tracing::debug!(
                stats_enabled = self.state.config.stats.enabled,
                has_request_json = request_json.is_some(),
                "No metrics collected for non-streaming response"
            );
        }

        // Compute final body (used for dump and non-streaming return)
        let final_body = if let Some(ref json) = json_value {
            serde_json::to_vec(json).unwrap_or_else(|e| {
                // json_value is Some only for a parsed-and-fixed Value, which always
                // re-serializes; if it somehow did not, the client gets the backend's
                // ORIGINAL bytes and this names the divergence [C-L9].
                tracing::debug!(error = %e, "Fixed response body failed to re-serialize; forwarding the original backend bytes");
                body_bytes.to_vec()
            })
        } else {
            body_bytes.to_vec()
        };

        // Dump request/response if dump mode is enabled (before any early returns).
        // [C-M10] The dump records the backend's ORIGINAL pre-decompression bytes and
        // the original Content-Type; the transformed body the proxy forwards rides
        // along as a `.decoded` variant only when the bytes actually differ.
        if let Some(ref dump_path) = self.state.dump_path {
            let request_json_clone = request_json.clone();
            let dump_path_clone = dump_path.clone();
            let request_method_str = request_method.to_string();
            let request_uri_str = request_uri.to_string();
            let request_content_type = headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok().map(|s| s.to_string()));
            let response_status = status.as_u16();
            let original_response_bytes = raw_body_bytes.clone();
            let transformed_variant: Option<Vec<u8>> =
                (body_decoded || final_body.as_slice() != &raw_body_bytes[..]).then(|| final_body.clone());

            tokio::spawn(async move {
                if let Some(req_json) = request_json_clone {
                    let req_bytes = serde_json::to_vec(&req_json).unwrap_or_else(|e| {
                        // A parsed Value re-serializes; but an empty request file WITH this
                        // line beats the old silent empty file [C-L9].
                        tracing::debug!(error = %e, "Dump: request JSON failed to serialize; writing an empty request body");
                        Vec::new()
                    });
                    if let Err(e) = dump::dump_request_response(
                        &dump_path_clone,
                        &request_method_str,
                        &request_uri_str,
                        &req_bytes,
                        request_content_type.as_deref(),
                        response_status,
                        &original_response_bytes,
                        original_response_content_type.as_deref(),
                        transformed_variant.as_deref(),
                    )
                    .await
                    {
                        tracing::warn!(error = %e, "Failed to dump request/response pair");
                    }
                }
            });
        }

        // If client wants streaming, synthesize it from complete JSON. Only a 2xx may
        // become a stream: synthesize_* always answers 200 and discards the backend
        // status, so an error body that parses as a completion would arrive as a
        // successful empty turn.
        if client_wants_streaming && status.is_success() {
            if let Some(ref json) = json_value {
                if is_anthropic_api {
                    // Anthropic API: try parsing as Anthropic format first
                    match serde_json::from_value::<AnthropicMessage>(json.clone()) {
                        Ok(anthropic_msg) => {
                            tracing::debug!("Backend returned Anthropic format, synthesizing streaming response");
                            // Infallible since task 62 - no failure arm to route.
                            return synthesize_anthropic_streaming_response(anthropic_msg, &self.state.config.synthesis).await;
                        }
                        Err(_) => {
                            // Backend returned OpenAI format - convert from the RAW buffered
                            // Value (no typed round-trip): every choice is streamed and
                            // unknown fields survive by construction. Malformed OpenAI
                            // bodies keep the task-11 contract: SSE error frame.
                            tracing::debug!("Backend returned OpenAI format, converting to Anthropic for streaming synthesis");
                            match synthesize_anthropic_openai_format_response(json, &self.state.config.synthesis) {
                                Some(response) => return response,
                                None => {
                                    tracing::error!(
                                        response_json = %json_preview(json),
                                        "Backend response is neither Anthropic format nor a well-formed OpenAI completion, ending the stream with an SSE error frame"
                                    );
                                    return anthropic_sse_error_response();
                                }
                            }
                        }
                    }
                } else {
                    // OpenAI API: synthesize in OpenAI SSE format from the RAW buffered
                    // Value - every choice streamed, unknown fields survive by construction.
                    match synthesize_streaming_response(json, &self.state.config.synthesis).await {
                        Ok(response) => {
                            tracing::debug!("Synthesized OpenAI streaming response from complete JSON");
                            return response;
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                response_json = %json_preview(json),
                                "Backend body is not a well-formed completion for synthesis - returning complete JSON"
                            );
                            // Fall through to return JSON
                        }
                    }
                }
            }
        }

        // Return complete JSON response: client wants non-streaming, or OpenAI-path
        // synthesis failed. An Anthropic-path failure never reaches here - it answered
        // with the SSE error frame [C-L8].
        // The backend's Content-Type is copied verbatim, charset parameter included:
        // json_value can only be Some when the original Content-Type already said JSON,
        // so the copied value stays truthful even for a fixed body.
        forward_response(status, &headers, final_body, body_decoded)
    }

    /// Simple pass-through with no fix application or stats collection
    /// Used for monitoring endpoints like /props, /slots, /v1/health
    async fn proxy_passthrough(&self, req: Request<Body>, backend: &Arc<BackendNode>, group_name: Option<&str>) -> Response {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let headers = req.headers().clone();
        let path = uri.path();
        let query = uri.query();

        tracing::debug!(method = %method, path = %path, query = ?query, "Pass-through request");

        // Read body
        let body_bytes = match to_bytes(req.into_body(), 1024 * 1024 * 10).await {
            Ok(bytes) => bytes,
            Err(e) => return body_read_error(e, "10 MiB"),
        };

        // Build complete URL with query string as-is (don't parse/re-encode)
        let effective_path = backend.effective_path(path);
        let backend_url = if let Some(q) = query {
            format!("{}{}?{}", backend.base_url(), effective_path, q)
        } else {
            format!("{}{}", backend.base_url(), effective_path)
        };

        tracing::debug!(
            backend_url = %backend_url,
            has_query = query.is_some(),
            "Building pass-through request"
        );

        // The router matched this method before dispatching here, so clone the validated
        // Method instead of re-parsing its bytes and panicking on a re-reject [C-L7].
        let mut backend_req = backend.http_client.request(method.clone(), &backend_url);

        // Copy headers (skip Host and Authorization as we'll set those explicitly)
        for (name, value) in headers.iter() {
            // Skip headers that will be set explicitly or handled by reqwest
            if name == header::HOST || name == header::AUTHORIZATION {
                continue;
            }

            backend_req = backend_req.header(name, value);
        }

        // Add Authorization header if api_key is configured
        if let Some(ref api_key) = backend.api_key {
            backend_req = backend_req.header(header::AUTHORIZATION, format!("Bearer {}", api_key));
        }
        backend_req = backend_req.body(body_bytes);

        let backend_response = match backend_req.send().await {
            Ok(resp) => resp,
            Err(e) => {
                self.mark_backend_failed(backend, group_name, "connect_error");
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(err_envelope("backend_connect_error", format!("Backend error: {}", e))),
                )
                    .into_response();
            }
        };

        // Pass through response
        let status = backend_response.status();
        self.observe_backend_outcome(backend, group_name, status);
        let headers = backend_response.headers().clone();
        let raw_body = match backend_response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(err_envelope("backend_read_error", format!("Failed to read response: {}", e))),
                )
                    .into_response();
            }
        };

        // Check for Content-Encoding and decompress if needed
        let content_encoding = headers.get(header::CONTENT_ENCODING).and_then(|ce| ce.to_str().ok());

        let (body, body_decoded) = match decompress_body(raw_body.to_vec(), content_encoding.map(str::to_string)).await {
            Ok(Decompressed::Decoded(decompressed)) => (decompressed, true),
            Ok(Decompressed::Passthrough(unchanged)) => (unchanged, false),
            Err(error) => return decompress_error_response(error, "pass-through"),
        };

        forward_response(status, &headers, body, body_decoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::augment::AugmentBackend;
    use crate::backends::{BackendNode, GroupedLoadBalancer, LoadBalancer, RoundRobinBalancer};
    use crate::config::{AppConfig, BackendConfig, StreamingMode};
    use crate::exporters::ExporterManager;
    use crate::fixes::FixRegistry;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    #[allow(dead_code)]
    fn create_test_handler_with_streaming(streaming_config: StreamingMode) -> ProxyHandler {
        let config = AppConfig {
            server: crate::config::ServerConfig {
                port: 8066,
                host: "0.0.0.0".to_string(),
                max_concurrent_requests: crate::config::default_max_concurrent(),
                allowed_origins: None,
            },
            backend: Some(BackendConfig::default()),
            backends: None,
            fixes: crate::config::FixesConfig {
                enabled: false,
                modules: HashMap::new(),
            },
            stats: crate::config::StatsConfig {
                enabled: false,
                format: crate::config::StatsFormat::Pretty,
            },
            exporters: crate::config::ExportersConfig {
                influxdb: crate::config::InfluxDbConfig {
                    enabled: false,
                    url: "http://localhost:8086".to_string(),
                    org: "test".to_string(),
                    bucket: "test".to_string(),
                    token: "test".to_string(),
                    batch_size: 1,
                    flush_interval_seconds: 1,
                },
            },
            streaming: streaming_config,
            synthesis: crate::config::SynthesisConfig::default(),
            augment_backend: None,
            reprompt: None,
            dump: crate::config::DumpConfig::default(),
        };

        let default_node = BackendNode {
            url: "http://localhost:8080".to_string(),
            model: None,
            api_key: None,
            timeout_seconds: 300,
            http_client: reqwest::Client::new(),
            active_requests: Arc::new(AtomicUsize::new(0)),
            strip_path_prefix: None,
            temperature: None,
            healthy: std::sync::atomic::AtomicBool::new(true),
            cooldown_until: std::sync::Mutex::new(std::time::Instant::now()),
        };
        let load_balancer = Arc::new(RoundRobinBalancer::new(vec![Arc::new(default_node)]).unwrap());
        let fix_registry = FixRegistry::new();
        let exporter_manager = ExporterManager::new();

        ProxyHandler::new(ProxyState {
            config: std::sync::Arc::new(config),
            load_balancer,
            fix_registry: std::sync::Arc::new(fix_registry),
            exporter_manager: std::sync::Arc::new(exporter_manager),
            augment_backend: None,
            reprompt_engine: None,
            hide_requests: false,
            log_augmented_request_text: false,
            dump_path: None,
            concurrent_requests: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            backend_streaming_fallback_hits: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            openai_stream_passthrough_total: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            backend_nonsse_when_streamed_for: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            anthropic_buffered_responses_total: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            anthropic_buffered_notice_once: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            rejected_requests: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            concurrent_semaphore: Some(std::sync::Arc::new(tokio::sync::Semaphore::new(100))),
        })
    }

    // REMOVED: Tests for should_stream() method
    // The method has been removed since we now always force non-streaming backend requests
    // and synthesize streaming responses when clients request them.
    //
    // New architecture:
    // - All backend requests: stream = false
    // - Client wants streaming: synthesize from complete JSON
    // - Client wants non-streaming: return complete JSON as-is

    #[test]
    fn test_is_json_content_type() {
        // Standard JSON content types
        assert!(ProxyHandler::is_json_content_type("application/json"));
        assert!(ProxyHandler::is_json_content_type("application/json; charset=utf-8"));
        assert!(ProxyHandler::is_json_content_type("APPLICATION/JSON"));

        // Vendor-specific JSON types
        assert!(ProxyHandler::is_json_content_type("application/vnd.api+json"));
        assert!(ProxyHandler::is_json_content_type("application/vnd.github.v3+json"));
        assert!(ProxyHandler::is_json_content_type(
            "application/vnd.custom+json; charset=utf-8"
        ));

        // Non-JSON types
        assert!(!ProxyHandler::is_json_content_type("text/html"));
        assert!(!ProxyHandler::is_json_content_type("text/plain"));
        assert!(!ProxyHandler::is_json_content_type("text/javascript"));
        assert!(!ProxyHandler::is_json_content_type("application/javascript"));
        assert!(!ProxyHandler::is_json_content_type("image/png"));
        assert!(!ProxyHandler::is_json_content_type("image/jpeg"));
        assert!(!ProxyHandler::is_json_content_type("text/css"));
        assert!(!ProxyHandler::is_json_content_type("application/octet-stream"));

        // big-fix 93 [C-L9]: the media type is what precedes the first ';'. Parameters must
        // never change the classification - in either direction. These two stay JSON:
        assert!(ProxyHandler::is_json_content_type("application/json;"));
        assert!(ProxyHandler::is_json_content_type("application/json;q=1;chrome=1"));
        // RED [C-L9]: the substring class the parameter strip removes. These are NOT JSON
        // documents: application/json-seq (RFC 7464 text sequences) and application/jsonpath
        // (RFC 9535) are real IANA types that merely CONTAIN "application/json"; a JSON-looking
        // parameter on a non-JSON type is likewise not JSON.
        assert!(
            !ProxyHandler::is_json_content_type("application/json-seq"),
            "json-seq is not a JSON document"
        );
        assert!(
            !ProxyHandler::is_json_content_type("application/jsonpath"),
            "jsonpath is not a JSON document"
        );
        assert!(
            !ProxyHandler::is_json_content_type("x-application/json"),
            "an unregistered type that happens to contain the substring is not JSON"
        );
        assert!(
            !ProxyHandler::is_json_content_type("text/plain; charset=\"application/json\""),
            "a parameter must not turn a text body into JSON"
        );
    }

    fn bare_node(model: Option<&str>) -> BackendNode {
        BackendNode {
            url: "http://localhost:8080".to_string(),
            model: model.map(|m| m.to_string()),
            api_key: None,
            timeout_seconds: 300,
            http_client: reqwest::Client::new(),
            active_requests: Arc::new(AtomicUsize::new(0)),
            strip_path_prefix: None,
            temperature: None,
            healthy: std::sync::atomic::AtomicBool::new(true),
            cooldown_until: std::sync::Mutex::new(std::time::Instant::now()),
        }
    }

    fn rewrite(body: serde_json::Value, allow_stream: bool) -> (serde_json::Value, bool) {
        let (bytes, sent_stream) = ProxyHandler::apply_backend_overrides_bytes(
            body.to_string().as_bytes(),
            &bare_node(None),
            allow_stream,
            "/v1/chat/completions",
        );
        (
            serde_json::from_slice(&bytes).expect("rewritten body must be valid JSON"),
            sent_stream,
        )
    }

    #[test]
    fn passthrough_keeps_stream_true_and_asks_for_usage() {
        let (out, sent_stream) = rewrite(serde_json::json!({"stream": true}), true);
        assert!(sent_stream, "passthrough must hand stream:true onward");
        assert_eq!(out["stream"], serde_json::json!(true));
        assert_eq!(out["stream_options"]["include_usage"], serde_json::json!(true));
    }

    #[test]
    fn passthrough_still_buffers_when_client_never_asked_to_stream() {
        let (out, sent_stream) = rewrite(serde_json::json!({}), true);
        assert!(!sent_stream);
        assert_eq!(out["stream"], serde_json::json!(false));
        assert!(out.get("stream_options").is_none(), "no stream, no stream_options");
    }

    #[test]
    fn fake_mode_is_unchanged_forces_false_and_strips_stream_options() {
        let (out, sent_stream) = rewrite(
            serde_json::json!({"stream": true, "stream_options": {"include_usage": true}}),
            false,
        );
        assert!(!sent_stream);
        assert_eq!(out["stream"], serde_json::json!(false));
        assert!(out.get("stream_options").is_none());
    }

    #[test]
    fn client_explicitly_declining_usage_is_respected() {
        let (out, sent_stream) = rewrite(
            serde_json::json!({"stream": true, "stream_options": {"include_usage": false}}),
            true,
        );
        assert!(sent_stream, "declining usage is not declining the stream");
        assert_eq!(
            out["stream_options"]["include_usage"],
            serde_json::json!(false),
            "explicit client intent must not be overwritten"
        );
    }

    #[test]
    fn injection_preserves_other_stream_options_keys() {
        let (out, _) = rewrite(serde_json::json!({"stream": true, "stream_options": {"observer": "x"}}), true);
        assert_eq!(out["stream_options"]["observer"], serde_json::json!("x"));
        assert_eq!(out["stream_options"]["include_usage"], serde_json::json!(true));
    }

    #[test]
    fn a_stream_options_that_is_not_an_object_is_neither_rewritten_nor_fatal() {
        for junk in [serde_json::json!("yes"), serde_json::json!([]), serde_json::json!(7)] {
            let (out, sent_stream) = rewrite(serde_json::json!({"stream": true, "stream_options": junk.clone()}), true);
            assert!(sent_stream);
            assert_eq!(out["stream_options"], junk, "a value we do not parse is forwarded untouched");
        }
    }

    #[test]
    fn non_json_body_passes_through_and_reports_no_stream() {
        let raw = b"not json at all".to_vec();
        let (bytes, sent_stream) =
            ProxyHandler::apply_backend_overrides_bytes(&raw, &bare_node(None), true, "/v1/chat/completions");
        assert_eq!(bytes, raw);
        assert!(!sent_stream);
    }

    #[test]
    fn backend_model_and_temperature_overrides_apply_in_both_modes() {
        let node = BackendNode {
            model: Some("renamed".to_string()),
            temperature: Some(0.1),
            ..bare_node(None)
        };
        for allow_stream in [true, false] {
            let body = serde_json::json!({"model": "client-name", "stream": true, "temperature": 0.9});
            let (bytes, _) = ProxyHandler::apply_backend_overrides_bytes(
                body.to_string().as_bytes(),
                &node,
                allow_stream,
                "/v1/chat/completions",
            );
            let out: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(out["model"], serde_json::json!("renamed"));
            assert_eq!(out["temperature"], serde_json::json!(0.1));
        }
    }

    /// Build a handler with an arbitrary balancer, augment backend, and streaming mode,
    /// everything else at the same inert settings as `create_test_handler_with_streaming`
    /// (no fixes, no stats, no exporters). Nothing talks to a real backend in these tests.
    fn handler_with_balancer(
        load_balancer: Arc<dyn LoadBalancer>,
        augment_backend: Option<Arc<AugmentBackend>>,
        streaming: StreamingMode,
    ) -> ProxyHandler {
        let config = AppConfig {
            server: crate::config::ServerConfig {
                port: 8066,
                host: "0.0.0.0".to_string(),
                max_concurrent_requests: crate::config::default_max_concurrent(),
                allowed_origins: None,
            },
            backend: Some(BackendConfig::default()),
            backends: None,
            fixes: crate::config::FixesConfig {
                enabled: false,
                modules: HashMap::new(),
            },
            stats: crate::config::StatsConfig {
                enabled: false,
                format: crate::config::StatsFormat::Pretty,
            },
            exporters: crate::config::ExportersConfig {
                influxdb: crate::config::InfluxDbConfig {
                    enabled: false,
                    url: "http://localhost:8086".to_string(),
                    org: "test".to_string(),
                    bucket: "test".to_string(),
                    token: "test".to_string(),
                    batch_size: 1,
                    flush_interval_seconds: 1,
                },
            },
            streaming,
            synthesis: crate::config::SynthesisConfig::default(),
            augment_backend: None,
            reprompt: None,
            dump: crate::config::DumpConfig::default(),
        };

        ProxyHandler::new(ProxyState {
            config: Arc::new(config),
            load_balancer,
            fix_registry: Arc::new(FixRegistry::new()),
            exporter_manager: Arc::new(ExporterManager::new()),
            augment_backend,
            reprompt_engine: None,
            hide_requests: false,
            log_augmented_request_text: false,
            dump_path: None,
            concurrent_requests: Arc::new(AtomicUsize::new(0)),
            backend_streaming_fallback_hits: Arc::new(AtomicUsize::new(0)),
            openai_stream_passthrough_total: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            backend_nonsse_when_streamed_for: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            anthropic_buffered_responses_total: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            anthropic_buffered_notice_once: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            rejected_requests: Arc::new(AtomicUsize::new(0)),
            concurrent_semaphore: Some(Arc::new(tokio::sync::Semaphore::new(100))),
        })
    }

    fn completion_request() -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "model": "ghost-model",
                    "messages": [{"role": "user", "content": "hello"}]
                })
                .to_string(),
            ))
            .unwrap()
    }

    async fn json_error_body(res: Response) -> serde_json::Value {
        let bytes = to_bytes(res.into_body(), 64 * 1024).await.expect("body must be readable");
        serde_json::from_slice(&bytes).expect("error body must be valid JSON")
    }

    #[tokio::test]
    async fn no_backend_configured_answers_503_with_error_envelope() {
        // GroupedLoadBalancer with zero groups: every model selection fails with
        // NoMatchingBackend, the exact state of a proxy started without a backend.
        let empty_groups: HashMap<String, crate::config::BackendGroupConfig> = HashMap::new();
        let balancer = Arc::new(GroupedLoadBalancer::new(empty_groups).expect("empty group map must build"));
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert_eq!(
            res.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "no-backend must be a real 503, not a 200 with an error body"
        );
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
            Some("application/json"),
            "error body must be served as JSON"
        );
        let body = json_error_body(res).await;
        assert_eq!(body["error"]["type"], serde_json::json!("no_backend"));
        assert!(
            body["error"]["message"].as_str().is_some_and(|m| !m.is_empty()),
            "envelope must carry a non-empty message, got: {body}"
        );
    }

    #[tokio::test]
    async fn augment_backend_failure_answers_502_with_error_envelope() {
        // Dead augment URL: connect to 127.0.0.1:1 is refused instantly, no listener needed.
        // The main-backend node is never contacted - augmentation fails before forwarding.
        let dead_augment = AugmentBackend {
            url: "http://127.0.0.1:1".to_string(),
            model: "augment-model".to_string(),
            prompt_file: "nonexistent-augment-prompt.md".to_string(),
            request_prompt_file: "nonexistent-request-prompt.md".to_string(),
            http_client: reqwest::Client::new(),
        };
        let balancer = Arc::new(RoundRobinBalancer::new(vec![Arc::new(bare_node(None))]).unwrap());
        let handler = handler_with_balancer(balancer, Some(Arc::new(dead_augment)), StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert_eq!(
            res.status(),
            StatusCode::BAD_GATEWAY,
            "augment failure must be a real 502, not a 200 with an error body"
        );
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
            Some("application/json"),
            "error body must be served as JSON"
        );
        let body = json_error_body(res).await;
        assert_eq!(body["error"]["type"], serde_json::json!("augment_backend_error"));
        assert!(
            body["error"]["message"].as_str().is_some_and(|m| !m.is_empty()),
            "envelope must carry a non-empty message, got: {body}"
        );
    }

    /// Capture writer (repo convention, 4th instance: fixes/registry.rs, augment.rs,
    /// proxy/context.rs): thread-local `set_default` + `current_thread` +
    /// `pin_interest_cache_for_tests` per the flake-family protocol.
    #[derive(Clone)]
    struct HandlerCaptureWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for HandlerCaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("capture lock").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for HandlerCaptureWriter {
        type Writer = Self;
        fn make_writer(&self) -> Self {
            self.clone()
        }
    }

    /// big-fix 93 [C-L9] RED: the augment pipeline fetches a real augmentation, but the
    /// request body fails the typed from_value parse, so injection is SKIPPED and the
    /// original bytes are forwarded. Baseline swallows the parse error silently - the log
    /// reads like augmentation ran. The skip must be named at debug level.
    #[tokio::test(flavor = "current_thread")]
    async fn augmentation_typed_parse_bypass_is_logged_not_silent() {
        crate::fixes::pin_interest_cache_for_tests();
        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let _sub = tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_ansi(false)
                .with_max_level(tracing::Level::DEBUG)
                .with_writer(HandlerCaptureWriter(buf.clone()))
                .finish(),
        );

        // Fake augment backend: answers the augmentation call with content "AUG".
        let augment_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("augment bind");
        let augment_port = augment_listener.local_addr().expect("augment addr").port();
        tokio::spawn(async move {
            let accepted = tokio::time::timeout(std::time::Duration::from_secs(5), augment_listener.accept()).await;
            let Ok(Ok((mut sock, _))) = accepted else { return };
            {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut head: Vec<u8> = Vec::new();
                let mut tmp = [0u8; 1024];
                loop {
                    match sock.read(&mut tmp).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            head.extend_from_slice(&tmp[..n]);
                            if head.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                    }
                }
                let body = r#"{"choices":[{"message":{"role":"assistant","content":"AUG"}}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });

        let augment = AugmentBackend {
            url: format!("http://127.0.0.1:{augment_port}"),
            model: "augment-model".to_string(),
            prompt_file: "task93-no-such-augment-prompt.md".to_string(),
            request_prompt_file: "task93-no-such-request-prompt.md".to_string(),
            http_client: reqwest::Client::new(),
        };

        let (port, rx) = recording_backend(completion_response_bytes()).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, Some(Arc::new(augment)), StreamingMode::default());

        // Well-formed JSON whose model is a NUMBER: extract_user_content_from_json still
        // finds "hi" (raw walker), so the augmentation is fetched; but the typed
        // ChatCompletionRequest parse fails and the injection must be bypassed.
        let body = serde_json::json!({"model": 123, "messages": [{"role": "user", "content": "hi"}]});
        let res = handler
            .handle(request_with_body(
                Method::POST,
                "/v1/chat/completions",
                Body::from(body.to_string()),
            ))
            .await;
        assert_eq!(res.status(), StatusCode::OK, "a bypassed injection is still a served request");

        let wire = recorded_raw(rx).await;
        assert!(
            wire.contains("\"model\":123"),
            "the ORIGINAL request must reach the backend (override re-serializes the untouched Value):\n{wire}"
        );
        assert!(
            !wire.contains("AUG"),
            "the fetched augmentation must NOT be injected into an unparseable body:\n{wire}"
        );

        let log = String::from_utf8_lossy(&buf.lock().expect("capture lock").clone()).to_string();
        assert!(
            log.contains("Augmentation injection skipped"),
            "the typed-parse bypass must be named at debug level, captured:\n{log}"
        );
        assert!(log.contains("ChatCompletionRequest"), "the log must name what failed to parse:\n{log}");
    }

    #[test]
    fn err_envelope_carries_kind_and_untrusted_text_verbatim() {
        // CJK + backend-controlled text must survive Into<String> untouched, no panic.
        let v = err_envelope("augment_backend_error", "后端错误: 拒绝连接 🤷");
        assert_eq!(v["error"]["type"], serde_json::json!("augment_backend_error"));
        assert_eq!(
            v["error"]["message"],
            serde_json::json!("后端错误: 拒绝连接 🤷"),
            "message must round-trip through the envelope unchanged"
        );
    }

    // ---- big-fix task 5: plain-text error bodies must become JSON envelopes ----
    //
    // Each test below drives a real error site in handle()/proxy_passthrough().
    // Pre-conversion these sites return (StatusCode, String), which Axum serves as
    // `text/plain; charset=utf-8` with a bare human sentence — so the JSON
    // content-type and error.type assertions are exactly what fails before the fix.

    /// Body that fails on its first frame with a CJK+emoji message: exercises the
    /// 400 body-read sites and untrusted-text survival through the envelope at once.
    fn exploding_body() -> Body {
        Body::from_stream(futures::stream::once(async {
            Err::<bytes::Bytes, std::io::Error>(std::io::Error::other("后端管道炸了 🤷"))
        }))
    }

    /// Frame a raw HTTP/1.1 response (always `connection: close`) for the fake backends.
    fn raw_http_response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
        let mut out = format!("HTTP/1.1 {status}\r\n").into_bytes();
        for (name, value) in headers {
            out.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
        }
        out.extend_from_slice(b"connection: close\r\n\r\n");
        out.extend_from_slice(body);
        out
    }

    /// One-shot backend: answers the first connection with `response` bytes verbatim,
    /// then drops the socket (so a promised-but-unwritten body stays truncated).
    async fn one_shot_backend(response: Vec<u8>) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fake backend must bind");
        let port = listener.local_addr().expect("fake backend addr").port();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.expect("one connection");
            {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut req = [0u8; 8192];
                let _ = sock.read(&mut req).await;
                let _ = sock.write_all(&response).await;
                let _ = sock.flush().await;
            }
        });
        port
    }

    fn node_at_port(port: u16) -> Arc<BackendNode> {
        Arc::new(BackendNode {
            url: format!("http://127.0.0.1:{port}"),
            ..bare_node(None)
        })
    }

    fn request_with_body(method: Method, uri: &str, body: Body) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .unwrap()
    }

    /// The one envelope shape asserted at every converted site: original status kept,
    /// JSON content-type, {"error":{"type":kind,"message":non-empty}}. Returns the
    /// parsed body so callers can probe the message further.
    async fn assert_error_envelope(res: Response, status: StatusCode, kind: &str) -> serde_json::Value {
        assert_eq!(res.status(), status, "status code must not change");
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
            Some("application/json"),
            "error body must be served as JSON, not plain text"
        );
        let body = json_error_body(res).await;
        assert_eq!(body["error"]["type"], serde_json::json!(kind), "envelope kind, got: {body}");
        assert!(
            body["error"]["message"].as_str().is_some_and(|m| !m.is_empty()),
            "envelope must carry the human message, got: {body}"
        );
        body
    }

    #[tokio::test]
    async fn unreadable_request_body_answers_400_with_error_envelope() {
        // Body read fails before the balancer hands out a node; node url is never contacted.
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(1)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler
            .handle(request_with_body(Method::POST, "/v1/chat/completions", exploding_body()))
            .await;

        // Adversarial probe: untrusted CJK+emoji error text must reach `message` verbatim.
        let body = assert_error_envelope(res, StatusCode::BAD_REQUEST, "invalid_request_json").await;
        assert!(
            body["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("后端管道炸了 🤷")),
            "untrusted text must survive into the envelope, got: {body}"
        );
    }

    #[tokio::test]
    async fn passthrough_unreadable_body_answers_400_with_error_envelope() {
        // proxy_passthrough re-reads its own request body. Reached directly because
        // handle() pre-buffers the body before rebuilding monitoring passthroughs,
        // so this site is only exercisable at the method boundary.
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(1)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());
        let node = node_at_port(1);

        let res = handler
            .proxy_passthrough(request_with_body(Method::GET, "/health", exploding_body()), &node, None)
            .await;

        assert_error_envelope(res, StatusCode::BAD_REQUEST, "invalid_request_json").await;
    }

    #[tokio::test]
    async fn backend_connect_failure_answers_502_with_error_envelope() {
        // Port 1: connect is refused instantly, deterministic without any listener.
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(1)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert_error_envelope(res, StatusCode::BAD_GATEWAY, "backend_connect_error").await;
    }

    #[tokio::test]
    async fn backend_body_read_failure_answers_502_with_error_envelope() {
        // content-length promises 100 bytes; only 8 arrive before the socket closes.
        let response = raw_http_response(
            "200 OK",
            &[("content-type", "application/json"), ("content-length", "100")],
            b"{\"id\":1",
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert_error_envelope(res, StatusCode::BAD_GATEWAY, "backend_read_error").await;
    }

    #[tokio::test]
    async fn backend_decompress_failure_answers_502_with_error_envelope() {
        // zstd is NOT a compiled reqwest feature, so reqwest forwards the body and
        // header untouched and the proxy's own decompress_body zstd branch fails.
        let response = raw_http_response(
            "200 OK",
            &[("content-type", "application/json"), ("content-encoding", "zstd")],
            b"not-a-zstd-frame",
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert_error_envelope(res, StatusCode::BAD_GATEWAY, "backend_decompress_error").await;
    }

    #[tokio::test]
    async fn passthrough_connect_failure_answers_502_with_error_envelope() {
        // GET /v1/health takes the monitoring pass-through arm inside handle().
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(1)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler
            .handle(request_with_body(Method::GET, "/v1/health", Body::empty()))
            .await;

        assert_error_envelope(res, StatusCode::BAD_GATEWAY, "backend_connect_error").await;
    }

    #[tokio::test]
    async fn proxy_metrics_renders_context_cache_stale_skips() {
        // big-fix 94 carry: the staleness counter must be readable on /proxy/metrics.
        // The accessor is process-global and other tests in this binary can bump it
        // concurrently, so pin the rendered value between a pre- and post-read instead
        // of demanding one exact number.
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(1)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let before = crate::proxy::context::context_cache_stale_skips();
        let res = handler
            .handle(request_with_body(Method::GET, "/proxy/metrics", Body::empty()))
            .await;
        let body = body_text(res).await;
        let after = crate::proxy::context::context_cache_stale_skips();

        let prefix = "llama_proxy_context_cache_stale_skips_total ";
        let rendered: u64 = body
            .lines()
            .find_map(|line| line.strip_prefix(prefix))
            .unwrap_or_else(|| panic!("metrics body must carry {prefix}:\n{body}"))
            .trim()
            .parse()
            .expect("stale-skip line must carry a u64");
        assert!(
            (before..=after).contains(&rendered),
            "rendered {rendered} outside [{before}, {after}] monotonic window"
        );
        assert!(
            body.contains("# TYPE llama_proxy_context_cache_stale_skips_total counter"),
            "stale-skip line needs a TYPE declaration:\n{body}"
        );
    }

    #[tokio::test]
    async fn passthrough_body_read_failure_answers_502_with_error_envelope() {
        let response = raw_http_response(
            "200 OK",
            &[("content-type", "application/json"), ("content-length", "100")],
            b"xx",
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler
            .handle(request_with_body(Method::GET, "/v1/health", Body::empty()))
            .await;

        assert_error_envelope(res, StatusCode::BAD_GATEWAY, "backend_read_error").await;
    }

    #[tokio::test]
    async fn passthrough_decompress_failure_answers_502_with_error_envelope() {
        let response = raw_http_response(
            "200 OK",
            &[("content-type", "application/json"), ("content-encoding", "zstd")],
            b"not-a-zstd-frame",
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler
            .handle(request_with_body(Method::GET, "/v1/health", Body::empty()))
            .await;

        assert_error_envelope(res, StatusCode::BAD_GATEWAY, "backend_decompress_error").await;
    }

    #[tokio::test]
    async fn json_backend_content_type_is_forwarded_verbatim_including_charset() {
        // [C-L6] A backend answering `application/json; charset=utf-8` must have that
        // exact value reach the client. The old code re-emitted a bare
        // "application/json" whenever it had parsed the body as JSON, silently
        // dropping the charset parameter.
        let backend_body = serde_json::json!({
            "id": "cmpl-1",
            "object": "chat.completion",
            "created": 1,
            "model": "ghost-model",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "hi"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        })
        .to_string();
        let len = backend_body.len().to_string();
        let response = raw_http_response(
            "200 OK",
            &[
                ("content-type", "application/json; charset=utf-8"),
                ("content-length", len.as_str()),
            ],
            backend_body.as_bytes(),
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("<missing>"),
            "application/json; charset=utf-8",
            "backend JSON content-type must pass through verbatim, charset included"
        );
    }

    // ---- big-fix task 6: body-reader errors map to 413 vs 400 by kind ----
    //
    // Pre-fix, BOTH to_bytes Err arms answer 400 invalid_request_json, so the
    // 413/request_body_too_large assertions below are exactly the RED signature.
    // No DefaultBodyLimit layer exists; the caps are the hard-coded to_bytes
    // limits (100 MiB main path, 10 MiB pass-through).

    /// Streaming body of `chunks` × ~1 MiB of CJK UTF-8 frames: with enough
    /// chunks it crosses the 10 MiB pass-through cap. Bounded stream, no
    /// sleeps - deterministic, and at most ~cap+1 MiB is ever buffered.
    fn oversized_stream_body(chunks: usize) -> Body {
        let frame = bytes::Bytes::from("汉字符负载".repeat(70_000)); // 70_000 × 15 B ≈ 1 MiB
        Body::from_stream(futures::stream::iter(
            (0..chunks).map(move |_| Ok::<_, std::io::Error>(frame.clone())),
        ))
    }

    /// ~1 MiB frames followed by a mid-stream read failure: the client-half
    /// "drop the body" probe - a genuine read error, never a limit error.
    fn mid_stream_failing_body(chunks: usize) -> Body {
        let frame = bytes::Bytes::from(vec![b'x'; 1024 * 1024]);
        let ok_frames = (0..chunks).map(move |_| Ok::<_, std::io::Error>(frame.clone()));
        let drop = std::iter::once(Err(std::io::Error::other("client aborted mid-body 🚪")));
        Body::from_stream(futures::stream::iter(ok_frames.chain(drop)))
    }

    #[tokio::test]
    async fn oversized_passthrough_stream_body_answers_413_with_limit_message() {
        // 11 MiB streamed into the 10 MiB pass-through cap. proxy_passthrough
        // is called directly because handle() pre-buffers with the 100 MiB
        // main cap before rebuilding passthrough requests (task 5 learning).
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(1)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());
        let node = node_at_port(1);

        let res = handler
            .proxy_passthrough(
                request_with_body(Method::GET, "/health", oversized_stream_body(11)),
                &node,
                None,
            )
            .await;

        let body = assert_error_envelope(res, StatusCode::PAYLOAD_TOO_LARGE, "request_body_too_large").await;
        assert!(
            body["error"]["message"].as_str().is_some_and(|m| m.contains("10 MiB")),
            "the 413 must name the applicable 10 MiB pass-through cap, got: {body}"
        );
    }

    #[tokio::test]
    async fn mid_stream_body_failure_still_answers_400_not_413() {
        // Guard (green pre AND post): a real read failure on the main path
        // (4 MiB of frames, then the stream dies) must stay 400 with the
        // unreadable-text verbatim - the 413 branch must not swallow it.
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(1)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler
            .handle(request_with_body(
                Method::POST,
                "/v1/chat/completions",
                mid_stream_failing_body(4),
            ))
            .await;

        let body = assert_error_envelope(res, StatusCode::BAD_REQUEST, "invalid_request_json").await;
        assert!(
            body["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("client aborted mid-body 🚪")),
            "untrusted read-error text must survive into the 400 envelope, got: {body}"
        );
    }

    /// A TRUE over-limit error from the same axum surface the handlers use:
    /// `to_bytes` with a 1 MiB cap over a bigger streaming body.
    async fn real_length_limit_error() -> axum::Error {
        to_bytes(oversized_stream_body(4), 1024 * 1024)
            .await
            .expect_err("4 MiB body must exceed a 1 MiB cap")
    }

    #[tokio::test]
    async fn real_axum_over_limit_error_is_the_classified_surface() {
        // Pins the classification primitive against the REAL error, not a
        // hand-made string: axum must surface http-body-util's LengthLimitError
        // Display verbatim through its transparent wrapper.
        let e = real_length_limit_error().await;
        assert_eq!(
            e.to_string(),
            "length limit exceeded",
            "axum-core error Display must delegate to the boxed LengthLimitError"
        );
        assert!(
            is_length_limit_error(&e),
            "the real over-limit error must classify as a length limit"
        );

        let other = to_bytes(mid_stream_failing_body(2), 1024 * 1024 * 100)
            .await
            .expect_err("mid-stream io error must surface");
        assert!(
            !is_length_limit_error(&other),
            "an ordinary read failure must NOT classify as a length limit"
        );
    }

    #[tokio::test]
    async fn main_path_limit_branch_maps_real_error_to_413_with_cap_label() {
        // The main path's 100 MiB cap needs a >100 MiB body to trigger through
        // handle() - too heavy for CI (a 100+ MiB buffer per run, flaky under
        // parallel workers). Both Err arms share body_read_error, whose 413
        // branch is pinned end-to-end by oversized_passthrough_stream_body_...
        // (10 MiB side); this test pins the SAME branch with the main path's
        // cap label against a real LengthLimitError.
        let e = real_length_limit_error().await;
        let res = body_read_error(e, "100 MiB");
        let body = assert_error_envelope(res, StatusCode::PAYLOAD_TOO_LARGE, "request_body_too_large").await;
        assert!(
            body["error"]["message"].as_str().is_some_and(|m| m.contains("100 MiB")),
            "the 413 must name the applicable 100 MiB main-path cap, got: {body}"
        );
    }

    // ---- big-fix task 18: the BackendGuard claim must track streamed-body lifetime ----
    //
    // A streaming response body is lazy: the backend keeps generating while the
    // client polls the body, long after handle() returned the headers. Today the
    // guard drops at handler-return, so the pending==1 assertion below IS the
    // accounting bug (RED signature: the load balancer sees the node idle while
    // it is still generating). The ==0 assertions pin the release edges —
    // drain-to-end and mid-flight drop — which must hold before AND after.

    /// Connection-close framed SSE backend: writes headers + the first chunk,
    /// then HOLDS the connection open until the test releases it; on release it
    /// writes `tail` and closes the socket (EOF ends the body). The test drives
    /// both edges - no sleeps, fully deterministic.
    async fn held_open_sse_backend(first: &'static [u8], tail: &'static [u8]) -> (u16, tokio::sync::oneshot::Sender<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("sse fake must bind");
        let port = listener.local_addr().expect("sse fake addr").port();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut sock, _) = listener.accept().await.expect("one connection");
            let mut req = [0u8; 8192];
            let _ = sock.read(&mut req).await;
            let _ = sock
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n")
                .await;
            let _ = sock.write_all(first).await;
            let _ = sock.flush().await;
            // Body stays open until the test releases, then the stream ends.
            let _ = release_rx.await;
            let _ = sock.write_all(tail).await;
            let _ = sock.flush().await;
            // sock drops here -> socket closed -> body EOF
        });
        (port, release_tx)
    }

    #[tokio::test]
    async fn guard_claim_stays_held_while_streamed_body_unconsumed() {
        let (port, _release) = held_open_sse_backend(b"data: a\n\n", b"data: [DONE]\n\n").await;
        let node = node_at_port(port);
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node.clone()]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;
        assert_eq!(res.status(), StatusCode::OK);

        // Headers are out, the body has not been touched once: the backend is
        // still generating and the node must still count this request. A guard
        // dropped at handle()-return reads 0 here - the undercount this task fixes.
        assert_eq!(
            node.active_requests.load(Ordering::Acquire),
            1,
            "claim must stay held while the streamed body is still pending"
        );
    }

    #[tokio::test]
    async fn guard_claim_released_after_body_drained_to_end() {
        let (port, release) = held_open_sse_backend(b"data: a\n\n", b"data: [DONE]\n\n").await;
        let node = node_at_port(port);
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node.clone()]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;
        release.send(()).expect("backend task must still be listening");

        let body = to_bytes(res.into_body(), 1024 * 1024).await.expect("SSE body must drain");
        assert!(
            String::from_utf8_lossy(&body).contains("data: [DONE]"),
            "drain must reach the end of the stream"
        );
        assert_eq!(
            node.active_requests.load(Ordering::Acquire),
            0,
            "claim must be released once the body completed"
        );
    }

    #[tokio::test]
    async fn guard_claim_released_when_body_dropped_mid_stream() {
        // cancel_resume probe: a client that disconnects mid-stream (body dropped
        // while the backend still holds the socket open) must not leak the claim.
        let (port, _release) = held_open_sse_backend(b"data: a\n\n", b"data: [DONE]\n\n").await;
        let node = node_at_port(port);
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node.clone()]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;
        assert_eq!(res.status(), StatusCode::OK);
        drop(res);

        assert_eq!(
            node.active_requests.load(Ordering::Acquire),
            0,
            "mid-flight body drop must release the claim (no phantom +1)"
        );
    }

    // ---- big-fix task 13: routing runs on the effective request path [C-H4] ----
    //
    // A node with `strip_path_prefix` is mounted behind that prefix on the client side:
    // the client asks for "/completions/v1/messages" while the backend-native path is
    // "/v1/messages". Before the fix the route match and the API-format detection looked
    // at the ORIGINAL path only, so such a request was misclassified as OpenAI-shaped
    // (or as an unknown route) while the bytes forwarded already went to the native path.

    /// One-shot backend that ALSO reports the full raw request (request line + headers +
    /// body, assembled per content-length) so tests can pin WHICH path and WHICH bytes
    /// the proxy actually put on the wire.
    async fn recording_backend(response: Vec<u8>) -> (u16, tokio::sync::mpsc::Receiver<Vec<u8>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("recording backend must bind");
        let port = listener.local_addr().expect("recording backend addr").port();
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.expect("one connection");
            {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut raw = Vec::new();
                let mut buf = [0u8; 8192];
                let mut content_length: Option<usize> = None;
                loop {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                    let head = String::from_utf8_lossy(&raw)
                        .lines()
                        .take_while(|l| !l.is_empty())
                        .collect::<Vec<_>>()
                        .join("\r\n")
                        .to_lowercase();
                    if content_length.is_none() {
                        content_length = head.lines().find_map(|l| {
                            l.strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        });
                    }
                    let sep = raw.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4);
                    match content_length {
                        Some(cl) if sep.is_some_and(|s| raw.len() - s >= cl) => break,
                        None if sep.is_some() => break,
                        _ => {}
                    }
                }
                let _ = tx.send(raw).await;
                let _ = sock.write_all(&response).await;
                let _ = sock.flush().await;
            }
        });
        (port, rx)
    }

    async fn recorded_raw(mut rx: tokio::sync::mpsc::Receiver<Vec<u8>>) -> String {
        let raw = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("backend must receive a request within 5s")
            .expect("request head");
        String::from_utf8_lossy(&raw).into_owned()
    }

    /// First line of the recorded request ("POST /v1/messages HTTP/1.1"), bounded so a
    /// missing request FAILS the test instead of hanging it.
    async fn recorded_request_line(rx: tokio::sync::mpsc::Receiver<Vec<u8>>) -> String {
        recorded_raw(rx).await.lines().next().expect("request line").to_string()
    }

    fn prefixed_node(port: u16, prefix: &str, model: Option<&str>) -> Arc<BackendNode> {
        Arc::new(BackendNode {
            url: format!("http://127.0.0.1:{port}"),
            model: model.map(str::to_string),
            strip_path_prefix: Some(prefix.to_string()),
            ..bare_node(None)
        })
    }

    fn completion_json_body() -> Vec<u8> {
        serde_json::json!({
            "id": "cmpl-1",
            "object": "chat.completion",
            "created": 1,
            "model": "test-model",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "hello"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}
        })
        .to_string()
        .into_bytes()
    }

    fn completion_response_bytes() -> Vec<u8> {
        raw_http_response("200 OK", &[("content-type", "application/json")], &completion_json_body())
    }

    async fn body_text(res: Response) -> String {
        let bytes = to_bytes(res.into_body(), 1024 * 1024).await.expect("body must be readable");
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[tokio::test]
    async fn prefixed_node_v1_messages_request_takes_anthropic_synthesis_path() {
        // RED [C-H4]: node mounted at "/completions", client speaks Anthropic.
        // The backend-native path is "/v1/messages", so this must take the Anthropic
        // synthesis path (message_start SSE) AND forward to the native path (no
        // double-strip). Baseline classifies it as OpenAI -> synthesized SSE carries
        // chat.completion.chunk frames and no message_start event.
        let (port, rx) = recording_backend(completion_response_bytes()).await;
        let node = prefixed_node(port, "/completions", None);
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let body = serde_json::json!({
            "model": "test-model",
            "max_tokens": 64,
            "messages": [{"role": "user", "content": "hi"}],
            "stream": true,
        });
        let res = handler
            .handle(request_with_body(
                Method::POST,
                "/completions/v1/messages",
                Body::from(body.to_string()),
            ))
            .await;

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
            Some("text/event-stream"),
            "client asked for a stream and must get synthesized SSE"
        );
        let text = body_text(res).await;
        assert!(
            text.contains("message_start"),
            "Anthropic synthesis must run for a /v1/messages node path, got: {text}"
        );
        assert!(
            !text.contains("chat.completion.chunk"),
            "OpenAI SSE frames must not leak into an Anthropic-classified route, got: {text}"
        );
        assert_eq!(
            recorded_request_line(rx).await,
            "POST /v1/messages HTTP/1.1",
            "forwarding must use the node-native path exactly once (no double-strip, no prefix left on)"
        );
    }

    #[tokio::test]
    async fn plain_chat_completions_routing_is_unchanged() {
        // Green before AND after: unprefixed node keeps the OpenAI synthesis path and
        // forwards the requested path verbatim.
        let (port, rx) = recording_backend(completion_response_bytes()).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());
        assert_eq!(handler.state.anthropic_buffered_responses_total.load(Ordering::Relaxed), 0);

        let res = handler.handle(completion_request_streaming()).await;

        assert_eq!(res.status(), StatusCode::OK);
        let text = body_text(res).await;
        assert!(
            text.contains("chat.completion.chunk"),
            "plain OpenAI route keeps OpenAI SSE, got: {text}"
        );
        assert!(
            !text.contains("message_start"),
            "Anthropic frames must not appear on the OpenAI route"
        );
        assert_eq!(recorded_request_line(rx).await, "POST /v1/chat/completions HTTP/1.1");
    }

    fn completion_request_streaming() -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "model": "test-model",
                    "messages": [{"role": "user", "content": "hello"}],
                    "stream": true,
                })
                .to_string(),
            ))
            .unwrap()
    }

    #[tokio::test]
    async fn v1_prefixed_node_still_completes_chat_completions() {
        // Regression pin (green before AND after): the documented Z.ai-style config
        // (strip_path_prefix "/v1") forwards the native "/chat/completions" and the
        // completion still comes back complete.
        let (port, rx) = recording_backend(completion_response_bytes()).await;
        let node = prefixed_node(port, "/v1", None);
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert_eq!(res.status(), StatusCode::OK);
        let text = body_text(res).await;
        assert!(
            text.contains("\"chat.completion\""),
            "completion body must reach the client, got: {text}"
        );
        assert_eq!(recorded_request_line(rx).await, "POST /chat/completions HTTP/1.1");
    }

    #[tokio::test]
    async fn prefixed_node_monitoring_arm_matches_on_the_routed_path() {
        // RED [C-H4]: GET /completions/props on a "/completions" node is the monitoring
        // route ("/props" natively). The monitoring arm must fire (verbatim passthrough),
        // not the completion pipeline. Distinguishing observable: the node's model
        // override rewrites the body ONLY on the completion pipeline; passthrough keeps
        // the client bytes.
        let body = br#"{"model":"client-name","keep":true}"#;
        let (port, rx) = recording_backend(raw_http_response(
            "200 OK",
            &[("content-type", "application/json")],
            br#"{"total_slots":1}"#,
        ))
        .await;
        let node = prefixed_node(port, "/completions", Some("renamed"));
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler
            .handle(request_with_body(Method::GET, "/completions/props", Body::from(&body[..])))
            .await;

        assert_eq!(res.status(), StatusCode::OK);
        let raw = recorded_raw(rx).await;
        assert_eq!(raw.lines().next().expect("request line"), "GET /props HTTP/1.1");
        assert!(
            raw.contains("\"model\":\"client-name\""),
            "monitoring arm must forward the client body VERBATIM, got: {raw}"
        );
        assert!(
            !raw.contains("renamed"),
            "model override must NOT touch a passthrough body, got: {raw}"
        );
    }

    #[tokio::test]
    async fn unicode_prefixed_node_classifies_anthropic_on_the_routed_path() {
        // Prefix-edge: a multibyte prefix must strip cleanly (byte-safe strip_prefix) and
        // the routed path must classify as Anthropic. Passthrough mode + dead backend:
        // the anthropic_buffered counter is the routing decision, taken BEFORE the
        // backend connect, so the 502 outcome is deterministic and format-independent.
        let node = Arc::new(BackendNode {
            url: "http://127.0.0.1:1".to_string(),
            strip_path_prefix: Some("/日本".to_string()),
            ..bare_node(None)
        });
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::Passthrough);

        let res = handler
            .handle(request_with_body(
                Method::POST,
                "/日本/v1/messages",
                Body::from(r#"{"model":"m","messages":[{"role":"user","content":"hi"}]}"#),
            ))
            .await;

        assert_eq!(res.status(), StatusCode::BAD_GATEWAY, "dead backend still answers 502");
        assert_eq!(
            handler.state.anthropic_buffered_responses_total.load(Ordering::Relaxed),
            1,
            "routed path /v1/messages must classify as Anthropic (buffered-under-passthrough counter)"
        );
    }

    #[tokio::test]
    async fn path_exactly_equal_to_prefix_is_not_anthropic_and_does_not_panic() {
        // Prefix-edge: request == prefix exactly -> routed == "". No arm may fire on "",
        // and "" must not be misclassified as Anthropic. Dead backend -> deterministic 502.
        let node = Arc::new(BackendNode {
            url: "http://127.0.0.1:1".to_string(),
            strip_path_prefix: Some("/completions".to_string()),
            ..bare_node(None)
        });
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::Passthrough);

        let res = handler
            .handle(request_with_body(
                Method::POST,
                "/completions",
                Body::from(r#"{"model":"m","messages":[{"role":"user","content":"hi"}]}"#),
            ))
            .await;

        assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(handler.state.anthropic_buffered_responses_total.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn trailing_slash_prefix_is_deterministic_and_never_anthropic() {
        // Prefix-edge: strip_path_prefix "/completions/" leaves routed == "v1/messages"
        // (no leading slash) - a misconfigured prefix whose native destination is broken
        // regardless of routing. The handler must stay deterministic: not Anthropic-classified,
        // dead backend -> 502 envelope, no panic.
        let node = Arc::new(BackendNode {
            url: "http://127.0.0.1:1".to_string(),
            strip_path_prefix: Some("/completions/".to_string()),
            ..bare_node(None)
        });
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::Passthrough);

        let res = handler
            .handle(request_with_body(
                Method::POST,
                "/completions/v1/messages",
                Body::from(r#"{"model":"m","messages":[{"role":"user","content":"hi"}]}"#),
            ))
            .await;

        assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(handler.state.anthropic_buffered_responses_total.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn backend_5xx_marks_the_node_into_cooldown() {
        let port = one_shot_backend(raw_http_response(
            "502 Bad Gateway",
            &[("content-type", "application/json")],
            br#"{"error":{"type":"upstream","message":"backend exploded"}}"#,
        ))
        .await;
        let node = node_at_port(port);
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node.clone()]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert_eq!(
            res.status(),
            StatusCode::BAD_GATEWAY,
            "the backend status must still be forwarded"
        );
        assert!(
            node.in_cooldown(Instant::now()),
            "a backend 5xx must park the selected node in failure cooldown"
        );
    }

    #[tokio::test]
    async fn backend_429_marks_the_node_into_cooldown() {
        let port = one_shot_backend(raw_http_response(
            "429 Too Many Requests",
            &[("content-type", "application/json")],
            br#"{"error":{"type":"rate_limited","message":"backend at capacity"}}"#,
        ))
        .await;
        let node = node_at_port(port);
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node.clone()]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(
            node.in_cooldown(Instant::now()),
            "a backend 429 must cool the node down, the proxy's own limiter 429 is a different signal"
        );
    }

    #[tokio::test]
    async fn backend_connect_failure_marks_the_node_into_cooldown() {
        let node = node_at_port(1);
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node.clone()]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert_error_envelope(res, StatusCode::BAD_GATEWAY, "backend_connect_error").await;
        assert!(node.in_cooldown(Instant::now()), "a refused connect must cool the node down");
    }

    #[tokio::test]
    async fn backend_2xx_clears_the_failure_cooldown() {
        let port = one_shot_backend(raw_http_response(
            "200 OK",
            &[("content-type", "application/json")],
            br#"{"id":"ok","choices":[]}"#,
        ))
        .await;
        let node = node_at_port(port);
        node.mark_failed(Duration::from_secs(600));
        assert!(node.in_cooldown(Instant::now()), "precondition: node starts cooled");

        let balancer = Arc::new(RoundRobinBalancer::new(vec![node.clone()]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert!(
            res.status().is_success(),
            "the fake backend answers 200, got {}",
            res.status()
        );
        assert!(
            !node.in_cooldown(Instant::now()),
            "a 2xx proves the node alive and must end its cooldown"
        );
    }

    // ---- big-fix task 14: passthrough body injections are gated on completion shape [C-H5] ----
    //
    // `apply_backend_overrides_bytes` ran the completion-pipeline rewrites (stream:false,
    // stream_options strip/inject, model override, temperature override) on EVERY routed
    // JSON body. A POST to a non-completion passthrough route (/tokenize, /embedding,
    // llama.cpp /completions...) with a node-level model/temperature override configured
    // got those fields injected into a body that was never a chat completion. The fix
    // gates all four injections on looks_like_completion_request (completion-shaped
    // path OR non-empty `messages` array); a non-completion body must go forward
    // BYTE-IDENTICAL - byte comparison, not value comparison.

    /// Everything after the head/body separator of a raw HTTP/1.1 request.
    fn recorded_body(raw: &str) -> &[u8] {
        let bytes = raw.as_bytes();
        let sep = bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|p| p + 4)
            .expect("recorded request must carry a head/body separator");
        &bytes[sep..]
    }

    fn override_node(port: u16) -> Arc<BackendNode> {
        Arc::new(BackendNode {
            url: format!("http://127.0.0.1:{port}"),
            model: Some("renamed".to_string()),
            temperature: Some(0.1),
            ..bare_node(None)
        })
    }

    #[tokio::test]
    async fn non_completion_passthrough_route_forwards_body_byte_identical() {
        // RED [C-H5]: node carries model + temperature overrides; the client POSTs a
        // llama.cpp /tokenize body WITHOUT `messages`. Every routed body used to run
        // through the completion rewrites, so the baseline leaks "stream":false,
        // "temperature":0.1 and rewrites "model" to the node's override - and even
        // re-serialises, so not one byte (key order, spacing) survives.
        let client_body = br#"{"content":"hello world","specials":"a\nb\t\"c\""}"#;
        let (port, rx) = recording_backend(raw_http_response(
            "200 OK",
            &[("content-type", "application/json")],
            br#"[{"token":0,"toksize":1,"text":"hello"}]"#,
        ))
        .await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![override_node(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler
            .handle(request_with_body(Method::POST, "/tokenize", Body::from(&client_body[..])))
            .await;

        assert_eq!(res.status(), StatusCode::OK);
        let raw = recorded_raw(rx).await;
        assert_eq!(
            recorded_body(&raw),
            &client_body[..],
            "a non-completion passthrough body must reach the backend byte-identical, got: {raw}"
        );
    }

    #[tokio::test]
    async fn chat_completion_route_still_gets_backend_overrides() {
        // Green guard (passes before AND after): the completion pipeline must KEEP
        // applying the node's model/temperature overrides. Without this, the RED test
        // above could be "fixed" by deleting the injection code entirely.
        let (port, rx) = recording_backend(completion_response_bytes()).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![override_node(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;
        assert_eq!(res.status(), StatusCode::OK);

        let raw = recorded_raw(rx).await;
        let forwarded: serde_json::Value =
            serde_json::from_slice(recorded_body(&raw)).expect("chat body stays JSON, got: {raw}");
        assert_eq!(
            forwarded["model"],
            serde_json::json!("renamed"),
            "model override must ride the chat body, got: {raw}"
        );
        assert_eq!(
            forwarded["temperature"],
            serde_json::json!(0.1),
            "temperature override must ride the chat body, got: {raw}"
        );
        assert_eq!(
            forwarded["stream"],
            serde_json::json!(false),
            "fake mode still forces stream:false, got: {raw}"
        );
    }

    #[test]
    fn completion_shape_predicate_pins_path_and_messages_edges() {
        let shaped = |body: &serde_json::Value, path: &str| ProxyHandler::looks_like_completion_request(body, path);
        let with_msgs = serde_json::json!({"model": "m", "messages": [{"role": "user", "content": "hi"}]});
        let one_empty_msg = serde_json::json!({"model": "m", "messages": [{}]});
        let empty_msgs = serde_json::json!({"model": "m", "messages": []});
        let no_msgs = serde_json::json!({"content": "text to tokenize"});

        assert!(
            shaped(&no_msgs, "/v1/chat/completions"),
            "chat path alone is completion-shaped"
        );
        assert!(shaped(&no_msgs, "/v1/messages"), "anthropic path alone is completion-shaped");
        assert!(
            shaped(&no_msgs, "/completions"),
            "llama.cpp native completions is completion-shaped"
        );
        assert!(
            shaped(&with_msgs, "/tokenize"),
            "messages on a plain route is completion-shaped via the OR clause"
        );
        assert!(
            shaped(&one_empty_msg, "/tokenize"),
            "messages:[{{}}] (one empty object) is a NON-EMPTY array -> completion-shaped (pinned edge)"
        );
        assert!(
            !shaped(&empty_msgs, "/tokenize"),
            "messages:[] is empty -> NOT completion-shaped unless the path says so (pinned edge)"
        );
        assert!(
            shaped(&empty_msgs, "/v1/chat/completions"),
            "path wins over an empty messages array"
        );
        assert!(!shaped(&no_msgs, "/tokenize"), "bare tokenize body is not completion-shaped");
    }

    #[test]
    fn non_completion_route_body_is_forwarded_byte_identical_at_the_gate() {
        let node = BackendNode {
            model: Some("renamed".to_string()),
            temperature: Some(0.1),
            ..bare_node(None)
        };
        // Key order + escapes deliberately un-canonical: a serde round-trip re-orders and
        // re-serialises, so this fails unless the bytes are returned UNTOUCHED.
        let client_body = br#"{"zz":"last","content":"a\nb\t\"c\"","nested":{"k":[1,2,3]}}"#;
        for allow_stream in [true, false] {
            for route in ["/tokenize", "/embedding", "/descriptions", "/lora-adapters"] {
                let (bytes, sent_stream) = ProxyHandler::apply_backend_overrides_bytes(client_body, &node, allow_stream, route);
                assert_eq!(bytes, &client_body[..], "{route} body must not be rewritten");
                assert!(!sent_stream, "a body that was not rewritten never reports stream:true");
            }
        }
    }

    #[test]
    fn completion_shaped_body_on_plain_route_still_gets_injections() {
        // The OR clause: a non-empty `messages` array opens the gate even on a path the
        // predicate does not recognise, keeping the completion pipeline intact.
        let node = BackendNode {
            model: Some("renamed".to_string()),
            temperature: Some(0.1),
            ..bare_node(None)
        };
        let body = serde_json::json!({"model": "client-name", "messages": [{"role": "user", "content": "hi"}]});
        let (bytes, _) = ProxyHandler::apply_backend_overrides_bytes(body.to_string().as_bytes(), &node, false, "/oddsuffix");
        let out: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(out["model"], serde_json::json!("renamed"));
        assert_eq!(out["temperature"], serde_json::json!(0.1));
        assert_eq!(out["stream"], serde_json::json!(false));
    }

    #[test]
    fn gate_evaluates_the_routed_path_consistent_with_task13_routing() {
        // Node mounted at "/completions": the client's "/completions/tokenize" is the
        // node-native "/tokenize" - NOT completion-shaped, even though the raw path
        // starts with "/completions". Task 13 routes on the routed path; the gate must
        // agree, or the two views disagree exactly where strip_path_prefix is set.
        let node = BackendNode {
            url: "http://127.0.0.1:8080".to_string(),
            model: Some("renamed".to_string()),
            temperature: Some(0.1),
            strip_path_prefix: Some("/completions".to_string()),
            ..bare_node(None)
        };
        let client_body = br#"{"content":"hello world"}"#;
        let (bytes, sent_stream) =
            ProxyHandler::apply_backend_overrides_bytes(client_body, &node, false, "/completions/tokenize");
        assert_eq!(bytes, &client_body[..], "routed /tokenize must forward byte-identical");
        assert!(!sent_stream);

        // Same mount, native chat path: gate opens on the routed view.
        let chat = serde_json::json!({"model": "client-name"});
        let (bytes, _) = ProxyHandler::apply_backend_overrides_bytes(
            chat.to_string().as_bytes(),
            &node,
            false,
            "/completions/v1/chat/completions",
        );
        let out: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            out["model"],
            serde_json::json!("renamed"),
            "routed chat path keeps injections"
        );
    }

    // ---- big-fix task 7: Content-Encoding stripped only from bodies the proxy decoded [C-H2] ----
    //
    // reqwest carries the gzip feature (Cargo.toml), so a gzip-encoded backend answer is
    // decoded by reqwest itself and its header stripped upstream of the proxy - that
    // scenario does NOT exercise this fix (verified: it passed at baseline, see
    // .omo/evidence/big-fix/task7.txt). The proxy's OWN decoders run for br/zstd/deflate
    // answers, and THAT is where the baseline leaked `content-encoding: br` next to an
    // already-decoded body.

    fn brotli_bytes(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        brotli::CompressorReader::new(data, 4096, 5, 22)
            .read_to_end(&mut out)
            .unwrap();
        out
    }

    #[tokio::test]
    async fn decoded_body_arrives_without_content_encoding_header() {
        let compressed = brotli_bytes(&completion_json_body());
        let len = compressed.len().to_string();
        let response = raw_http_response(
            "200 OK",
            &[
                ("content-type", "application/json"),
                ("content-encoding", "br"),
                ("content-length", len.as_str()),
            ],
            &compressed,
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        let dump = res
            .headers()
            .iter()
            .map(|(k, v)| format!("{k}: {}", v.to_str().unwrap_or("<binary>")))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            res.headers().get(header::CONTENT_ENCODING).is_none(),
            "proxy decoded the body, so Content-Encoding must not reach the client.\nDUMP:\n{dump}"
        );
        let text = body_text(res).await;
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("decoded body must parse");
        assert_eq!(parsed["id"], serde_json::json!("cmpl-1"));
    }

    #[tokio::test]
    async fn passthrough_endpoint_decoded_body_loses_content_encoding_header() {
        // The monitoring pass-through arm decodes with the same helper and had the same
        // leak at its own header-copy site.
        let plain = br#"{"total_slots":1}"#;
        let compressed = brotli_bytes(plain);
        let len = compressed.len().to_string();
        let response = raw_http_response(
            "200 OK",
            &[
                ("content-type", "application/json"),
                ("content-encoding", "br"),
                ("content-length", len.as_str()),
            ],
            &compressed,
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler
            .handle(request_with_body(Method::GET, "/v1/health", Body::empty()))
            .await;

        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers().get(header::CONTENT_ENCODING).is_none(),
            "decoded passthrough body must not keep Content-Encoding"
        );
        assert_eq!(body_text(res).await, String::from_utf8_lossy(plain));
    }

    #[tokio::test]
    async fn gzip_answered_backend_reaches_client_decoded_without_content_encoding() {
        // Spec acceptance: gzip fake -> no content-encoding header out, body parses.
        // reqwest's own gzip decoding already satisfies this at baseline; pinned so a
        // future reqwest-feature change that hands gzip to decompress_body still lands
        // on the client-visible outcome.
        use flate2::write::GzEncoder;
        use std::io::Write;
        let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&completion_json_body()).unwrap();
        let gz = enc.finish().unwrap();
        let len = gz.len().to_string();
        let response = raw_http_response(
            "200 OK",
            &[
                ("content-type", "application/json"),
                ("content-encoding", "gzip"),
                ("content-length", len.as_str()),
            ],
            &gz,
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers().get(header::CONTENT_ENCODING).is_none(),
            "decoded body must not be answered with a stale Content-Encoding"
        );
        let text = body_text(res).await;
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("body must parse");
        assert_eq!(parsed["id"], serde_json::json!("cmpl-1"));
    }

    #[tokio::test]
    async fn zero_byte_decoded_body_still_counts_as_decoded() {
        // An encoded stream that decodes to ZERO bytes is a body the proxy decoded,
        // not a passthrough: the header must go even though the body is empty.
        let compressed = brotli_bytes(b"");
        let len = compressed.len().to_string();
        let response = raw_http_response(
            "200 OK",
            &[
                ("content-type", "application/json"),
                ("content-encoding", "br"),
                ("content-length", len.as_str()),
            ],
            &compressed,
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler
            .handle(request_with_body(Method::GET, "/v1/health", Body::empty()))
            .await;

        assert!(
            res.headers().get(header::CONTENT_ENCODING).is_none(),
            "Decoded(empty) must drop the header, not Passthrough it"
        );
        assert_eq!(body_text(res).await, "");
    }

    #[test]
    fn empty_gzip_stream_decodes_to_empty_decoded_not_passthrough() {
        use flate2::write::GzEncoder;
        let empty_gz = GzEncoder::new(Vec::new(), flate2::Compression::default())
            .finish()
            .expect("empty gzip stream must build");
        match decode_body_bytes(&empty_gz, Some("gzip")) {
            Ok(Decompressed::Decoded(b)) => assert!(b.is_empty(), "decoded bytes must be exactly empty"),
            Ok(Decompressed::Passthrough(b)) => {
                panic!(
                    "empty gzip stream must decode to Decoded(empty), got Passthrough({} bytes)",
                    b.len()
                )
            }
            Err(e) => panic!("empty gzip stream is valid gzip, got Err({e})"),
        }
    }

    #[tokio::test]
    async fn unknown_content_encoding_token_is_forwarded_with_header_and_body() {
        // Pinned CURRENT behavior: an encoding token the proxy cannot decode is handed
        // on unchanged WITH its header (Passthrough). The client decides what to do
        // with it; the proxy never silently rewrites what it did not transform.
        let plain = br#"[{"token":0}]"#;
        let response = raw_http_response(
            "200 OK",
            &[("content-type", "application/json"), ("content-encoding", "banana")],
            plain,
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler
            .handle(request_with_body(Method::GET, "/v1/health", Body::empty()))
            .await;

        assert_eq!(
            res.headers().get(header::CONTENT_ENCODING).and_then(|v| v.to_str().ok()),
            Some("banana"),
            "an untransformed body must keep its declared encoding"
        );
        assert_eq!(body_text(res).await, String::from_utf8_lossy(plain));
    }

    #[tokio::test]
    async fn identity_content_encoding_is_forwarded_unchanged() {
        let plain = br#"[{"token":1}]"#;
        let response = raw_http_response(
            "200 OK",
            &[("content-type", "application/json"), ("content-encoding", "identity")],
            plain,
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler
            .handle(request_with_body(Method::GET, "/v1/health", Body::empty()))
            .await;

        assert_eq!(
            res.headers().get(header::CONTENT_ENCODING).and_then(|v| v.to_str().ok()),
            Some("identity"),
            "identity declares an untouched body - keep the header"
        );
        assert_eq!(body_text(res).await, String::from_utf8_lossy(plain));
    }

    #[test]
    fn no_encoding_header_yields_passthrough_of_identical_bytes() {
        let raw = b"unchanged body bytes";
        match decode_body_bytes(raw, None) {
            Ok(Decompressed::Passthrough(b)) => assert_eq!(b, raw),
            Ok(Decompressed::Decoded(_)) => panic!("no encoding means nothing was decoded"),
            Err(e) => panic!("no encoding cannot fail, got Err({e})"),
        }
    }

    // ---- big-fix task 8: the `deflate` token must accept zlib AND raw-deflate [C-M4] ----
    //
    // RFC 9110 defines Content-Encoding: deflate as ZLIB-format data, but senders mean
    // raw deflate. The baseline arm carried ONLY the raw decoder: a zlib fixture failed
    // with "corrupt deflate stream" (RED raw in .omo/evidence/big-fix/task8.txt).
    // Chain-level tests run against the sync core so they survive the task-10 offload.

    fn zlib_fixture(payload: &[u8]) -> Vec<u8> {
        use flate2::read::ZlibEncoder;
        let mut out = Vec::new();
        ZlibEncoder::new(payload, flate2::Compression::default())
            .read_to_end(&mut out)
            .unwrap();
        out
    }

    fn raw_deflate_fixture(payload: &[u8]) -> Vec<u8> {
        use flate2::read::DeflateEncoder;
        let mut out = Vec::new();
        DeflateEncoder::new(payload, flate2::Compression::default())
            .read_to_end(&mut out)
            .unwrap();
        out
    }

    fn decoded_or_panic(bytes: &[u8], encoding: &str) -> Vec<u8> {
        match decode_body_bytes(bytes, Some(encoding)) {
            Ok(Decompressed::Decoded(b)) => b,
            Ok(Decompressed::Passthrough(b)) => {
                panic!("{encoding} input must decode, got Passthrough({} bytes)", b.len())
            }
            Err(e) => panic!("{encoding} input must decode, got Err({e})"),
        }
    }

    #[test]
    fn deflate_alias_decodes_zlib_wrapped_body() {
        let payload = completion_json_body();
        assert_eq!(decoded_or_panic(&zlib_fixture(&payload), "deflate"), payload);
    }

    #[test]
    fn deflate_alias_decodes_raw_deflate_body() {
        let payload = completion_json_body();
        assert_eq!(decoded_or_panic(&raw_deflate_fixture(&payload), "deflate"), payload);
    }

    #[test]
    fn deflate_alias_error_names_both_attempts_for_unusable_bytes() {
        let junk = b"neither zlib nor raw deflate, definitely not deflate";
        match decode_body_bytes(junk, Some("deflate")) {
            Err(DecompressError::Decode(e)) => {
                assert!(
                    e.contains("zlib:") && e.contains("raw-deflate:"),
                    "both attempts must be reported, got: {e}"
                );
            }
            Err(DecompressError::PayloadTooLarge { .. }) => panic!("junk cannot be over the cap"),
            Ok(other) => panic!(
                "junk must not decode, got a {}-byte body",
                match other {
                    Decompressed::Decoded(b) | Decompressed::Passthrough(b) => b.len(),
                }
            ),
        }
    }

    #[test]
    fn gzip_branch_is_untouched_by_the_zlib_first_ordering() {
        use flate2::write::GzEncoder;
        use std::io::Write;
        let payload = br#"{"gzip":"still gzip","digits":[1,2,3,4,5,6,7,8,9,0]}"#;
        let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(payload).unwrap();
        let gz = enc.finish().unwrap();
        assert_eq!(decoded_or_panic(&gz, "gzip"), payload);
    }

    #[tokio::test]
    async fn deflate_zlib_backend_body_is_served_decoded_without_header() {
        // End-to-end through the completion path: reqwest does not decode deflate, so
        // the proxy's own alias runs. Baseline outcome was a 502 backend_decompress_error.
        let plain = completion_json_body();
        let compressed = zlib_fixture(&plain);
        let len = compressed.len().to_string();
        let response = raw_http_response(
            "200 OK",
            &[
                ("content-type", "application/json"),
                ("content-encoding", "deflate"),
                ("content-length", len.as_str()),
            ],
            &compressed,
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert_eq!(res.status(), StatusCode::OK, "zlib-format deflate must not 502");
        assert!(res.headers().get(header::CONTENT_ENCODING).is_none());
        let parsed: serde_json::Value = serde_json::from_str(&body_text(res).await).expect("body must parse");
        assert_eq!(parsed["id"], serde_json::json!("cmpl-1"));
    }

    #[tokio::test]
    async fn deflate_raw_backend_body_still_served_decoded_without_header() {
        // Green guard: the sender that meant raw deflate keeps working after the reorder.
        let plain = completion_json_body();
        let compressed = raw_deflate_fixture(&plain);
        let len = compressed.len().to_string();
        let response = raw_http_response(
            "200 OK",
            &[
                ("content-type", "application/json"),
                ("content-encoding", "deflate"),
                ("content-length", len.as_str()),
            ],
            &compressed,
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(header::CONTENT_ENCODING).is_none());
        let parsed: serde_json::Value = serde_json::from_str(&body_text(res).await).expect("body must parse");
        assert_eq!(parsed["id"], serde_json::json!("cmpl-1"));
    }

    // ---- big-fix task 9: one forward_response, one hop-by-hop predicate [C-H3] ----
    //
    // The RED evidence is structural (grep BEFORE = 2 copy loops / 2 skip predicates,
    // quoted in .omo/evidence/big-fix/task9.txt) plus the baseline header dump below:
    // the old loops forwarded ALL SIX hop-by-hop headers to the client.

    #[tokio::test]
    async fn hop_by_hop_headers_do_not_reach_the_client() {
        let plain = br#"{"total_slots":1}"#;
        let response = raw_http_response(
            "200 OK",
            &[
                ("content-type", "application/json"),
                ("upgrade", "websocket"),
                ("te", "trailers"),
                ("trailer", "X-Next"),
                ("proxy-authenticate", "Basic realm=\"fake\""),
                ("proxy-authorization", "Basic whatever"),
                ("x-backend-marker", "keepme"),
            ],
            plain,
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler
            .handle(request_with_body(Method::GET, "/v1/health", Body::empty()))
            .await;

        for h in [
            "connection",
            "upgrade",
            "te",
            "trailer",
            "proxy-authenticate",
            "proxy-authorization",
        ] {
            let name = header::HeaderName::from_lowercase(h.as_bytes()).unwrap();
            assert!(
                res.headers().get(&name).is_none(),
                "hop-by-hop `{h}` must not be forwarded, dump: {:?}",
                res.headers()
            );
        }
        assert_eq!(
            res.headers().get("x-backend-marker").and_then(|v| v.to_str().ok()),
            Some("keepme"),
            "end-to-end headers must survive the filter"
        );
        assert_eq!(body_text(res).await, String::from_utf8_lossy(plain));
    }

    #[tokio::test]
    async fn hop_by_hop_filter_applies_on_the_buffered_completion_path_too() {
        // The extracted forwarder must govern BOTH former copy sites; a filter only on
        // the pass-through arm would let the skip sets drift back apart.
        let backend_body = serde_json::json!({"id": "x", "choices": []}).to_string();
        let response = raw_http_response(
            "200 OK",
            &[
                ("content-type", "application/json"),
                ("connection", "keep-alive"),
                ("upgrade", "h2c"),
            ],
            backend_body.as_bytes(),
        );
        let port = one_shot_backend(response).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(completion_request()).await;

        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(header::CONNECTION).is_none());
        assert!(res.headers().get(header::UPGRADE).is_none());
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
    }

    #[test]
    fn hop_by_hop_predicate_pins_the_rfc_9110_set() {
        let in_set = [
            header::CONNECTION,
            header::PROXY_AUTHENTICATE,
            header::PROXY_AUTHORIZATION,
            header::TE,
            header::TRAILER,
            header::UPGRADE,
        ];
        for name in &in_set {
            assert!(is_hop_by_hop_header(name), "{name} is hop-by-hop per RFC 9110 §7.6.1");
        }
        // The two body-framing headers are dropped separately (Axum recomputes them),
        // and Content-Encoding is body-transform state, not connection state.
        for name in [
            header::CONTENT_LENGTH,
            header::TRANSFER_ENCODING,
            header::CONTENT_ENCODING,
            header::CONTENT_TYPE,
        ] {
            assert!(
                !is_hop_by_hop_header(&name),
                "{name} must not live in the hop-by-hop predicate"
            );
        }
    }

    // ---- big-fix task 10: decompression cap + blocking-pool offload ----

    fn compress_with(codec: &str, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        match codec {
            "gzip" => {
                use flate2::read::GzEncoder;
                GzEncoder::new(data, flate2::Compression::default())
                    .read_to_end(&mut out)
                    .unwrap();
            }
            "deflate" => {
                use flate2::read::ZlibEncoder;
                ZlibEncoder::new(data, flate2::Compression::default())
                    .read_to_end(&mut out)
                    .unwrap();
            }
            "br" => {
                brotli::CompressorReader::new(data, 4096, 5, 22)
                    .read_to_end(&mut out)
                    .unwrap();
            }
            "zstd" => out = zstd::encode_all(data, 9).unwrap(),
            other => unreachable!("no codec fixture for {other}"),
        }
        out
    }

    fn bomb_payload(codec: &str) -> Vec<u8> {
        // Zip-bomb shape: the fixture is ~100 KB compressed, the decode demands 70 MiB.
        let big = vec![b'x'; 70 * 1024 * 1024];
        compress_with(codec, &big)
    }

    #[test]
    fn every_codec_refuses_payloads_expanding_over_the_cap() {
        for codec in ["gzip", "deflate", "br", "zstd"] {
            let bomb = bomb_payload(codec);
            assert!(bomb.len() < 1024 * 1024, "{codec} bomb fixture must stay small");
            match decode_body_bytes(&bomb, Some(codec)) {
                Err(DecompressError::PayloadTooLarge { limit_bytes }) => {
                    assert_eq!(limit_bytes, MAX_DECOMPRESSED_BYTES, "{codec} cap value");
                }
                Ok(Decompressed::Decoded(b)) | Ok(Decompressed::Passthrough(b)) => {
                    panic!("{codec}: {} decoded bytes slipped past the cap", b.len());
                }
                Err(DecompressError::Decode(e)) => {
                    panic!("{codec}: over-cap payload must be PayloadTooLarge, got Decode({e})");
                }
            }
        }
    }

    #[test]
    fn under_cap_payloads_still_decode_byte_intact() {
        let payload = completion_json_body();
        for codec in ["gzip", "deflate", "br", "zstd"] {
            let fixture = compress_with(codec, &payload);
            assert_eq!(decoded_or_panic(&fixture, codec), payload, "{codec} under cap");
        }
    }

    #[test]
    fn bodies_that_are_never_decoded_are_not_capped() {
        let big = vec![b'q'; MAX_DECOMPRESSED_BYTES + 1];
        match decode_body_bytes(&big, Some("banana")) {
            Ok(Decompressed::Passthrough(b)) => assert_eq!(b.len(), big.len()),
            other => panic!("unsupported encoding must pass through, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn brotli_bomb_expanding_over_cap_answers_413_backend_payload_too_large() {
        // Brotli, not gzip, for the end-to-end fixture: reqwest's gzip feature
        // transparently decodes gzip backends upstream of the proxy (task-7
        // finding), so only br reliably reaches the proxy's own decoder — the code
        // path the cap guards. gzip/zstd/deflate get decoder-level coverage above.
        let bomb = bomb_payload("br");
        let len = bomb.len().to_string();
        let port = one_shot_backend(raw_http_response(
            "200 OK",
            &[
                ("content-type", "application/json"),
                ("content-encoding", "br"),
                ("content-length", len.as_str()),
            ],
            &bomb,
        ))
        .await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let started = std::time::Instant::now();
        let res = handler.handle(completion_request()).await;
        let elapsed = started.elapsed();
        // hung_commands attestation: the refusal must be prompt — the decode stops at
        // cap+1 instead of expanding. Bound is generous: 20 s vs sub-second actual.
        println!("task10 e2e bomb: {} compressed bytes refused in {:?}", bomb.len(), elapsed);
        assert!(elapsed < std::time::Duration::from_secs(20), "bomb refusal took {elapsed:?}");

        let envelope = assert_error_envelope(res, StatusCode::PAYLOAD_TOO_LARGE, "backend_payload_too_large").await;
        assert!(
            envelope["error"]["message"].as_str().unwrap_or_default().contains("64 MiB"),
            "message should name the cap: {envelope}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn light_request_completes_while_heavy_decode_runs_off_reactor() {
        // Ordering, not timing, is the assertion. On a single-threaded runtime the
        // heavy request is spawned FIRST, so any inline decode — the baseline
        // behavior, captured red as ["heavy","light"] over a ~350 ms window — forces
        // heavy to finish first. Margins (documented, no sleeps): baseline gap
        // ~350 ms decode vs ~2 ms light path; post-fix the light path (~1-5 ms
        // including its own loopback round trip) races a spawn_blocking dispatch of
        // ~10 µs plus the heavy request's multi-second body streaming — three
        // orders of magnitude of headroom either way.
        let order: Arc<std::sync::Mutex<Vec<&'static str>>> = Arc::new(std::sync::Mutex::new(Vec::new()));

        let filler = "x".repeat(60 * 1024 * 1024);
        let heavy_json = serde_json::json!({
            "id": "chatcmpl-heavy",
            "object": "chat.completion",
            "created": 0,
            "model": "test-model",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": filler}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        })
        .to_string();
        let heavy_len = heavy_json.len();
        let heavy_payload = compress_with("br", heavy_json.as_bytes());
        let len = heavy_payload.len().to_string();
        let heavy_port = one_shot_backend(raw_http_response(
            "200 OK",
            &[
                ("content-type", "application/json"),
                ("content-encoding", "br"),
                ("content-length", len.as_str()),
            ],
            &heavy_payload,
        ))
        .await;
        let bal_h = Arc::new(RoundRobinBalancer::new(vec![node_at_port(heavy_port)]).unwrap());
        let h_h = handler_with_balancer(bal_h, None, StreamingMode::default());
        let o = order.clone();
        let heavy = tokio::spawn(async move {
            let res = h_h.handle(completion_request()).await;
            assert_eq!(res.status(), StatusCode::OK, "under-cap body must still succeed");
            let bytes = to_bytes(res.into_body(), 256 * 1024 * 1024)
                .await
                .expect("heavy body must be readable");
            assert_eq!(bytes.len(), heavy_len, "offload must not truncate the body");
            o.lock().unwrap().push("heavy");
        });

        let light_port = one_shot_backend(raw_http_response(
            "200 OK",
            &[("content-type", "application/json")],
            br#"{"ok":true}"#,
        ))
        .await;
        let bal_l = Arc::new(RoundRobinBalancer::new(vec![node_at_port(light_port)]).unwrap());
        let h_l = handler_with_balancer(bal_l, None, StreamingMode::default());
        let o = order.clone();
        let light = tokio::spawn(async move {
            let res = h_l.handle(request_with_body(Method::GET, "/v1/health", Body::empty())).await;
            assert_eq!(res.status(), StatusCode::OK);
            let _ = to_bytes(res.into_body(), 1024 * 1024).await.expect("light body");
            o.lock().unwrap().push("light");
        });

        let (h_res, l_res) = tokio::join!(heavy, light);
        h_res.expect("heavy task");
        l_res.expect("light task");
        let done = order.lock().unwrap().clone();
        assert_eq!(done, vec!["light", "heavy"], "light request must not queue behind the decode");
    }

    // ---- big-fix task 11: /v1/messages synthesis failure ends the stream with an
    //      Anthropic SSE error frame - never a 200 leaking the raw OpenAI body [C-L8] ----
    //
    // Baseline (fcec6c8): every failure arm inside the is_anthropic_api synthesis
    // block logged and "fell through to return JSON", so a stream:true Claude Code
    // request received HTTP 200 + application/json + the untranslated backend object.
    // The frame bytes asserted below are the post-fix wire contract, pinned literally.

    /// Backend body that translates to NEITHER API shape: it lacks Anthropic's
    /// required fields (id/type/role/model) and carries a null `choices[].index`,
    /// which `ChatCompletionResponse` rejects (`index: u32`). The null-index family
    /// is the real-world malformed-completion class this arm sees in production.
    fn untranslatable_backend_body() -> Vec<u8> {
        br#"{"id":"cmpl-junk","choices":[{"index":null,"message":{"role":"assistant","content":"leaky"}}]}"#.to_vec()
    }

    /// Anthropic streaming request as Claude Code frames it (content-block messages,
    /// max_tokens, stream:true).
    fn anthropic_stream_request() -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "model": "test-model",
                    "max_tokens": 64,
                    "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
                    "stream": true
                })
                .to_string(),
            ))
            .unwrap()
    }

    #[tokio::test]
    async fn messages_stream_synthesis_failure_serves_sse_error_frame_not_openai_body() {
        let backend_body = untranslatable_backend_body();
        let len = backend_body.len().to_string();
        let port = one_shot_backend(raw_http_response(
            "200 OK",
            &[("content-type", "application/json"), ("content-length", len.as_str())],
            &backend_body,
        ))
        .await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(anthropic_stream_request()).await;

        let status = res.status();
        let ct = res
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("<missing>")
            .to_string();
        let bytes = to_bytes(res.into_body(), 1024 * 1024).await.expect("body readable");
        let text = String::from_utf8_lossy(&bytes).into_owned();
        // Baseline capture lives IN this message: pre-fix it prints
        // status=200 content-type=application/json body=<the raw OpenAI object>.
        assert_eq!(
            ct, "text/event-stream",
            "failed synthesis must still be an SSE stream; got status={status} content-type={ct} body={text:?}"
        );

        // Exact wire bytes - a literal copy on purpose: the test pins the contract,
        // it does not echo a production constant. Valid SSE per the Anthropic SDK:
        // an `event: error` frame (which the SDK raises on), then the `[DONE]`
        // terminal data frame mandated by big-fix 11, each properly frame-terminated.
        const EXPECTED_FRAME: &str = concat!(
            "event: error\n",
            "data: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",",
            "\"message\":\"The proxy failed to convert the backend response into ",
            "an Anthropic streaming response\"}}\n\n",
            "data: [DONE]\n\n",
        );
        assert_eq!(text, EXPECTED_FRAME, "error frame bytes, got: {text:?}");

        // Machine-readable for the Anthropic SDK: the data payload parses, is typed
        // `error`, and carries a typed inner error with a non-empty message.
        let data_line = text
            .lines()
            .find_map(|l| l.strip_prefix("data: "))
            .expect("error frame must carry a data line");
        let payload: serde_json::Value = serde_json::from_str(data_line).expect("error data must be JSON");
        assert_eq!(payload["type"], serde_json::json!("error"));
        assert_eq!(payload["error"]["type"], serde_json::json!("api_error"));
        assert!(payload["error"]["message"].as_str().is_some_and(|m| !m.is_empty()));

        // No raw OpenAI body may leak into the stream.
        assert!(!text.contains("choices"), "raw OpenAI body leaked: {text:?}");
        assert!(!text.contains("leaky"), "backend content leaked: {text:?}");
    }

    #[tokio::test]
    async fn translatable_backend_still_gets_synthesized_anthropic_sse() {
        // Scope guard (green pre AND post): only the FAILURE arms gain the error
        // frame. A parseable OpenAI-format body must still synthesize the real
        // Anthropic stream (message_start event) with no error frame anywhere.
        let port = one_shot_backend(completion_response_bytes()).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let res = handler.handle(anthropic_stream_request()).await;

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
            Some("text/event-stream")
        );
        let text = body_text(res).await;
        assert!(
            text.contains("message_start"),
            "synthesized stream must open with message_start, got: {text}"
        );
        assert!(
            !text.contains("event: error"),
            "success path must not gain an error frame: {text}"
        );
    }

    #[tokio::test]
    async fn nonstreaming_messages_untranslatable_body_still_returns_json() {
        // Scope guard (green pre AND post): without stream:true the client asked for
        // a JSON document; the verbatim JSON return stays correct there.
        let backend_body = untranslatable_backend_body();
        let port = one_shot_backend(raw_http_response(
            "200 OK",
            &[("content-type", "application/json")],
            &backend_body,
        ))
        .await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());

        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "model": "test-model",
                    "max_tokens": 64,
                    "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
                })
                .to_string(),
            ))
            .unwrap();
        let res = handler.handle(req).await;

        assert_eq!(res.status(), StatusCode::OK);
        let text = body_text(res).await;
        assert!(text.contains("choices"), "JSON document path untouched: {text}");
    }

    // ---- big-fix task 12: the dump preserves the ORIGINAL wire bytes + Content-Type;
    //      when the proxy transformed the body it also writes a `.decoded` variant [C-M10] ----
    //
    // Baseline (d0f9baf): the buffered-path dump is fed `final_body` (decompressed,
    // fixed, re-serialized) - the backend's original compressed response never reaches
    // disk. reqwest decodes gzip backends upstream of the proxy (task-7 finding), so an
    // end-to-end fixture must use brotli to actually exercise the proxy's own decoder;
    // gzip keeps dump-function-level coverage in the unit test below.

    /// meta.txt is the LAST file dump_request_response writes, so its presence proves
    /// the whole dump landed. Bounded by 5 s against a spawned task writing three small
    /// files - orders of magnitude of margin, and a missing dump FAILS instead of
    /// hanging (task-15 recipe).
    async fn poll_dump_request_dir(root: &std::path::Path) -> std::path::PathBuf {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Ok(entries) = std::fs::read_dir(root) {
                    for e in entries.flatten() {
                        let p = e.path();
                        if p.is_dir() && p.join("meta.txt").exists() {
                            return p;
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("dump dir (with meta.txt) must appear within 5 s")
    }

    #[tokio::test]
    async fn brotli_backend_dump_holds_original_wire_bytes_and_decoded_variant() {
        let dir = tempfile::tempdir().expect("temp dump dir");
        let plain = completion_json_body();
        let br_body = brotli_bytes(&plain);
        let len = br_body.len().to_string();
        let port = one_shot_backend(raw_http_response(
            "200 OK",
            &[
                ("content-type", "application/json"),
                ("content-encoding", "br"),
                ("content-length", len.as_str()),
            ],
            &br_body,
        ))
        .await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let mut handler = handler_with_balancer(balancer, None, StreamingMode::default());
        handler.state.dump_path = Some(Arc::new(dir.path().to_path_buf()));

        let res = handler.handle(completion_request()).await;
        let _ = body_text(res).await;

        let req_dir = poll_dump_request_dir(dir.path()).await;
        let dumped = std::fs::read(req_dir.join("res.json")).expect("res.json must exist");
        assert_eq!(
            dumped, br_body,
            "res file must be byte-equal to the ORIGINAL pre-decompression bytes"
        );
        let variant = std::fs::read(req_dir.join("res.json.decoded")).expect(".decoded variant must exist");
        let parsed: serde_json::Value = serde_json::from_slice(&variant).expect("variant is the transformed JSON body");
        assert_eq!(parsed["id"], serde_json::json!("cmpl-1"));
        let meta = std::fs::read_to_string(req_dir.join("meta.txt")).expect("meta.txt must exist");
        assert!(
            meta.contains("Content-Type: Some(\"application/json\")"),
            "original Content-Type must be recorded: {meta}"
        );
        assert!(
            meta.contains(&format!("Body Size: {} bytes", br_body.len())),
            "Body Size must be the ORIGINAL size: {meta}"
        );
        assert!(meta.contains("Decoded Variant"), "variant must be recorded in meta: {meta}");
    }

    #[tokio::test]
    async fn dump_unit_gzip_originals_and_non200_are_preserved() {
        // Spec-literal acceptance at the dump-function level: gzip original byte-equal,
        // .decoded variant raw, non-2xx status combo. An e2e gzip fixture cannot reach
        // the dump as gzip (reqwest decodes upstream); this is the honest gzip coverage.
        let dir = tempfile::tempdir().expect("temp dump dir");
        let dump_path = Arc::new(dir.path().to_path_buf());
        let plain = completion_json_body();
        let gz = compress_with("gzip", &plain);
        dump::dump_request_response(
            &dump_path,
            "POST",
            "/v1/chat/completions",
            br#"{"model":"m","messages":[]}"#,
            Some("application/json"),
            500,
            &gz,
            Some("application/json; charset=utf-8"),
            Some(&plain),
        )
        .await
        .expect("dump must succeed");
        let req_dir = std::fs::read_dir(dir.path())
            .expect("dump root")
            .next()
            .expect("one request dir")
            .expect("dir entry")
            .path();
        assert_eq!(
            std::fs::read(req_dir.join("res.json")).expect("res.json must exist"),
            gz,
            "gzip original must be byte-equal on disk"
        );
        assert_eq!(
            std::fs::read(req_dir.join("res.json.decoded")).expect(".decoded variant must exist"),
            plain,
            "variant must be the raw transformed bytes"
        );
        let meta = std::fs::read_to_string(req_dir.join("meta.txt")).expect("meta.txt must exist");
        assert!(meta.contains("Status: 500"), "non-2xx status recorded: {meta}");
        assert!(meta.contains("charset=utf-8"), "original CT with charset: {meta}");
        assert!(meta.contains(&format!("Body Size: {} bytes", gz.len())), "{meta}");
    }

    #[tokio::test]
    async fn uncompressed_untouched_response_dumps_without_decoded_variant() {
        // Nothing decoded, nothing fixed (registry empty, stats off) and
        // completion_json_body() = json!().to_string() = compact BTreeMap order =
        // stable under a Value round-trip: exactly three files, legacy pretty JSON,
        // no variant.
        let dir = tempfile::tempdir().expect("temp dump dir");
        let port = one_shot_backend(completion_response_bytes()).await;
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(port)]).unwrap());
        let mut handler = handler_with_balancer(balancer, None, StreamingMode::default());
        handler.state.dump_path = Some(Arc::new(dir.path().to_path_buf()));

        let res = handler.handle(completion_request()).await;
        let _ = body_text(res).await;

        let req_dir = poll_dump_request_dir(dir.path()).await;
        let files: Vec<String> = std::fs::read_dir(&req_dir)
            .expect("req dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        assert!(files.contains(&"res.json".to_string()), "{files:?}");
        assert!(
            !files.iter().any(|f| f.ends_with(".decoded")),
            "an untouched body must not get a variant: {files:?}"
        );
        let dumped = std::fs::read_to_string(req_dir.join("res.json")).expect("res.json");
        let parsed: serde_json::Value = serde_json::from_str(&dumped).expect("pretty JSON");
        assert_eq!(parsed["id"], serde_json::json!("cmpl-1"));
    }

    // ---- big-fix task 16: the /health match arm in handle() is dead at the server
    //      level and gets deleted [C-L1] ----
    //
    // Dead-at-baseline proof, structural: server.rs registers
    //     .route("/health", get(health_handler))        // server.rs:203
    // AHEAD of the wildcard catch-alls
    //     .route("/*path", any(proxy_handler))          // server.rs:206
    // matchit binds a static segment before a wildcard capture, so EVERY request to
    // /health lands on the static node: GET answers health_handler's body, any other
    // method gets the router's own 405 (only get() was mounted there).
    // ProxyHandler::handle never observes "/health", so the arm
    //     | (&Method::GET, "/health")                   // fcec6c8 handler.rs:596
    // could not fire. The smoke test pins that shadowing behaviorally - green before
    // AND after the deletion (it is the shadow proof, not a red-then-green pin; the
    // deletion's correctness is the still-green suite). The route table below mirrors
    // server.rs run_server (server.rs is READ-ONLY for this task, so it is mirrored,
    // not imported); the "OK" body is pinned verbatim from health_handler,
    // server.rs:221-223.

    #[tokio::test]
    async fn server_route_table_shadows_health_and_keeps_v1_health_live() {
        // Ephemeral 127.0.0.1:0, in-process serve, abort teardown - zero fixed ports,
        // zero sleeps. The dead node at 127.0.0.1:1 answers connect instantly-refused.
        let balancer = Arc::new(RoundRobinBalancer::new(vec![node_at_port(1)]).unwrap());
        let handler = handler_with_balancer(balancer, None, StreamingMode::default());
        let state = handler.state.clone();

        async fn proxy_hop(
            axum::extract::State(st): axum::extract::State<ProxyState>,
            req: axum::extract::Request,
        ) -> axum::response::Response {
            ProxyHandler::new(st).handle(req).await
        }

        let app = axum::Router::new()
            .route("/health", axum::routing::get(|| async { "OK" }))
            .route("/v1/*path", axum::routing::any(proxy_hop))
            .route("/*path", axum::routing::any(proxy_hop))
            .fallback(proxy_hop)
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("smoke listener");
        let addr = listener.local_addr().expect("smoke addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("smoke server");
        });

        let client = reqwest::Client::new();
        let base = format!("http://{addr}");

        // GET /health: the SERVER body ("OK", server.rs:221-223). The proxy handler
        // behind this state could only ever answer 502 with an error envelope - any
        // shadowing failure would show up as exactly that.
        let health = client.get(format!("{base}/health")).send().await.expect("GET /health");
        assert_eq!(health.status(), StatusCode::OK);
        assert_eq!(health.text().await.expect("health body"), "OK");

        // POST /health: the router's own 405 (only get() is mounted on the static
        // node) - proof not even a non-GET method falls through to the proxy.
        let post = client.post(format!("{base}/health")).send().await.expect("POST /health");
        assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED);

        // GET /v1/health: the wildcard still delivers to ProxyHandler - the arm that
        // survives the deletion answers with the dead-backend 502 envelope.
        let v1 = client.get(format!("{base}/v1/health")).send().await.expect("GET /v1/health");
        assert_eq!(v1.status(), StatusCode::BAD_GATEWAY);
        let body: serde_json::Value = v1.json().await.expect("502 envelope JSON");
        assert_eq!(body["error"]["type"], serde_json::json!("backend_connect_error"));

        server.abort();
    }

    // --- task 68: fixes + metrics run on the MERGED body [C-L18, C-M1] ---

    struct CapturingExporter {
        samples: Arc<std::sync::Mutex<Vec<crate::stats::RequestMetrics>>>,
    }
    #[async_trait::async_trait]
    impl crate::exporters::MetricsExporter for CapturingExporter {
        async fn export(&self, metrics: &crate::stats::RequestMetrics) -> Result<(), crate::exporters::ExportError> {
            self.samples.lock().unwrap().push(metrics.clone());
            Ok(())
        }
        fn name(&self) -> &str {
            "capture"
        }
    }

    /// Queue backend where each entry is (response-delay-ms, body).
    async fn queue_backend(responses: Vec<(u64, serde_json::Value)>) -> String {
        use axum::{routing::post, Json, Router};
        let queue = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::from(responses)));
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move || {
                let queue = queue.clone();
                async move {
                    let (delay_ms, body) = queue
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or((0, serde_json::json!({"error": "queue exhausted"})));
                    if delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                    Json(body)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn reprompt_e2e_handler(url: &str, samples: Arc<std::sync::Mutex<Vec<crate::stats::RequestMetrics>>>) -> ProxyHandler {
        use crate::proxy::reprompt::RepromptEngine;
        let config = AppConfig {
            server: crate::config::ServerConfig {
                port: 8066,
                host: "0.0.0.0".to_string(),
                max_concurrent_requests: crate::config::default_max_concurrent(),
                allowed_origins: None,
            },
            backend: Some(BackendConfig::default()),
            backends: None,
            fixes: crate::config::FixesConfig {
                enabled: true,
                modules: HashMap::new(),
            },
            stats: crate::config::StatsConfig {
                enabled: true,
                format: crate::config::StatsFormat::Compact,
            },
            exporters: crate::config::ExportersConfig {
                influxdb: crate::config::InfluxDbConfig {
                    enabled: false,
                    url: "http://localhost:8086".to_string(),
                    org: "test".to_string(),
                    bucket: "test".to_string(),
                    token: "test".to_string(),
                    batch_size: 1,
                    flush_interval_seconds: 1,
                },
            },
            streaming: StreamingMode::Fake,
            synthesis: crate::config::SynthesisConfig::default(),
            augment_backend: None,
            reprompt: None,
            dump: crate::config::DumpConfig::default(),
        };
        let mut reg = FixRegistry::new();
        reg.register(Arc::new(crate::fixes::ToolCallNullIndexFix::new()));
        let mut mgr = ExporterManager::new();
        mgr.add(Arc::new(CapturingExporter { samples }));
        let rep_cfg = crate::config::RepromptConfig {
            enabled: true,
            prompt: Some("Continue.".into()),
            max_retries: 2,
            done_sentinels: vec!["DONE_NO_MORE".into()],
            ..Default::default()
        };
        let node = BackendNode {
            url: url.to_string(),
            ..bare_node(None)
        };
        let lb = Arc::new(RoundRobinBalancer::new(vec![Arc::new(node)]).unwrap());
        ProxyHandler::new(ProxyState {
            config: Arc::new(config),
            load_balancer: lb,
            fix_registry: Arc::new(reg),
            exporter_manager: Arc::new(mgr),
            augment_backend: None,
            reprompt_engine: Some(Arc::new(RepromptEngine::from_config(&rep_cfg).expect("engine"))),
            hide_requests: false,
            log_augmented_request_text: false,
            dump_path: None,
            concurrent_requests: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            backend_streaming_fallback_hits: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            openai_stream_passthrough_total: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            backend_nonsse_when_streamed_for: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            anthropic_buffered_responses_total: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            anthropic_buffered_notice_once: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            rejected_requests: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            concurrent_semaphore: Some(Arc::new(tokio::sync::Semaphore::new(100))),
        })
    }

    #[tokio::test]
    async fn test_reprompt_metrics_and_fixes_run_on_merged_body() {
        let premature = serde_json::json!({"id":"a","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"stopped early"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}});
        let merged = serde_json::json!({"id":"b","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"now the tool call","tool_calls":[{"id":"c1","type":"function","function":{"name":"write","arguments":"{\"filePath\":\"/x\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":77,"total_tokens":87}});
        let url = queue_backend(vec![(0, premature), (300, merged)]).await;
        let samples = Arc::new(std::sync::Mutex::new(Vec::new()));
        let handler = reprompt_e2e_handler(&url, samples.clone());
        let body = serde_json::json!({"model":"m","messages":[{"role":"user","content":"do it"}],"tools":[{"type":"function","function":{"name":"write","parameters":{"type":"object"}}}]});
        let res = handler
            .handle(request_with_body(
                Method::POST,
                "/v1/chat/completions",
                Body::from(body.to_string()),
            ))
            .await;
        let out = axum::body::to_bytes(res.into_body(), 10_000).await.unwrap();
        let out: serde_json::Value = serde_json::from_slice(&out).unwrap();

        let mut exported = Vec::new();
        for _ in 0..40 {
            exported = samples.lock().unwrap().clone();
            if !exported.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(exported.len(), 1, "the ONE log/export line");
        assert_eq!(exported[0].finish_reason, "tool_calls", "metrics reflect the merged body");
        assert_eq!(exported[0].completion_tokens, Some(77), "merged completion_tokens");
        assert!(
            exported[0].duration_ms >= 250.0,
            "duration spans the reprompt rounds: {}",
            exported[0].duration_ms
        );
        assert_eq!(
            out["choices"][0]["message"]["tool_calls"][0]["index"].as_u64(),
            Some(0),
            "fixes run on the merged body"
        );
    }
}
