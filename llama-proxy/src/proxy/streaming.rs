//! Streaming response handling (SSE)

use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Instant;

use crate::backends::BackendGuard;
use crate::config::StatsFormat;
use crate::exporters::ExporterManager;
use crate::fixes::FixRegistry;
use crate::proxy::fetch_context_total;
use crate::stats::RequestMetrics;
use axum::http::header;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::task::Poll;

/// Global count of SSE events (blank-line-terminated) framed on pass-through streams
pub static SSE_EVENTS_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Global count of streamed events whose `data:` payload failed to parse as JSON
pub static SSE_UNPARSED_EVENTS_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Global count of pass-through streams that ended without [DONE]/message_stop
pub static STREAM_TRUNCATED_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Global count of pass-through streams the stats observer gave up on after the
/// inactivity timeout. The client transfer itself is not cut by it.
pub static STREAM_STALLED_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Global count of fix detections that were reported but NOT repaired (pass-through)
pub static FIX_UNREPAIRED_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Global count of pass-through responses carrying a non-identity Content-Encoding
pub static STREAM_COMPRESSED_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Global count of pass-through streams the client disconnected from
pub static STREAM_CLIENT_GONE_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Global count of pass-through SSE responses framed by the proxy: the
/// denominator for every other passthrough ratio.
pub static PASSTHROUGH_STREAMS_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Maximum bytes the framer will hold without seeing an event terminator.
/// A backend that never emits a blank line (or terminates events with byte
/// sequences we don't frame on) would otherwise stall the client forever and
/// grow pending without bound. On breach we release pending verbatim.
const PENDING_HIGH_WATER: usize = 1024 * 1024;

/// Byte-framed SSE: splits a raw byte stream into complete
/// `\n\n`-terminated events, holding an unterminated tail until it completes.
/// Operates on raw bytes; never decodes, reparses, or reserializes on the hot
/// path. Line endings are normalized to LF before the scan ([D-M1]: the SSE
/// spec allows LF, CRLF, or lone CR as line terminators, and a `\r\n\r\n`
/// blank line must dispatch an event exactly like `\n\n`); apart from that
/// normalization events reach the client as the backend sent them.
struct FramingState {
    pending: Vec<u8>,
    // Unscanned frontier: bytes before it were already scanned for
    // terminators, except the very last one (a terminator's two \n may
    // straddle a chunk boundary). Keeps total scanning O(total bytes).
    scan_from: usize,
    // A CR ended the last push: whether it was CRLF (one LF) or a lone CR
    // line terminator (also one LF) needs the next push's first byte.
    cr_held: bool,
}

enum Framed {
    /// All complete events to date, verbatim (each including its blank line)
    Events(Vec<u8>),
    /// High-water breached with no terminator in sight: these bytes are
    /// released verbatim so the client is not stalled and pending is bounded.
    /// Carrying the bytes here (rather than asking the caller to call
    /// `finish()`) makes a forgotten drain unreachable.
    ForceFlush(Vec<u8>),
    Nothing,
}

impl FramingState {
    fn new() -> Self {
        Self {
            pending: Vec::new(),
            scan_from: 0,
            cr_held: false,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        if self.cr_held {
            self.cr_held = false;
            // The held CR is one line break either way; an LF starting this
            // push is that break's tail and is swallowed by it.
            self.pending.push(b'\n');
            if rest.first() == Some(&b'\n') {
                rest = &rest[1..];
            }
        }
        // Normalize before the pending scan: CRLF and lone CR (both legal SSE
        // line terminators) each become exactly one LF; a CR as the very last
        // byte stays undecided until the next push.
        let mut cursor = 0;
        while let Some(rel) = rest[cursor..].iter().position(|&b| b == b'\r') {
            let idx = cursor + rel;
            self.pending.extend_from_slice(&rest[cursor..idx]);
            if idx + 1 < rest.len() {
                self.pending.push(b'\n');
                cursor = if rest[idx + 1] == b'\n' { idx + 2 } else { idx + 1 };
            } else {
                self.cr_held = true;
                cursor = idx + 1;
            }
        }
        self.pending.extend_from_slice(&rest[cursor..]);
    }

    /// Release all complete events as one byte block, or signal a forced flush.
    fn take_framed(&mut self) -> Framed {
        if self.pending.is_empty() {
            return Framed::Nothing;
        }
        debug_assert!(
            find_double_newline(&self.pending[..(self.scan_from + 1).min(self.pending.len())]).is_none(),
            "scan_from={} skips a terminator",
            self.scan_from
        );
        let start = self.scan_from.min(self.pending.len().saturating_sub(1));
        let mut last: Option<usize> = None;
        let mut cursor = start;
        while let Some(rel) = find_double_newline(&self.pending[cursor..]) {
            last = Some(cursor + rel);
            cursor = (cursor + rel + 2).min(self.pending.len());
        }
        if let Some(p) = last {
            let end = p + 2;
            let out: Vec<u8> = self.pending.drain(..end).collect();
            self.scan_from = self.pending.len().saturating_sub(1);
            Framed::Events(out)
        } else {
            self.scan_from = self.pending.len().saturating_sub(1);
            if self.pending.len() > PENDING_HIGH_WATER {
                tracing::warn!(
                    pending_bytes = self.pending.len(),
                    "SSE framer exceeded high-water mark without an event terminator; flushing verbatim"
                );
                let out = {
                    if self.cr_held {
                        self.cr_held = false;
                        self.pending.push(b'\n');
                    }
                    std::mem::take(&mut self.pending)
                };
                self.scan_from = 0;
                Framed::ForceFlush(out)
            } else {
                Framed::Nothing
            }
        }
    }

    /// Release any unterminated tail verbatim (stream end or client disconnect)
    fn finish(&mut self) -> Option<Vec<u8>> {
        self.scan_from = 0;
        if self.cr_held {
            self.cr_held = false;
            self.pending.push(b'\n');
        }
        if self.pending.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.pending))
        }
    }
}

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

fn count_double_newlines(buf: &[u8]) -> usize {
    let mut count = 0;
    let mut rest = buf;
    while let Some(rel) = find_double_newline(rest) {
        count += 1;
        rest = &rest[(rel + 2).min(rest.len())..];
    }
    count
}

/// Analysis-only parse of one complete SSE event. Lossy by design: multiple
/// `data:` lines join with `\n` per the SSE spec, comments and unknown fields
/// are ignored. Only the verbatim byte path is authoritative.
fn split_event(raw: &[u8]) -> (Option<String>, Vec<String>) {
    let text = String::from_utf8_lossy(raw);
    let mut event_type = None;
    let mut payloads = Vec::new();
    for line in text.lines() {
        if let Some(v) = strip_field(line, "event:") {
            event_type = Some(v.to_string());
        } else if let Some(v) = strip_field(line, "data:") {
            payloads.push(v.to_string());
        }
    }
    (event_type, payloads)
}

/// Extract a field value, tolerating the optional single space after the colon
/// (the SSE spec allows both `data: x` and `data:x`).
fn strip_field<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    line.strip_prefix(prefix).map(|v| v.strip_prefix(' ').unwrap_or(v))
}

/// True when this event is the terminal marker for either wire format. Driven
/// off parsed fields only: a substring scan would false-positive on a model
/// emitting the literal text `data: [DONE]` inside JSON string content.
fn is_completion_event(event_type: Option<&str>, payloads: &[String]) -> bool {
    event_type == Some("message_stop") || payloads.iter().any(|p| p.trim() == "[DONE]")
}

/// Count `data:` payloads that should be JSON but fail to parse. [DONE],
/// comments and empty payloads are legal non-JSON and don't count.
fn count_unparsed_payloads(payloads: &[String]) -> usize {
    payloads
        .iter()
        .filter(|p| {
            let t = p.trim();
            !t.is_empty() && t != "[DONE]" && !t.starts_with(':') && serde_json::from_str::<serde_json::Value>(t).is_err()
        })
        .count()
}

/// Pass through a response verbatim because we cannot safely frame it
/// (e.g. an encoded body whose bytes we must not newline-split).
fn verbatim_passthrough(
    backend_response: reqwest::Response,
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
    guard: BackendGuard,
) -> Response {
    let status = backend_response.status();
    let headers = backend_response.headers().clone();
    let byte_stream = backend_response.bytes_stream().map(move |chunk_result| {
        let _permit = &permit;
        let _guard = &guard;
        chunk_result.map_err(|e| std::io::Error::other(e.to_string()))
    });
    let mut builder = Response::builder().status(status);
    for (name, value) in headers {
        if let Some(name) = name {
            if name != axum::http::header::CONTENT_LENGTH {
                builder = builder.header(name, value);
            }
        }
    }
    builder.body(Body::from_stream(byte_stream)).unwrap().into_response()
}

/// Consume the backend SSE stream: frame complete events, pass them through
/// verbatim, accumulate raw bytes for end-of-stream stats. State lives in the
/// stream itself (single consumer), so no per-chunk clones or async locks;
/// `unfold` (unlike `then`) also sees stream end, where an unterminated tail
/// is flushed.
struct FusedStream<S> {
    inner: S,
    done: bool,
}

impl<S, E> futures::Stream for FusedStream<S>
where
    S: futures::Stream<Item = Result<Bytes, E>> + Unpin,
{
    type Item = Result<Bytes, E>;

    fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }
        match std::pin::Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(item)) => {
                if item.is_err() {
                    this.done = true;
                }
                Poll::Ready(Some(item))
            }
            Poll::Ready(None) => {
                this.done = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

struct StreamPassState<S> {
    underlying: FusedStream<S>,
    framing: FramingState,
    // None: nobody consumes the accumulation (stats off, no dump) - skip it
    accumulated: Option<Arc<std::sync::Mutex<Vec<u8>>>>,
    completion_tx: Option<tokio::sync::oneshot::Sender<StreamEnd>>,
    activity_tx: tokio::sync::watch::Sender<()>,
    // Backend read error (rendered; reqwest::Error is !Clone): pending
    // complete events and the unterminated tail are delivered first, then the
    // error surfaces to abort the response - unless completion already fired,
    // which downgrades to a graceful end.
    pending_error: Option<String>,
    completion_signaled: bool,
    // Concurrency permit: released when this stream is dropped (body completed
    // or client disconnected), not when the handler returns.
    _permit: Option<tokio::sync::OwnedSemaphorePermit>,
    // Backend busy claim: same body-lifetime contract as the permit - the node
    // is still generating while the client polls this stream, so the guard
    // releases here, not at handler return (big-fix E-H1).
    _guard: BackendGuard,
}

impl<S> StreamPassState<S> {
    /// The single exit for delivering bytes: accumulate, detect completion,
    /// signal activity. One copy of this policy instead of four drifting ones.
    fn emit_block(mut self, out: Vec<u8>) -> (Result<Bytes, std::io::Error>, Self) {
        self.maybe_accumulate(&out);
        self.completion_signaled |= detect_and_signal_completion(&out, &mut self.completion_tx);
        let _ = self.activity_tx.send(());
        (Ok(Bytes::from(out)), self)
    }

    fn maybe_accumulate(&self, bytes: &[u8]) {
        if let Some(acc) = &self.accumulated {
            // never panic on a poisoned lock: this path only appends
            let mut acc = acc.lock().unwrap_or_else(|e| e.into_inner());
            acc.extend_from_slice(bytes);
        }
    }

    /// Drain a pending error, flushing the unterminated tail first. If the
    /// stream had already semantically completed ([DONE] delivered), a late
    /// transport error downgrades to a graceful end: aborting would make the
    /// client throw on a body it had fully received.
    fn drain_error(mut self) -> DrainError<Self> {
        let err = match self.pending_error.take() {
            Some(e) => e,
            None => return DrainError::NoError(self),
        };
        let mut this = self;
        if let Some(tail) = this.framing.finish() {
            this.maybe_accumulate(&tail);
            this.completion_signaled |= detect_and_signal_completion(&tail, &mut this.completion_tx);
            let _ = this.activity_tx.send(());
            this.pending_error = Some(err);
            return DrainError::Emit((Ok(Bytes::from(tail)), this));
        }
        if this.completion_signaled {
            tracing::warn!(
                error = %err,
                "Backend connection error after stream completion; ending gracefully instead of aborting the client transfer"
            );
            return DrainError::EndNow;
        }
        signal_end(&mut this.completion_tx, StreamEnd::BackendError);
        DrainError::Emit((Err(std::io::Error::other(err)), this))
    }
}

enum DrainError<S> {
    NoError(S),
    Emit((Result<Bytes, std::io::Error>, S)),
    /// Terminate the pass-through without surfacing an error to the client.
    EndNow,
}

async fn advance_stream<S, E>(mut state: StreamPassState<S>) -> Option<(Result<Bytes, std::io::Error>, StreamPassState<S>)>
where
    S: futures::Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    use futures::StreamExt;
    loop {
        match state.framing.take_framed() {
            Framed::Events(out) => {
                // Counted only where the unparsed/fix ratios are also computed:
                // a denominator that keeps ticking while its numerator cannot
                // would make every ratio read falsely pristine.
                if state.accumulated.is_some() {
                    SSE_EVENTS_TOTAL.fetch_add(count_double_newlines(&out) as u64, AtomicOrdering::Relaxed);
                }
                return Some(state.emit_block(out));
            }
            Framed::ForceFlush(out) => {
                return Some(state.emit_block(out));
            }
            Framed::Nothing => {}
        }

        match state.drain_error() {
            DrainError::NoError(s) => state = s,
            DrainError::Emit(step) => return Some(step),
            DrainError::EndNow => return None,
        }

        match state.underlying.next().await {
            None => {
                // Stream end: release any unterminated tail verbatim
                if let Some(tail) = state.framing.finish() {
                    return Some(state.emit_block(tail));
                }
                signal_end(&mut state.completion_tx, StreamEnd::BackendClosed);
                let _ = state.activity_tx.send(());
                return None;
            }
            Some(Err(e)) => {
                tracing::error!(error = %e, "Error reading stream chunk");
                state.pending_error = Some(e.to_string());
                continue;
            }
            Some(Ok(chunk)) => {
                // Signal activity even for a zero-length chunk (HTTP/2
                // keepalive DATA): the stream is alive, so the inactivity
                // timer must reset or a quiet-but-live stream reads as Stalled.
                let _ = state.activity_tx.send(());
                if chunk.is_empty() {
                    return Some((Ok(chunk), state));
                }
                state.framing.push(&chunk);
                continue;
            }
        }
    }
}

/// Fire the completion signal when framed bytes contain a terminal event.
/// Returns whether this call completed the stream.
/// Why a pass-through stream ended. Sent over the completion oneshot BEFORE
/// its sender is dropped, so a receiver seeing the sender drop without a
/// value can only mean one thing: the body was dropped (client disconnected).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamEnd {
    Completed,
    BackendClosed,
    BackendError,
    Stalled,
    ClientGone,
}

impl StreamEnd {
    fn as_str(self) -> &'static str {
        match self {
            StreamEnd::Completed => "ok",
            StreamEnd::BackendClosed | StreamEnd::BackendError => "truncated",
            StreamEnd::Stalled => "stalled",
            StreamEnd::ClientGone => "client_gone",
        }
    }
}

fn signal_end(completion_tx: &mut Option<tokio::sync::oneshot::Sender<StreamEnd>>, reason: StreamEnd) {
    if let Some(tx) = completion_tx.take() {
        let _ = tx.send(reason);
    }
}

/// Byte prefilter for completion detection: both terminal markers must appear
/// literally in a block's bytes. `is_completion_event` matches an `event:`
/// field exactly `message_stop` and a `data:` payload whose trim is exactly
/// `[DONE]` - `split_event` does no JSON escape decoding, so firing requires
/// the literal windows checked here. Conservative by construction: a false
/// positive falls through to the parser, a false negative is impossible, and
/// the overwhelmingly common non-terminal block pays nothing.
fn mentions_bridge_event(b: &[u8]) -> bool {
    b.windows(5).any(|w| w == b"[DONE") || b.windows(12).any(|w| w == b"message_stop")
}

fn detect_and_signal_completion(bytes: &[u8], completion_tx: &mut Option<tokio::sync::oneshot::Sender<StreamEnd>>) -> bool {
    if completion_tx.is_none() || !mentions_bridge_event(bytes) {
        return false;
    }
    let text = String::from_utf8_lossy(bytes);
    for raw in text.split("\n\n") {
        let (event_type, payloads) = split_event(raw.as_bytes());
        if is_completion_event(event_type.as_deref(), &payloads) {
            if let Some(tx) = completion_tx.take() {
                let _ = tx.send(StreamEnd::Completed);
                return true;
            }
            return false;
        }
    }
    false
}

/// Everything the end-of-stream observer task needs beyond the channels and
/// the accumulated bytes. Built once per pass-through stream when something
/// can consume the analysis (stats or dump), then moved into the spawned task.
/// [D-M7] decomposition: this replaced a 240-line inline `async move` closure;
/// each named member is one responsibility, edit them independently.
struct StreamObserver {
    completion_rx: tokio::sync::oneshot::Receiver<StreamEnd>,
    activity_rx: tokio::sync::watch::Receiver<()>,
    accumulated: Arc<std::sync::Mutex<Vec<u8>>>,
    start: Instant,
    fix_registry: Arc<FixRegistry>,
    request_json: Option<serde_json::Value>,
    stats_enabled: bool,
    stats_format: StatsFormat,
    streaming_mode: crate::config::StreamingMode,
    group_name: Option<String>,
    concurrent_requests: usize,
    exporter_manager: Arc<ExporterManager>,
    http_client: reqwest::Client,
    backend_url: String,
    strip_path_prefix: Option<String>,
    dump: Option<StreamDump>,
}

/// A streamed response that can be dumped: exists only when every input the
/// dump needs (path, method, URI, parsed request) is present.
struct StreamDump {
    path: Arc<std::path::PathBuf>,
    response_headers: axum::http::HeaderMap,
    method: String,
    uri: String,
    request_json: serde_json::Value,
    backend_request_body: Option<Vec<u8>>,
    status: u16,
}

impl StreamObserver {
    /// Await the end, tally it, analyze the accumulated bytes exactly once.
    async fn run(mut self) {
        let end = self.await_end().await;
        Self::tally_end(end, self.start);
        // The operator's question is "did the client get a whole answer?",
        // which a stalled stream also failed to deliver. Which counter it
        // landed in is diagnosis, and that is what stream_end is for.
        let stream_incomplete = match end {
            StreamEnd::Completed | StreamEnd::ClientGone => false,
            StreamEnd::BackendClosed | StreamEnd::BackendError | StreamEnd::Stalled => true,
        };
        let stream_end = end.as_str();

        // Copy out and release: the std MutexGuard must not be held across
        // the awaits below, and by now the stream is finished, so the
        // accumulated bytes are immutable.
        let acc = self.accumulated.lock().unwrap_or_else(|e| e.into_inner()).clone();
        tracing::trace!("Accumulated SSE data length: {} bytes", acc.len());
        if acc.is_empty() {
            return;
        }
        let acc_text = String::from_utf8_lossy(&acc);
        let preview: String = acc_text.chars().take(1000).collect();
        tracing::trace!("Accumulated SSE preview:\n{}", preview);

        Self::scan_unparsed_events(&acc_text);

        match parse_accumulated_sse(&acc_text) {
            Some(final_event) => {
                self.note_unrepaired_fixes(&final_event);
                tracing::trace!(
                    "Successfully merged SSE events into final response with keys: {:?}",
                    final_event.as_object().map(|o| o.keys().collect::<Vec<_>>())
                );
                self.spawn_dump(&final_event);
                if self.stats_enabled {
                    self.publish_stats(&final_event, stream_incomplete, stream_end).await;
                }
            }
            None => {
                tracing::trace!(
                    duration_ms = self.start.elapsed().as_millis() as u64,
                    "Streaming completed (unable to parse final event)"
                );
            }
        }
    }

    /// Wait for the end reason: completion signal, activity channel closing,
    /// or the inactivity timeout.
    async fn await_end(&mut self) -> StreamEnd {
        // Reset on each chunk. There is deliberately no absolute cap: a stream that
        // keeps producing is not stalled however long it runs, and its total length
        // is already bounded by the backend client's timeout_seconds, which ends the
        // body with an error (BackendError) rather than leaving this task waiting.
        const ACTIVITY_TIMEOUT_SECS: u64 = 90;

        let end_reason = loop {
            // Create fresh activity timeout each iteration (resets on activity)
            let activity_timeout_sleep = tokio::time::sleep(tokio::time::Duration::from_secs(ACTIVITY_TIMEOUT_SECS));
            tokio::pin!(activity_timeout_sleep);

            tokio::select! {
                // End reason from the stream (see StreamEnd for the contract)
                result = &mut self.completion_rx => {
                    match result {
                        Ok(reason) => break reason,
                        Err(_) => break StreamEnd::ClientGone,
                    }
                }

                // Activity detected - reset the activity timeout by continuing loop
                res = self.activity_rx.changed() => {
                    if res.is_ok() {
                        tracing::trace!("Stream activity detected, resetting timeout");
                        // Continue loop with fresh timers
                        continue;
                    } else {
                        // All senders dropped: the body stream is gone.
                        // Waiting further would idle until the inactivity
                        // timeout and log a spurious warning.
                        tracing::trace!("Activity channel closed, stream ended");
                        break StreamEnd::ClientGone;
                    }
                }

                // 90s inactivity timeout
                _ = &mut activity_timeout_sleep => {
                    tracing::warn!(
                        "Stream inactivity timeout ({}s since last chunk), extracting metrics",
                        ACTIVITY_TIMEOUT_SECS
                    );
                    break StreamEnd::Stalled;
                }
            }
        };

        // select! resolves ready branches at random, so on a fast local
        // stream the timeout/activity branch can win a tie against a
        // completion that already fired. A sent value survives sender drop
        // in a oneshot, so drain it: the real end reason beats the branch
        // that happened to be polled.
        self.completion_rx.try_recv().unwrap_or(end_reason)
    }

    /// Map the end reason onto the process-wide end-class counters.
    fn tally_end(end: StreamEnd, start: Instant) {
        match end {
            StreamEnd::Completed => {
                tracing::trace!(duration_ms = start.elapsed().as_millis() as u64, "Stream completed normally");
            }
            StreamEnd::BackendClosed | StreamEnd::BackendError => {
                STREAM_TRUNCATED_TOTAL.fetch_add(1, AtomicOrdering::Relaxed);
            }
            StreamEnd::Stalled => {
                STREAM_STALLED_TOTAL.fetch_add(1, AtomicOrdering::Relaxed);
            }
            StreamEnd::ClientGone => {
                STREAM_CLIENT_GONE_TOTAL.fetch_add(1, AtomicOrdering::Relaxed);
                tracing::debug!("Stream ended by client disconnect before completion");
            }
        }
    }

    /// Detect-only pass (this path never repairs): per-event JSON parse
    /// failures are counted against the verbatim-forwarding denominator.
    fn scan_unparsed_events(acc_text: &str) {
        let mut unparsed_events = 0u64;
        let mut unparsed_preview: Option<String> = None;
        for raw_event in acc_text.split("\n\n") {
            let (_event_type, payloads) = split_event(raw_event.as_bytes());
            let bad = count_unparsed_payloads(&payloads);
            if bad > 0 {
                unparsed_events += 1;
                if unparsed_preview.is_none() {
                    if let Some(p) = payloads
                        .iter()
                        .find(|p| serde_json::from_str::<serde_json::Value>(p.trim()).is_err())
                    {
                        let snippet: String = p.chars().take(120).collect();
                        unparsed_preview = Some(snippet);
                    }
                }
            }
        }
        if unparsed_events > 0 {
            SSE_UNPARSED_EVENTS_TOTAL.fetch_add(unparsed_events, AtomicOrdering::Relaxed);
            tracing::warn!(
                unparsed_events = unparsed_events,
                example = %unparsed_preview.unwrap_or_default(),
                "streaming_pass_through: {} SSE event payload(s) were not valid JSON - forwarded verbatim, unanalyzed",
                unparsed_events
            );
        }
    }

    /// Fix predicates run against the merged response so operators see what
    /// the client got unmodified. Runs before merging succeeds or fails - a
    /// stream we can't merge is exactly the malformed case to surface.
    fn note_unrepaired_fixes(&self, final_event: &serde_json::Value) {
        let detected = self
            .fix_registry
            .detect_fixes(final_event, self.request_json.as_ref(), "passthrough_forwards_verbatim");
        if detected.is_empty() {
            return;
        }
        let n = FIX_UNREPAIRED_TOTAL.fetch_add(detected.len() as u64, AtomicOrdering::Relaxed) + 1;
        // A model that reliably emits the defect would WARN on
        // 100% of requests and bury everything else; the counter
        // stays exact, the log is sampled.
        if n == 1 || n.is_multiple_of(100) {
            tracing::warn!(
                fixes = %detected.join(","),
                total_unrepaired = n,
                mode = %self.streaming_mode,
                "streaming_pass_through: detected, NOT repaired (SSE forwarded bytes verbatim; per-chunk repair corrupts partial deltas) - client received them as-is"
            );
        }
    }

    /// Hand the dump to its own task; the inputs move in (they exist for this
    /// one use), only `final_event` is cloned because stats still needs it.
    fn spawn_dump(&mut self, final_event: &serde_json::Value) {
        let Some(StreamDump {
            path,
            response_headers,
            method,
            uri,
            request_json,
            backend_request_body,
            status,
        }) = self.dump.take()
        else {
            return;
        };
        let final_event = final_event.clone();

        tokio::spawn(async move {
            // Prefer the bytes actually sent; the parsed request_json
            // predates the proxy's own stream/stream_options edits.
            let request_body = backend_request_body.unwrap_or_else(|| serde_json::to_vec(&request_json).unwrap_or_default());
            let response_body = serde_json::to_vec(&final_event).unwrap_or_default();
            let req_content_type = Some("application/json");
            let res_content_type = response_headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok());

            if let Err(e) = dump::dump_request_response(
                &path,
                &method,
                &uri,
                &request_body,
                req_content_type,
                status,
                &response_body,
                res_content_type,
            )
            .await
            {
                tracing::warn!(error = %e, "Failed to dump streaming request/response pair");
            }
        });
    }

    /// Assemble the sample, enrich it with context usage, log and export.
    async fn publish_stats(&mut self, final_event: &serde_json::Value, stream_incomplete: bool, stream_end: &'static str) {
        let Some(req_json) = self.request_json.as_ref() else {
            tracing::trace!(
                duration_ms = self.start.elapsed().as_millis() as u64,
                "Streaming completed (no request JSON)"
            );
            return;
        };
        let mut metrics = RequestMetrics::from_response(
            final_event,
            req_json,
            true, // streaming
            self.start.elapsed().as_millis() as f64,
        );
        // Set group name if we're in multi-backend mode
        metrics.group_name = self.group_name.take();
        metrics.concurrent_requests = Some(self.concurrent_requests);
        metrics.stream_incomplete = stream_incomplete;
        metrics.stream_end = Some(stream_end);

        // A sample with no throughput signal has no token count
        // either, so nothing can be done with context_percent -
        // skip the /slots round-trip it would be wasted on.
        if metrics.has_throughput_signal() {
            match fetch_context_total(&self.http_client, &self.backend_url, self.strip_path_prefix.as_deref()).await {
                Some(ctx_total) => {
                    metrics.context_total = Some(ctx_total);
                    metrics.calculate_context_percent();
                }
                None => {
                    // Warn once per backend URL, not per request
                    crate::proxy::warn_context_fetch_failed_once(&self.backend_url, &metrics.model).await;
                    // Continue without context metrics - the request still succeeds
                }
            }
        }

        // The gate logs the sample and decides whether it is fit
        // to export.
        if crate::exporters::log_sample_and_should_export(&metrics, self.stats_format) {
            // Export to remote systems
            self.exporter_manager.export_all(&metrics).await;
        }
    }
}

/// Dump utilities for request/response debugging
pub mod dump {
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::fs;

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

    /// Dump request/response pair to disk
    pub async fn dump_request_response(
        dump_path: &Arc<PathBuf>,
        request_method: &str,
        request_uri: &str,
        request_body: &[u8],
        request_content_type: Option<&str>,
        response_status: u16,
        response_body: &[u8],
        response_content_type: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use tokio::io::AsyncWriteExt;

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

        // Write response file
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

        // Write metadata
        let meta_path = request_dir.join("meta.txt");
        let mut meta_file = fs::File::create(&meta_path).await?;
        let meta_str = format!(
            "Request:\n  Method: {}\n  URI: {}\n  Content-Type: {:?}\n  Body Size: {} bytes\n\nResponse:\n  Status: {}\n  Content-Type: {:?}\n  Body Size: {} bytes\n",
            request_method, request_uri, request_content_type, request_body.len(), response_status, response_content_type, response_body.len()
        );
        meta_file.write_all(meta_str.as_bytes()).await?;
        meta_file.flush().await?;

        tracing::info!(request_id = %request_id, dump_path = %request_dir.display(), "Dumped request/response pair");

        Ok(())
    }
}

/// Handle a streaming (SSE) response from the backend
#[allow(clippy::too_many_arguments)]
pub async fn handle_streaming_response(
    backend_response: reqwest::Response,
    fix_registry: Arc<FixRegistry>,
    stats_enabled: bool,
    stats_format: StatsFormat,
    exporter_manager: Arc<ExporterManager>,
    request_json: Option<serde_json::Value>,
    start: Instant,
    http_client: reqwest::Client,
    backend_url: String,
    group_name: Option<String>,
    strip_path_prefix: Option<String>,
    dump_path: Option<Arc<std::path::PathBuf>>,
    request_method: Option<String>,
    request_uri: Option<String>,
    // The exact bytes handed to the backend, so a dump records what was really sent.
    backend_request_body: Option<Vec<u8>>,
    concurrent_requests: usize,
    // Only for log accuracy: the detect-only notice must not claim the mode is
    // passthrough when a backend disobeyed `stream:false` under `fake`.
    streaming_mode: crate::config::StreamingMode,
    // Concurrency permit for this request, if limiting is enabled. Held for the lifetime
    // of the returned body rather than of this call: the response is a lazy stream, so
    // the work this permit accounts for happens while the client polls the body, long
    // after this function returns.
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
    // Backend busy claim from the balancer. Same body-lifetime contract as the permit:
    // the node is still generating while the client polls the body, so the guard must
    // ride the stream - releasing it at handler return would let the balancer pile
    // requests onto a node that is still busy (big-fix E-H1).
    guard: BackendGuard,
) -> Response {
    let status = backend_response.status();
    let headers = backend_response.headers().clone();
    let response_status = status.as_u16();
    let response_headers = headers.clone();

    // A body we cannot safely newline-split (unknown Content-Encoding surviving
    // reqwest's transparent decoding) must not be framed at all: forward every
    // byte untouched and skip framing/accumulation/stats for this response.
    let content_encoding = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("identity")
        .to_string();
    if !content_encoding.eq_ignore_ascii_case("identity") {
        let n = STREAM_COMPRESSED_TOTAL.fetch_add(1, AtomicOrdering::Relaxed) + 1;
        if n == 1 || n.is_multiple_of(100) {
            tracing::warn!(
                content_encoding = %content_encoding,
                count = n,
                "Streaming response carries Content-Encoding; proxying verbatim with no stats (backend ignored Accept-Encoding). reqwest decodes gzip transparently, so this usually means br/zstd."
            );
        }
        return verbatim_passthrough(backend_response, permit, guard);
    }

    // Accumulate raw SSE bytes (verbatim) for end-of-stream stats/dump.
    // std Mutex, not tokio: never held across an await, single consumer.
    // Skip entirely when nothing will read it back (stats off, no dump).
    let accumulates = stats_enabled || dump_path.is_some();

    // Every response past this point is framed by us: this is the denominator
    // for the passthrough_* ratios (compressed responses bypass framing and
    // are counted separately). Counted only when something can observe how the
    // stream ended - the end-reason numerators live in the spawned task this
    // boolean gates, so an ungated denominator would read a pristine 0%
    // truncation on a backend that truncates every response.
    if accumulates {
        PASSTHROUGH_STREAMS_TOTAL.fetch_add(1, AtomicOrdering::Relaxed);
    }

    let stream = backend_response.bytes_stream();

    let accumulated = accumulates.then(|| Arc::new(std::sync::Mutex::new(Vec::<u8>::new())));
    let accumulated_out = accumulated.clone();

    // Create oneshot channel for stream completion signaling
    let (completion_tx, completion_rx) = tokio::sync::oneshot::channel::<StreamEnd>();

    // Activity signaling for the stats observer's inactivity timeout (watch channel)
    let (activity_tx, activity_rx) = tokio::sync::watch::channel(());

    let pass_state = StreamPassState {
        underlying: FusedStream {
            inner: stream,
            done: false,
        },
        framing: FramingState::new(),
        accumulated,
        completion_tx: Some(completion_tx),
        activity_tx,
        pending_error: None,
        completion_signaled: false,
        _permit: permit,
        _guard: guard,
    };

    // Pass-through stream: complete SSE events are forwarded byte-for-byte as
    // the backend sent them (no parse/re-serialize on the hot path; fixes do
    // not run on this path - see detect_fixes at stream end).
    let processed_stream = futures::stream::unfold(pass_state, advance_stream);

    // Spawn task to collect stats and/or dump after stream completes
    if let Some(accumulated) = accumulated_out {
        // [D-M6] Moves, not clones: after this block the fn reads only
        // headers/status/stream, so every binding below moves into the task.
        // The single surviving deep clone is dump-gated; the default (no
        // --dump) path clones nothing. Do not re-clone (regression [D]M6).
        let dump = match (dump_path, request_method, request_uri, request_json.as_ref()) {
            (Some(path), Some(method), Some(uri), Some(parsed)) => Some(StreamDump {
                path,
                response_headers,
                method,
                uri,
                request_json: parsed.clone(),
                // The dump is the only second consumer of the exact bytes sent
                // to the backend; with no dump the buffer is dropped, not copied.
                backend_request_body,
                status: response_status,
            }),
            _ => None,
        };
        let observer = StreamObserver {
            completion_rx,
            activity_rx,
            accumulated,
            start,
            fix_registry,
            request_json,
            stats_enabled,
            stats_format,
            streaming_mode,
            group_name,
            concurrent_requests,
            exporter_manager,
            http_client,
            backend_url,
            strip_path_prefix,
            dump,
        };
        tokio::spawn(observer.run());
    }

    // Build streaming response
    let mut response = Response::builder().status(status);

    for (name, value) in headers {
        if let Some(name) = name {
            // Skip content-length as we're streaming
            if name != axum::http::header::CONTENT_LENGTH {
                response = response.header(name, value);
            }
        }
    }

    let body = Body::from_stream(processed_stream);
    response.body(body).unwrap().into_response()
}

/// API format detected from SSE stream
#[derive(Debug, Clone, Copy, PartialEq)]
enum StreamFormat {
    OpenAI,
    Anthropic,
    Unknown,
}

/// Detect API format from SSE data. Field parsing goes through `strip_field`
/// so backends that omit the space after `data:`/`event:` (legal SSE) are not
/// silently misdetected as Unknown - see split_event for the canonical parser.
fn detect_format(data: &str) -> StreamFormat {
    // Look for Anthropic markers: event: message_start or "type": "message_start" in data
    for line in data.lines() {
        if let Some(event_type) = strip_field(line, "event:") {
            if event_type == "message_start" || event_type == "content_block_delta" {
                return StreamFormat::Anthropic;
            }
        }
        if let Some(json_str) = strip_field(line, "data:") {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                // `choices` is checked first: an OpenAI chunk is OpenAI even if some
                // backend also puts a `type` on it. Keying on "root has a type field"
                // misread such a stream as Anthropic and corrupted the merged result.
                if json.get("choices").is_some() {
                    return StreamFormat::OpenAI;
                }
                if json.get("type").and_then(|t| t.as_str()).is_some_and(is_anthropic_event) {
                    return StreamFormat::Anthropic;
                }
            }
        }
    }
    StreamFormat::Unknown
}

/// Event names the Anthropic Messages API puts in a root-level `type`. `completion`
/// is excluded on purpose: it belongs to the retired text-completions API.
fn is_anthropic_event(name: &str) -> bool {
    matches!(
        name,
        "message_start"
            | "content_block_start"
            | "content_block_delta"
            | "content_block_stop"
            | "message_delta"
            | "message_stop"
            | "ping"
            | "error"
    )
}

/// Parsed SSE event with optional event type
struct SseEvent {
    event_type: Option<String>,
    data: serde_json::Value,
}

fn parse_accumulated_sse(data: &str) -> Option<serde_json::Value> {
    tracing::trace!("Parsing accumulated SSE data ({} bytes)", data.len());

    let format = detect_format(data);
    tracing::trace!("Detected stream format: {:?}", format);

    let events = parse_sse_events(data);
    tracing::trace!("Parsed {} SSE events", events.len());

    match format {
        StreamFormat::Anthropic => {
            let merged = merge_anthropic_events(events);
            tracing::trace!("Merged Anthropic response: {:?}", merged);
            Some(merged)
        }
        StreamFormat::OpenAI => {
            let mut combined: Option<serde_json::Value> = None;
            for event in events {
                combined = Some(merge_chunk(combined, event.data));
            }
            tracing::trace!("Merged OpenAI response: {:?}", combined);
            combined
        }
        StreamFormat::Unknown => {
            tracing::warn!("Unknown SSE format, cannot extract metrics");
            None
        }
    }
}

/// Parse SSE events with their types. Like detect_format, tolerant of the
/// optional space after the field colon (SSE spec).
fn parse_sse_events(data: &str) -> Vec<SseEvent> {
    let mut events = Vec::new();
    let mut current_event_type: Option<String> = None;

    for line in data.lines() {
        if let Some(v) = strip_field(line, "event:") {
            current_event_type = Some(v.to_string());
        } else if let Some(json_str) = strip_field(line, "data:") {
            if json_str.trim() == "[DONE]" {
                continue;
            }
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                events.push(SseEvent {
                    event_type: current_event_type.clone(),
                    data: json,
                });
                current_event_type = None;
            }
        }
    }

    events
}

/// Merge Anthropic SSE events into a single response
fn merge_anthropic_events(events: Vec<SseEvent>) -> serde_json::Value {
    use serde_json::json;

    let mut message = json!({"content": []});

    for event in &events {
        let event_type = event.event_type.as_deref().unwrap_or_else(|| {
            // Fall back to data's type field if no event line
            event.data.get("type").and_then(|t| t.as_str()).unwrap_or("")
        });

        match event_type {
            "message_start" => {
                // message_start contains the initial message object
                if let Some(msg) = event.data.get("message") {
                    message = msg.clone();
                    // Ensure content array exists
                    if message.get("content").is_none() {
                        message["content"] = json!([]);
                    }
                }
            }
            "content_block_start" => {
                // Create content block at specified index
                let idx = event.data.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                if idx > 100 {
                    tracing::warn!(idx, "content_block_start index too large, skipping");
                } else if let Some(block) = event.data.get("content_block") {
                    if let Some(content) = message.get_mut("content").and_then(|c| c.as_array_mut()) {
                        while content.len() <= idx {
                            content.push(json!(null));
                        }
                        content[idx] = block.clone();
                    }
                }
            }
            "content_block_delta" => {
                // Append delta text/thinking to content block
                let idx = event.data.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                if idx > 100 {
                    tracing::warn!(idx, "content_block_delta index too large, skipping");
                } else if let Some(delta) = event.data.get("delta") {
                    if let Some(content) = message.get_mut("content").and_then(|c| c.as_array_mut()) {
                        // Ensure array is large enough and block exists with proper type
                        while content.len() <= idx {
                            content.push(json!(null));
                        }

                        // If block is null, initialize it based on delta type
                        if content[idx].is_null() {
                            let delta_type = delta.get("type").and_then(|t| t.as_str()).unwrap_or("text_delta");
                            let block_type = if delta_type == "thinking_delta" {
                                "thinking"
                            } else if delta_type == "input_json_delta" {
                                "tool_use"
                            } else {
                                "text"
                            };
                            content[idx] = json!({"type": block_type});
                        }

                        let block = &mut content[idx];
                        if let Some(obj) = block.as_object_mut() {
                            // Handle text_delta - use in-place mutation for O(n) performance
                            if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                if let Some(serde_json::Value::String(ref mut existing)) = obj.get_mut("text") {
                                    existing.push_str(text);
                                } else {
                                    obj.insert("text".to_string(), json!(text));
                                }
                            }

                            // Handle thinking_delta (for reasoning models)
                            if let Some(thinking) = delta.get("thinking").and_then(|t| t.as_str()) {
                                if let Some(serde_json::Value::String(ref mut existing)) = obj.get_mut("thinking") {
                                    existing.push_str(thinking);
                                } else {
                                    obj.insert("thinking".to_string(), json!(thinking));
                                }
                            }

                            // Handle partial_json for tool_use input
                            if let Some(partial) = delta.get("partial_json").and_then(|p| p.as_str()) {
                                if let Some(serde_json::Value::String(ref mut existing)) = obj.get_mut("input") {
                                    existing.push_str(partial);
                                } else {
                                    obj.insert("input".to_string(), json!(partial));
                                }
                            }
                        }
                    }
                }
            }
            "message_delta" => {
                // Contains stop_reason and output_tokens usage
                if let Some(delta) = event.data.get("delta") {
                    if let Some(stop) = delta.get("stop_reason").and_then(|s| s.as_str()) {
                        message["stop_reason"] = json!(stop);
                    }
                }
                // usage is at top level of message_delta event
                if let Some(usage) = event.data.get("usage") {
                    if let Some(msg_usage) = message.get_mut("usage") {
                        if let Some(obj) = msg_usage.as_object_mut() {
                            if let Some(output_tokens) = usage.get("output_tokens") {
                                obj.insert("output_tokens".to_string(), output_tokens.clone());
                            }
                        }
                    } else {
                        message["usage"] = usage.clone();
                    }
                }
            }
            "content_block_stop" | "message_stop" | "ping" => {
                // These are signals only, no data to merge
            }
            "error" => {
                // Log error events at warn level
                if let Some(err) = event.data.get("error") {
                    tracing::warn!("Anthropic error event: {:?}", err);
                } else {
                    tracing::warn!("Anthropic error event with no error field: {:?}", event.data);
                }
            }
            _ => {
                tracing::trace!("Unknown Anthropic event type: {}", event_type);
            }
        }
    }

    message
}

/// Merge a streaming chunk into accumulated response
/// A choice to accumulate into. Streamed chunks carry only `delta`, so the `message`
/// the rest of the proxy reads (fix predicates, stats, dumps) has to be built here;
/// `tool_calls` starts as null because the merge below only fills an existing slot.
fn seed_choice(choice: &serde_json::Value) -> serde_json::Value {
    let mut seeded = choice.clone();
    if let Some(obj) = seeded.as_object_mut() {
        if obj.remove("delta").is_some() || !obj.contains_key("message") {
            obj.insert(
                "message".to_string(),
                serde_json::json!({ "role": "assistant", "content": null, "tool_calls": null }),
            );
        }
    }
    seeded
}

/// Merge a continuation fragment into an already-seeded tool-call slot:
/// argument fragments concatenate, a name lands on a nameless slot or
/// completes a split name, and an identical name resend is skipped.
fn merge_tool_call_slot(slot: &mut serde_json::Value, new_call: &serde_json::Value) {
    let Some(new_func) = new_call.get("function") else {
        return;
    };
    if slot.get("function").is_none() {
        slot["function"] = new_func.clone();
        return;
    }
    let Some(acc_func) = slot.get_mut("function").and_then(|f| f.as_object_mut()) else {
        return;
    };
    if let Some(name) = new_func.get("name").and_then(|n| n.as_str()).filter(|n| !n.is_empty()) {
        match acc_func.get_mut("name") {
            Some(serde_json::Value::String(existing)) if existing.is_empty() => *existing = name.to_string(),
            Some(serde_json::Value::String(existing)) if existing != name => existing.push_str(name),
            Some(serde_json::Value::String(_)) => {}
            _ => {
                acc_func.insert("name".to_string(), serde_json::Value::String(name.to_string()));
            }
        }
    }
    if let Some(args) = new_func.get("arguments").and_then(|a| a.as_str()) {
        match acc_func.get_mut("arguments") {
            Some(serde_json::Value::String(existing)) => existing.push_str(args),
            _ => {
                acc_func.insert("arguments".to_string(), serde_json::Value::String(args.to_string()));
            }
        }
    }
}

fn merge_chunk(acc: Option<serde_json::Value>, chunk: serde_json::Value) -> serde_json::Value {
    match (acc, chunk) {
        (None, chunk) => {
            // Start from the chunk's envelope with no choices, then merge the chunk into
            // it, so the first delta goes through the same path as every later one.
            let mut envelope = chunk.clone();
            let seeded = match envelope.get_mut("choices").and_then(|c| c.as_array_mut()) {
                Some(choices) => {
                    choices.clear();
                    true
                }
                None => false,
            };
            // A first chunk without a choices array (usage-only) must still seed the
            // slot list: without the key, every later choice delta finds no merge
            // target and is silently dropped (captured raw in task53 evidence).
            if !seeded {
                envelope["choices"] = serde_json::Value::Array(Vec::new());
            }
            merge_chunk(Some(envelope), chunk)
        }
        (Some(mut acc), chunk) => {
            // Merge choices
            if let Some(acc_choices) = acc.get_mut("choices").and_then(|c| c.as_array_mut()) {
                if let Some(chunk_choices) = chunk.get("choices").and_then(|c| c.as_array()) {
                    for (i, choice) in chunk_choices.iter().enumerate() {
                        // A usage-only first chunk (`choices: []`) leaves nothing to merge
                        // into, so choices are seeded whenever they first appear.
                        if acc_choices.len() == i {
                            acc_choices.push(seed_choice(choice));
                        }
                        if let Some(acc_choice) = acc_choices.get_mut(i) {
                            if let Some(delta) = choice.get("delta") {
                                if let Some(role) = delta.get("role").filter(|r| r.is_string()) {
                                    if let Some(acc_msg) = acc_choice.get_mut("message") {
                                        acc_msg["role"] = role.clone();
                                    }
                                }

                                // Merge delta content
                                if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                    if let Some(acc_msg) = acc_choice.get_mut("message") {
                                        match acc_msg.get_mut("content") {
                                            Some(serde_json::Value::String(existing)) => existing.push_str(content),
                                            _ => acc_msg["content"] = serde_json::Value::String(content.to_string()),
                                        }
                                    }
                                }

                                // Merge reasoning_text (concatenate like content)
                                if let Some(reasoning) = delta.get("reasoning_text").and_then(|r| r.as_str()) {
                                    if let Some(acc_msg) = acc_choice.get_mut("message") {
                                        match acc_msg.get_mut("reasoning_text") {
                                            Some(serde_json::Value::String(existing)) => existing.push_str(reasoning),
                                            _ => acc_msg["reasoning_text"] = serde_json::Value::String(reasoning.to_string()),
                                        }
                                    }
                                }

                                // Merge reasoning_opaque (replace, not concat - it's a state blob)
                                if let Some(opaque) = delta.get("reasoning_opaque") {
                                    if let Some(acc_msg) = acc_choice.get_mut("message") {
                                        acc_msg["reasoning_opaque"] = opaque.clone();
                                    }
                                }

                                // Merge tool calls
                                if let Some(tool_calls) = delta.get("tool_calls") {
                                    if let Some(acc_tc) = acc_choice.get_mut("message").and_then(|m| m.get_mut("tool_calls")) {
                                        // Append or merge tool calls
                                        if acc_tc.is_null() {
                                            *acc_tc = tool_calls.clone();
                                        } else if let (Some(acc_arr), Some(new_arr)) =
                                            (acc_tc.as_array_mut(), tool_calls.as_array())
                                        {
                                            for new_call in new_arr {
                                                match new_call.get("index").and_then(|i| i.as_u64()) {
                                                    Some(idx) if idx > 100 => {
                                                        tracing::warn!(idx, "tool call index too large, skipping");
                                                    }
                                                    Some(idx) => {
                                                        let idx = idx as usize;
                                                        // Find or create slot for this index
                                                        while acc_arr.len() <= idx {
                                                            acc_arr.push(serde_json::Value::Null);
                                                        }
                                                        if acc_arr[idx].is_null() {
                                                            acc_arr[idx] = new_call.clone();
                                                        } else {
                                                            merge_tool_call_slot(&mut acc_arr[idx], new_call);
                                                        }
                                                    }
                                                    // A fragment with no index continues the last
                                                    // recorded slot; dropping it truncates the
                                                    // arguments mid-JSON.
                                                    None => match acc_arr.iter_mut().rev().find(|s| !s.is_null()) {
                                                        Some(last) => merge_tool_call_slot(last, new_call),
                                                        None => acc_arr.push(new_call.clone()),
                                                    },
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Update finish reason
                            if let Some(finish) = choice.get("finish_reason") {
                                if !finish.is_null() {
                                    acc_choice["finish_reason"] = finish.clone();
                                }
                            }
                        }
                    }
                }
            }

            // Merge usage if present
            if let Some(usage) = chunk.get("usage") {
                acc["usage"] = usage.clone();
            }

            // Explicitly preserve model field (llama.cpp includes in first chunk)
            if let Some(model) = chunk.get("model") {
                if !model.is_null() {
                    acc["model"] = model.clone();
                }
            }

            // Preserve timings if present (llama.cpp extension, in final chunk)
            if let Some(timings) = chunk.get("timings") {
                acc["timings"] = timings.clone();
            }

            // vLLM's equivalent of `timings`, on the usage chunk. Without this the
            // collector sees neither key and every tokens/sec column is empty on a
            // vLLM backend, because the wall-clock estimate it used to fall back to
            // was removed as not-a-measurement.
            if let Some(metrics) = chunk.get("metrics").filter(|v| !v.is_null()) {
                acc["metrics"] = metrics.clone();
            }

            acc
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_merge_chunk_preserves_model() {
        let chunk1 = json!({"model": "qwen3", "choices": []});
        let chunk2 = json!({"choices": [{"delta": {"content": "hi"}}]});

        let merged = merge_chunk(None, chunk1);
        let merged = merge_chunk(Some(merged), chunk2);

        assert_eq!(merged["model"].as_str().unwrap(), "qwen3");
    }

    #[test]
    fn test_merge_chunk_preserves_timings() {
        let chunk1 = json!({"model": "qwen3", "choices": []});
        let chunk2 = json!({
            "usage": {"prompt_tokens": 10},
            "timings": {"prompt_ms": 50.5}
        });

        let merged = merge_chunk(None, chunk1);
        let merged = merge_chunk(Some(merged), chunk2);

        assert!(merged.get("timings").is_some());
        assert_eq!(merged["timings"]["prompt_ms"].as_f64().unwrap(), 50.5);
    }

    // The shape a real backend emits: `delta` only, never `message`. The older merge
    // tests seed a first chunk that carries both, which no backend sends.
    fn realistic_tool_call_stream() -> &'static str {
        concat!(
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi \"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"there\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"t1\",\"type\":\"function\",\"function\":{\"name\":\"write\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\":\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"1}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"id\":\"c1\",\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":9}}\n\n",
            "data: [DONE]\n\n",
        )
    }

    #[test]
    fn delta_only_stream_accumulates_content_and_tool_calls() {
        let merged = parse_accumulated_sse(realistic_tool_call_stream()).unwrap();
        let msg = &merged["choices"][0]["message"];
        assert_eq!(msg["role"], json!("assistant"));
        assert_eq!(msg["content"], json!("Hi there"));
        assert_eq!(msg["tool_calls"][0]["id"], json!("t1"));
        assert_eq!(msg["tool_calls"][0]["function"]["name"], json!("write"));
        assert_eq!(msg["tool_calls"][0]["function"]["arguments"], json!("{\"a\":1}"));
        assert_eq!(merged["choices"][0]["finish_reason"], json!("tool_calls"));
        assert_eq!(merged["usage"]["completion_tokens"], json!(9));
    }

    // The point of the detect-only pass: a defect split across streamed fragments must
    // still be seen once the fragments are merged. Built from delta-only chunks.
    #[test]
    fn defect_split_across_streamed_fragments_is_detected_after_merge() {
        let fragments = [
            "{\"content\":\"x\",\"filePath\":\"/a/primes.pl\",",
            "\"filePath\"/b/primes.pl\"}",
        ];
        let mut sse = String::from(
            "data: {\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"t1\",\"type\":\"function\",\"function\":{\"name\":\"write\",\"arguments\":\"\"}}]}}]}\n\n",
        );
        for fragment in fragments {
            let chunk = json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "function": {"arguments": fragment}}]}}]});
            sse.push_str(&format!("data: {}\n\n", chunk));
        }
        sse.push_str("data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n");

        let merged = parse_accumulated_sse(&sse).unwrap();
        let detected = crate::fixes::create_default_registry().detect_fixes(&merged, None, "test");
        assert!(
            detected.iter().any(|name| name.contains("bad_filepath")),
            "merged stream must expose the defect to the fix predicates, got {:?} from {}",
            detected,
            merged
        );
    }

    #[test]
    fn test_parse_accumulated_sse_complete() {
        let sse = "data: {\"model\":\"qwen3\",\"choices\":[]}\n\
                   data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\
                   data: [DONE]\n";

        let result = parse_accumulated_sse(sse).unwrap();
        assert_eq!(result["model"].as_str().unwrap(), "qwen3");
        assert_eq!(result["usage"]["prompt_tokens"].as_u64().unwrap(), 10);
    }

    #[test]
    fn test_merge_chunk_accumulates_content() {
        let chunk1 = json!({
            "model": "test-model",
            "choices": [{"index": 0, "delta": {"content": "Hello"}, "message": {"content": "Hello"}}]
        });
        let chunk2 = json!({
            "choices": [{"index": 0, "delta": {"content": " World"}}]
        });

        let merged = merge_chunk(None, chunk1);
        let merged = merge_chunk(Some(merged), chunk2);

        assert_eq!(merged["choices"][0]["message"]["content"].as_str().unwrap(), "Hello World");
    }

    #[test]
    fn test_merge_chunk_seeds_choices_from_usage_only_first_chunk() {
        // (a) A first chunk without a choices key used to leave the envelope
        // with no slot list; every later delta was silently dropped.
        let acc = merge_chunk(None, json!({"id": "1", "usage": {"prompt_tokens": 5}}));
        let acc = merge_chunk(Some(acc), json!({"choices": [{"index": 0, "delta": {"content": "hi"}}]}));
        assert_eq!(acc["choices"][0]["message"]["content"].as_str(), Some("hi"));
    }

    #[test]
    fn test_merge_chunk_indexless_tail_extends_last_slot() {
        // (b) Backends that omit `index` on continuation fragments used to
        // have the arguments tail dropped, freezing arguments mid-JSON.
        let acc = merge_chunk(
            None,
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"name": "f", "arguments": "{\"a\""}}]}}]}),
        );
        let acc = merge_chunk(
            Some(acc),
            json!({"choices": [{"delta": {"tool_calls": [{"function": {"arguments": ":1}"}}]}}]}),
        );
        assert_eq!(
            acc["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"].as_str(),
            Some(r#"{"a":1}"#)
        );
    }

    #[test]
    fn test_merge_chunk_indexless_fragment_before_any_slot_opens_one() {
        let acc = merge_chunk(
            None,
            json!({"choices": [{"delta": {"tool_calls": [{"function": {"name": "f", "arguments": "{}"}}]}}]}),
        );
        assert_eq!(
            acc["choices"][0]["message"]["tool_calls"][0]["function"]["name"].as_str(),
            Some("f")
        );
        let acc = merge_chunk(
            Some(acc),
            json!({"choices": [{"delta": {"tool_calls": [{"function": {"arguments": " "}}]}}]}),
        );
        assert_eq!(
            acc["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"].as_str(),
            Some("{} ")
        );
    }

    #[test]
    fn test_merge_chunk_indexless_tail_targets_the_latest_call_slot() {
        let acc = merge_chunk(
            None,
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"name": "a", "arguments": "1"}}]}}]}),
        );
        let acc = merge_chunk(
            Some(acc),
            json!({"choices": [{"delta": {"tool_calls": [{"index": 1, "function": {"name": "b", "arguments": "2"}}]}}]}),
        );
        let acc = merge_chunk(
            Some(acc),
            json!({"choices": [{"delta": {"tool_calls": [{"function": {"arguments": "3"}}]}}]}),
        );
        let tool_calls = acc["choices"][0]["message"]["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls[0]["function"]["arguments"].as_str(), Some("1"));
        assert_eq!(tool_calls[1]["function"]["arguments"].as_str(), Some("23"));
    }

    #[test]
    fn test_merge_chunk_late_name_lands_on_its_slot() {
        // (c) Nameless opening slot, then split name fragments, then an
        // identical resend: set, append, skip - never drop, never duplicate.
        let acc = merge_chunk(
            None,
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": "{}"}}]}}]}),
        );
        let name_of = |acc: &serde_json::Value| {
            acc["choices"][0]["message"]["tool_calls"][0]["function"]["name"]
                .as_str()
                .unwrap_or("<MISSING>")
                .to_string()
        };
        assert_eq!(name_of(&acc), "<MISSING>");
        let acc = merge_chunk(
            Some(acc),
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"name": "tool_", "arguments": ""}}]}}]}),
        );
        assert_eq!(name_of(&acc), "tool_");
        let acc = merge_chunk(
            Some(acc),
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"name": "x", "arguments": ""}}]}}]}),
        );
        assert_eq!(name_of(&acc), "tool_x");
        let acc = merge_chunk(
            Some(acc),
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"name": "tool_x", "arguments": ""}}]}}]}),
        );
        assert_eq!(name_of(&acc), "tool_x");
    }

    #[test]
    fn test_detect_format_anthropic() {
        let sse = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude\"}}\n";
        assert_eq!(detect_format(sse), StreamFormat::Anthropic);

        let sse2 = "data: {\"type\":\"content_block_delta\",\"index\":0}\n";
        assert_eq!(detect_format(sse2), StreamFormat::Anthropic);
    }

    #[test]
    fn test_detect_format_openai() {
        let sse = "data: {\"model\":\"gpt-4\",\"choices\":[]}\n";
        assert_eq!(detect_format(sse), StreamFormat::OpenAI);
    }

    #[test]
    fn detect_format_reads_choices_as_openai_even_with_a_type_field() {
        // A backend that puts `type` on an OpenAI chunk used to be merged as Anthropic,
        // which silently corrupted every stat derived from the merged object.
        let sse = "data: {\"type\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n";
        assert_eq!(detect_format(sse), StreamFormat::OpenAI);
    }

    #[test]
    fn detect_format_does_not_call_an_unknown_type_field_anthropic() {
        let sse = "data: {\"type\":\"something_custom\",\"foo\":1}\n";
        assert_eq!(detect_format(sse), StreamFormat::Unknown);
    }

    #[test]
    fn detect_format_treats_legacy_completion_type_as_unknown() {
        // `completion` belongs to the retired text-completions API. Admitting it made
        // non-Anthropic streams merge down the Anthropic path and report garbage stats.
        let sse = "data: {\"type\":\"completion\",\"text\":\"hi\"}\n";
        assert_eq!(detect_format(sse), StreamFormat::Unknown);
    }

    #[test]
    fn test_merge_chunk_preserves_vllm_metrics() {
        let chunk1 = json!({"choices": [{"delta": {"content": "hi"}}]});
        let usage_chunk = json!({
            "choices": [],
            "usage": {"prompt_tokens": 61, "completion_tokens": 56},
            "metrics": {"time_to_first_token_ms": 103.0, "generation_time_ms": 562.0, "tokens_per_second": 84.1}
        });

        let merged = merge_chunk(None, chunk1);
        let merged = merge_chunk(Some(merged), usage_chunk);

        assert_eq!(merged["usage"]["completion_tokens"].as_u64().unwrap(), 56);
        assert_eq!(merged["metrics"]["generation_time_ms"].as_f64().unwrap(), 562.0);
        assert_eq!(merged["metrics"]["tokens_per_second"].as_f64().unwrap(), 84.1);
    }

    #[test]
    fn test_merge_chunk_null_metrics_does_not_replace_measured_metrics() {
        // vLLM sends `metrics: null` unless started with --enable-per-request-metrics.
        // Merging that over a measured block would erase a real measurement.
        let first = json!({
            "choices": [],
            "metrics": {"generation_time_ms": 562.0}
        });
        let later = json!({"choices": [], "usage": {"completion_tokens": 1}, "metrics": null});

        let merged = merge_chunk(None, first);
        let merged = merge_chunk(Some(merged), later);

        assert_eq!(merged["metrics"]["generation_time_ms"].as_f64().unwrap(), 562.0);
    }

    #[test]
    fn test_merge_chunk_null_metrics_is_not_introduced() {
        let merged = merge_chunk(
            Some(json!({"choices": [{"delta": {"content": "hi"}}]})),
            json!({"choices": [], "usage": {"completion_tokens": 1}, "metrics": null}),
        );
        assert!(merged.get("metrics").is_none(), "a null metrics block is not a measurement");
    }

    #[test]
    fn test_parse_anthropic_sse() {
        let sse = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-3\",\"stop_reason\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\nevent: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n";

        let result = parse_accumulated_sse(sse).unwrap();

        assert_eq!(result["model"].as_str().unwrap(), "claude-3");
        assert_eq!(result["stop_reason"].as_str().unwrap(), "end_turn");
        assert_eq!(result["usage"]["input_tokens"].as_u64().unwrap(), 10);
        assert_eq!(result["usage"]["output_tokens"].as_u64().unwrap(), 5);

        let content = result["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["text"].as_str().unwrap(), "Hello world");
    }

    #[test]
    fn test_merge_anthropic_with_thinking() {
        let sse = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-3\",\"usage\":{\"input_tokens\":100,\"output_tokens\":0}}}\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Let me think\"}}\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Answer\"}}\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":50}}\n";

        let result = parse_accumulated_sse(sse).unwrap();

        assert_eq!(result["model"].as_str().unwrap(), "claude-3");
        assert_eq!(result["usage"]["input_tokens"].as_u64().unwrap(), 100);
        assert_eq!(result["usage"]["output_tokens"].as_u64().unwrap(), 50);

        let content = result["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["thinking"].as_str().unwrap(), "Let me think");
        assert_eq!(content[1]["text"].as_str().unwrap(), "Answer");
    }
}

#[cfg(test)]
mod framing_tests {
    use super::*;

    fn frame_all(chunks: &[&[u8]]) -> (Vec<u8>, FramingState) {
        // Mirrors advance_stream: one take_framed per push.
        let mut st = FramingState::new();
        let mut emitted = Vec::new();
        for c in chunks {
            st.push(c);
            match st.take_framed() {
                Framed::Events(out) | Framed::ForceFlush(out) => emitted.extend_from_slice(&out),
                Framed::Nothing => {}
            }
        }
        (emitted, st)
    }

    #[test]
    fn split_mid_data() {
        let (emitted, st) = frame_all(&[b"data: hel", b"lo\n\n"]);
        assert_eq!(emitted, b"data: hello\n\n".to_vec());
        assert!(st.pending.is_empty());
    }

    #[test]
    fn split_inside_terminator() {
        let (emitted, st) = frame_all(&[b"data: x\n", b"\n"]);
        assert_eq!(emitted, b"data: x\n\n".to_vec());
        assert!(st.pending.is_empty());
    }

    #[test]
    fn complete_then_leading_newline() {
        let (emitted, st) = frame_all(&[b"data: x\n\n", b"\ndata: y\n\n"]);
        let mut expected = b"data: x\n\n".to_vec();
        expected.extend_from_slice(b"\ndata: y\n\n");
        assert_eq!(emitted, expected);
        assert!(st.pending.is_empty());
    }

    #[test]
    fn multiple_events_one_chunk() {
        let one = b"data: a\n\ndata: b\n\ndata: c\n\n";
        let (emitted, st) = frame_all(&[one]);
        assert_eq!(emitted, one.to_vec());
        assert!(st.pending.is_empty());
    }

    #[test]
    fn no_terminator_emits_nothing() {
        let (emitted, st) = frame_all(&[b"data: x", b"data: y"]);
        assert!(emitted.is_empty());
        assert_eq!(st.pending, b"data: xdata: y".to_vec());
    }

    #[test]
    fn crlf_events_frame_exactly() {
        // Plan acceptance: "data: x\r\n\r\ndata: y\r\n\r\n" yields exactly two
        // events. Baseline framed zero (Framed::Nothing, both events stuck in
        // pending - captured raw in task52 evidence).
        let (emitted, st) = frame_all(&[b"data: x\r\n\r\ndata: y\r\n\r\n"]);
        assert_eq!(emitted, b"data: x\n\ndata: y\n\n".to_vec());
        assert!(st.pending.is_empty());
        assert_eq!(count_double_newlines(&emitted), 2);
    }

    #[test]
    fn crlf_terminator_split_across_chunks() {
        // The CR arrives at the end of one chunk, its LF in the next; the
        // event must still frame, exactly once, with exactly one LF pair.
        let (emitted, st) = frame_all(&[b"data: x\r", b"\n\r", b"\ndata: y\r\n\r\n"]);
        assert_eq!(emitted, b"data: x\n\ndata: y\n\n".to_vec());
        assert!(st.pending.is_empty());
    }

    #[test]
    fn mixed_crlf_and_lf_frame() {
        let (emitted, mut st) = frame_all(&[b"data: a\r\n\ndata: b\n\r"]);
        // The trailing CR is held, not yet a break; the first event is out.
        assert_eq!(emitted, b"data: a\n\n".to_vec());
        assert_eq!(st.finish().unwrap(), b"data: b\n\n".to_vec());
    }

    #[test]
    fn lone_cr_is_a_line_break() {
        // SSE spec: a lone CR is a line terminator, so \r\r is a blank line.
        let (emitted, mut st) = frame_all(&[b"data: x\r\rdata: y\r\r"]);
        assert_eq!(emitted, b"data: x\n\n".to_vec());
        // The corpus's final CR is still held at stream end; finish() completes
        // the second event's blank line.
        let mut emitted = emitted;
        emitted.extend_from_slice(&st.finish().unwrap());
        assert_eq!(emitted, b"data: x\n\ndata: y\n\n".to_vec());
    }

    #[test]
    fn crlf_done_frames_and_signals_completion() {
        // A CRLF-framed [DONE] now reaches the completion detector as a framed
        // block (baseline: framing never emitted it, so detection never ran).
        let mut st = FramingState::new();
        st.push(b"data: [DONE]\r\n\r\n");
        let out = match st.take_framed() {
            Framed::Events(b) => b,
            other => panic!("expected framed event, got {:?}", std::mem::discriminant(&other)),
        };
        let mut tx = Some(tokio::sync::oneshot::channel::<StreamEnd>().0);
        assert!(detect_and_signal_completion(&out, &mut tx));
        assert!(tx.is_none());
    }

    #[test]
    fn held_cr_flushes_at_stream_end() {
        let mut st = FramingState::new();
        st.push(b"data: tail\r");
        assert!(matches!(st.take_framed(), Framed::Nothing));
        assert_eq!(st.finish().unwrap(), b"data: tail\n".to_vec());
        assert!(st.finish().is_none());
    }

    #[test]
    fn bom_passes_through_framing() {
        // No interpretation beyond framing: a UTF-8 BOM rides the bytes and
        // must not disturb the CRLF terminator.
        let (emitted, st) = frame_all(&[b"\xEF\xBB\xBFdata: x\r\n\r\n"]);
        assert_eq!(emitted, b"\xEF\xBB\xBFdata: x\n\n".to_vec());
        assert!(st.pending.is_empty());
    }

    #[test]
    fn crlf_corpus_reassembles_normalized() {
        // Property: any 2-way split of a CRLF corpus reassembles (modulo the
        // documented line-ending normalization) with no byte lost or added.
        fn normalize(v: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(v.len());
            let mut i = 0;
            while i < v.len() {
                if v[i] == b'\r' {
                    out.push(b'\n');
                    i += if v.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                } else {
                    out.push(v[i]);
                    i += 1;
                }
            }
            out
        }
        let corpus: Vec<u8> =
            b"data: {\"a\":1}\r\n\r\nevent: message_stop\rdata: {\"type\":\"message_stop\"}\r\n\r\n\r\n: ping\r\n\r\n".to_vec();
        let expected = normalize(&corpus);
        for i in 1..corpus.len() {
            let (mut emitted, mut st) = frame_all(&[&corpus[..i], &corpus[i..]]);
            if let Some(tail) = st.finish() {
                emitted.extend_from_slice(&tail);
            }
            assert_eq!(emitted, expected, "2-way CRLF split at {}", i);
        }
    }

    #[test]
    fn high_water_flush_without_terminator() {
        // (retargeted from crlf_not_a_terminator_but_high_water_flushes: its
        // fixture never contained a CR; CRLF now frames, this pins high-water.)
        let mut st = FramingState::new();
        let big = vec![b'a'; PENDING_HIGH_WATER + 10];
        st.push(&big);
        match st.take_framed() {
            Framed::ForceFlush(bytes) => assert_eq!(bytes.len(), PENDING_HIGH_WATER + 10),
            other => panic!("expected force flush, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn force_flush_resets_frontier() {
        // After a force flush the frontier describes bytes that no longer
        // exist; the next event must still be framed (and the frontier
        // invariant must not fire in debug builds).
        let mut st = FramingState::new();
        st.push(&vec![b'a'; PENDING_HIGH_WATER + 10]);
        match st.take_framed() {
            Framed::ForceFlush(bytes) => assert_eq!(bytes.len(), PENDING_HIGH_WATER + 10),
            other => panic!("expected force flush, got {:?}", std::mem::discriminant(&other)),
        }
        st.push(b"\n\ndata: x\n\n");
        match st.take_framed() {
            Framed::Events(out) => assert_eq!(out, b"\n\ndata: x\n\n"),
            other => panic!("expected events, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn comment_only_event() {
        let (emitted, st) = frame_all(&[b": ping\n\n"]);
        assert_eq!(emitted, b": ping\n\n".to_vec());
        assert!(st.pending.is_empty());
        let (evt, payloads) = split_event(b": ping\n");
        assert!(evt.is_none());
        assert!(payloads.is_empty());
    }

    #[test]
    fn data_without_space() {
        let (_evt, payloads) = split_event(b"data:{\"a\":1}\n");
        assert_eq!(payloads, vec!["{\"a\":1}".to_string()]);
    }

    #[test]
    fn utf8_multibyte_split() {
        let event = "data: \"héllo 🌍\"\n\n".as_bytes().to_vec();
        let mid = 12; // lands inside multibyte sequences
        let (emitted, st) = frame_all(&[&event[..mid], &event[mid..]]);
        assert_eq!(emitted, event);
        assert!(st.pending.is_empty());
    }

    fn signal_completion_events(bytes: &[u8], tx: &mut Option<tokio::sync::oneshot::Sender<StreamEnd>>) {
        detect_and_signal_completion(bytes, tx);
    }

    #[test]
    fn done_not_fooled_by_payload() {
        let payload = b"data: {\"content\":\"here is text: data: [DONE] inside\"}\n\n";
        let mut tx = Some(tokio::sync::oneshot::channel::<StreamEnd>().0);
        let (rx_keep, _rx) = tokio::sync::oneshot::channel::<StreamEnd>();
        drop(rx_keep);
        signal_completion_events(payload, &mut tx);
        assert!(tx.is_some(), "completion must NOT fire for [DONE] inside JSON content");
    }

    #[test]
    fn done_fires_once() {
        let mut tx = Some(tokio::sync::oneshot::channel::<StreamEnd>().0);
        signal_completion_events(b"data: [DONE]\n\n", &mut tx);
        assert!(tx.is_none(), "completion must fire for real [DONE]");
        signal_completion_events(b"data: [DONE]\n\n", &mut tx); // must not panic
    }

    #[test]
    fn message_stop_fires() {
        let mut tx = Some(tokio::sync::oneshot::channel::<StreamEnd>().0);
        signal_completion_events(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n", &mut tx);
        assert!(tx.is_none());
    }

    #[test]
    fn message_stop_in_content_does_not_fire() {
        let mut tx = Some(tokio::sync::oneshot::channel::<StreamEnd>().0);
        signal_completion_events(b"data: {\"text\":\"event: message_stop\"}\n\n", &mut tx);
        assert!(tx.is_some());
    }

    #[test]
    fn prefilter_matches_only_literal_marker_windows() {
        assert!(!mentions_bridge_event(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n"
        ));
        assert!(mentions_bridge_event(b"data: [DONE]\n\n"));
        assert!(mentions_bridge_event(
            b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        ));
        // CRLF framing keeps both markers literal in the bytes
        assert!(mentions_bridge_event(b"data: [DONE]\r\n\r\n"));
    }

    #[test]
    fn markerless_block_skips_parse_and_keeps_sender() {
        // Pay-for-what-you-use: the common delta block must not fire, and the
        // sender must survive for the block that actually carries the marker.
        let mut tx = Some(tokio::sync::oneshot::channel::<StreamEnd>().0);
        let fired = detect_and_signal_completion(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"counting: 1, 2,\"}}]}\n\n",
            &mut tx,
        );
        assert!(!fired);
        assert!(tx.is_some());
        // ...and the marker block that follows still fires
        let fired = detect_and_signal_completion(b"data: [DONE]\n\n", &mut tx);
        assert!(fired);
        assert!(tx.is_none());
    }

    #[test]
    fn prefilter_hit_inside_content_still_parses_honestly() {
        // Literal "[DONE" inside JSON string content passes the prefilter; the
        // parse must still refuse to fire - superset prefilter, honest parser.
        let mut tx = Some(tokio::sync::oneshot::channel::<StreamEnd>().0);
        let fired = detect_and_signal_completion(b"data: {\"text\":\"looks like [DONE] but is content\"}\n\n", &mut tx);
        assert!(!fired);
        assert!(tx.is_some());
    }

    #[test]
    fn finish_releases_unterminated_tail() {
        let mut st = FramingState::new();
        st.push(b"data: partial");
        assert!(matches!(st.take_framed(), Framed::Nothing));
        assert_eq!(st.finish().unwrap(), b"data: partial".to_vec());
        assert!(st.finish().is_none());
    }

    #[test]
    fn reassembly_property_exhaustive_splits() {
        let corpus: Vec<u8> =
            b"event: message_start\ndata: {\"a\":1}\n\ndata: b\n\ndata: {\"c\":\"d\\n\\ne\"}\n\n: ping\n\ndata: [DONE]\n\n"
                .to_vec();
        for i in 1..corpus.len() {
            let (emitted, st) = frame_all(&[&corpus[..i], &corpus[i..]]);
            let mut full = emitted.clone();
            let mut st2 = FramingState::new();
            st2.pending = st.pending.clone();
            if let Some(tail) = st2.finish() {
                full.extend_from_slice(&tail);
            }
            assert_eq!(full, corpus, "2-way split at {}", i);
        }
    }

    #[test]
    fn reassembly_property_single_chunk() {
        let corpus: Vec<u8> = b"data: a\n\ndata: b\n\ndata: [DONE]\n\n".to_vec();
        let (emitted, st) = frame_all(&[&corpus]);
        assert_eq!(emitted, corpus);
        assert!(st.pending.is_empty());
    }

    #[test]
    fn escaped_newlines_dont_split() {
        // \n\n INSIDE the JSON payload is escaped text (backslash-n), not real newlines
        let event = b"data: {\"t\":\"a\\n\\nb\"}\n\n";
        let (emitted, st) = frame_all(&[event]);
        assert_eq!(emitted, event.to_vec());
        assert!(st.pending.is_empty());
    }

    #[test]
    fn unparsed_payload_counting() {
        let (_e, good) = split_event(b"data: {\"ok\":true}\ndata: [DONE]\ndata: \n\n".as_ref().to_vec().as_slice());
        assert_eq!(count_unparsed_payloads(&good), 0);
        let (_e, bad) = split_event(b"data: {broken\n\n".as_ref().to_vec().as_slice());
        assert_eq!(count_unparsed_payloads(&bad), 1);
    }

    #[test]
    fn scan_resumes_after_drain() {
        // after draining event 1, the terminator split across the original
        // chunk boundary for event 2 must still be found
        let (mut emitted, _st) = frame_all(&[b"data: one\n\ndata: two\n", b"\ndata: three\n\n"]);
        emitted.extend_from_slice(&[]);
        let mut expected = b"data: one\n\n".to_vec();
        expected.extend_from_slice(b"data: two\n\n");
        expected.extend_from_slice(b"data: three\n\n");
        assert_eq!(emitted, expected);
    }

    struct TestErr(&'static str);
    impl std::fmt::Display for TestErr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    type TestStream = futures::stream::Iter<std::vec::IntoIter<Result<Bytes, TestErr>>>;

    /// Throwaway guard over an unshared node: framing tests exercise the stream,
    /// the parked claim only has to exist and drop with the state.
    fn test_guard() -> BackendGuard {
        let node = Arc::new(crate::backends::BackendNode {
            url: "http://test.invalid".to_string(),
            model: None,
            api_key: None,
            timeout_seconds: 300,
            http_client: reqwest::Client::new(),
            active_requests: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            strip_path_prefix: None,
            temperature: None,
            healthy: std::sync::atomic::AtomicBool::new(true),
            cooldown_until: std::sync::Mutex::new(std::time::Instant::now()),
        });
        BackendGuard::new(node)
    }

    fn pass_state(chunks: Vec<Result<Bytes, TestErr>>) -> StreamPassState<TestStream> {
        let (completion_tx, completion_rx) = tokio::sync::oneshot::channel::<StreamEnd>();
        let (activity_tx, activity_rx) = tokio::sync::watch::channel(());
        let _ = (completion_rx, activity_rx);
        StreamPassState {
            underlying: FusedStream {
                inner: futures::stream::iter(chunks),
                done: false,
            },
            framing: FramingState::new(),
            accumulated: Some(Arc::new(std::sync::Mutex::new(Vec::new()))),
            completion_tx: Some(completion_tx),
            activity_tx,
            pending_error: None,
            completion_signaled: false,
            _permit: None,
            _guard: test_guard(),
        }
    }

    /// Drive the pass-through to the first Err (or to stream end), returning
    /// the bytes emitted before it plus the error message.
    async fn drive(state: StreamPassState<TestStream>) -> (Vec<u8>, Option<String>) {
        let mut state = state;
        let mut out = Vec::new();
        while let Some((item, next)) = advance_stream(state).await {
            state = next;
            match item {
                Ok(bytes) => out.extend_from_slice(&bytes),
                Err(e) => return (out, Some(e.to_string())),
            }
        }
        (out, None)
    }

    #[tokio::test]
    async fn deferred_error_flushes_tail_then_errors() {
        let (out, err) = drive(pass_state(vec![
            Ok(Bytes::from_static(b"data: a\n\ndata: part")),
            Err(TestErr("connection reset")),
        ]))
        .await;
        assert_eq!(out, b"data: a\n\ndata: part".to_vec());
        assert!(err.is_some(), "the backend error must surface after the tail");
    }

    #[tokio::test]
    async fn empty_chunk_is_forwarded_and_pass_continues() {
        let (out, err) = drive(pass_state(vec![Ok(Bytes::new()), Ok(Bytes::from_static(b"data: a\n\n"))])).await;
        assert_eq!(out, b"data: a\n\n".to_vec());
        assert!(err.is_none());
    }

    #[tokio::test]
    async fn three_way_split_reassembles() {
        let (out, _) = drive(pass_state(vec![
            Ok(Bytes::from_static(b"data: he")),
            Ok(Bytes::from_static(b"ll")),
            Ok(Bytes::from_static(b"o\n\n")),
        ]))
        .await;
        assert_eq!(out, b"data: hello\n\n".to_vec());
    }

    #[tokio::test]
    async fn force_flush_forwards_bytes_and_next_event_frames() {
        let big = vec![b'a'; PENDING_HIGH_WATER + 10];
        let (out, _) = drive(pass_state(vec![
            Ok(Bytes::from(big.clone())),
            Ok(Bytes::from_static(b"\n\ndata: x\n\n")),
        ]))
        .await;
        let mut expected = big;
        expected.extend_from_slice(b"\n\ndata: x\n\n");
        assert_eq!(out, expected, "no byte may be lost across a force flush");
    }

    #[tokio::test]
    async fn error_after_completion_ends_gracefully() {
        let mut state = pass_state(vec![
            Ok(Bytes::from_static(b"data: [DONE]\n\n")),
            Err(TestErr("connection reset")),
        ]);
        let mut out = Vec::new();
        let mut err = None;
        while let Some((item, next)) = advance_stream(state).await {
            state = next;
            match item {
                Ok(bytes) => out.extend_from_slice(&bytes),
                Err(e) => {
                    err = Some(e.to_string());
                    break;
                }
            }
        }
        assert_eq!(out, b"data: [DONE]\n\n".to_vec());
        assert!(err.is_none(), "an error after [DONE] must not abort a complete transfer");
    }
}
