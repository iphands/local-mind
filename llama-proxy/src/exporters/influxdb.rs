//! InfluxDB v2 metrics exporter

use async_trait::async_trait;

use super::{ExportError, MetricsExporter};
use crate::stats::RequestMetrics;

#[cfg(feature = "influxdb")]
mod worker;
#[cfg(feature = "influxdb")]
use worker::{SendOutcome, WriterQueue, REQUEST_TIMEOUT};

/// InfluxDB v2 exporter configuration
#[derive(Debug, Clone)]
pub struct InfluxDbConfig {
    pub url: String,
    pub org: String,
    pub bucket: String,
    pub token: String,
}

/// The single honest answer for "requested while compiled out". One string for
/// both gates (config-time [`InfluxDbExporter::from_config`] and the export
/// backstop) so no path can downgrade it to a silent success.
#[cfg(not(feature = "influxdb"))]
fn feature_off_error() -> ExportError {
    ExportError::Config("influxdb enabled in config but binary built without the influxdb feature".to_string())
}

/// InfluxDB v2 metrics exporter
pub struct InfluxDbExporter {
    #[cfg(feature = "influxdb")]
    queue: WriterQueue,
    #[cfg(not(feature = "influxdb"))]
    _phantom: (),
}

/// Check the destination URL before handing it to influxdb2.
///
/// `influxdb2::Client::new` PANICS on an unparseable url (its builder does
/// `Url::parse(..).unwrap_or_else(|_| panic!(..))`), which would take the
/// process down at startup and make every caller's `Err` arm unreachable.
/// The load-time gate lives in config validation (F12-R2); this is the
/// last-line checked parse so construction degrades to a typed
/// [`ExportError::Config`] no matter how a config reached this point.
#[cfg(feature = "influxdb")]
fn checked_client_url(url: &str) -> Result<(), ExportError> {
    let parsed = url::Url::parse(url).map_err(|e| ExportError::Config(format!("influxdb.url is not a valid URL: {e}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ExportError::Config(format!(
            "influxdb.url must be an http or https URL: {url}"
        )));
    }
    Ok(())
}

/// Tag values rewritten at the wire boundary. Reconciliation number for the
/// "sanitized, not skipped" policy of [`tag_value`].
#[cfg(feature = "influxdb")]
pub(crate) static TAG_SANITIZE_TOTAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// WARN interval for tag sanitization - same counter-exact/log-sampled split
/// as the exporter's skip filter, because a hostile backend would flood.
#[cfg(feature = "influxdb")]
const TAG_SANITIZE_WARN_INTERVAL: u64 = 100;

/// Neutralize line-protocol control characters in a tag value.
///
/// influxdb2 0.5 escapes only `,`, `=` and ` ` in tag values
/// (`TAG_VALUE_DELIMITERS`); `\n` and `\r` pass through UNESCAPED, so a
/// response-sourced value like `"m\nforged_measurement 1"` forges a second
/// measurement line into the operator's bucket - remotely writable through
/// the model the backend echoes. Policy: REPLACE every control char (<0x20
/// plus DEL) and the three delimiters with `_`, keep the sample, count it in
/// [`TAG_SANITIZE_TOTAL`] and WARN 1-in-100 (first always). Skipping the
/// whole sample would destroy a valid measurement and hand a hostile backend
/// a telemetry-off switch; the delimiters are replaced although influxdb2
/// backslash-escapes them so tag cardinality never depends on hostile
/// quoting. The control chars are what the injection proof turns on; the
/// replacement never widens the value.
#[cfg(feature = "influxdb")]
fn tag_value<'a>(field: &'static str, raw: &'a str) -> std::borrow::Cow<'a, str> {
    fn is_unsafe(ch: char) -> bool {
        (ch as u32) < 0x20 || ch == '\u{7f}' || matches!(ch, ',' | '=' | ' ')
    }
    if !raw.chars().any(is_unsafe) {
        return std::borrow::Cow::Borrowed(raw);
    }
    let sanitized: String = raw.chars().map(|ch| if is_unsafe(ch) { '_' } else { ch }).collect();
    let total = TAG_SANITIZE_TOTAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if total == 1 || total.is_multiple_of(TAG_SANITIZE_WARN_INTERVAL) {
        tracing::warn!(
            tag = field,
            sanitized_total = total,
            "influxdb tag value carried line-protocol control characters or delimiters - \
             replaced with '_' (sample kept, counted in tag sanitize total; 1 in 100 logs)"
        );
    }
    std::borrow::Cow::Owned(sanitized)
}

impl InfluxDbExporter {
    /// Create a new InfluxDB exporter. The bounded writer thread starts here
    /// and here only - one per exporter, for the exporter's whole life.
    #[cfg(feature = "influxdb")]
    pub fn new(config: InfluxDbConfig) -> Result<Self, ExportError> {
        checked_client_url(&config.url)?;
        let client = influxdb2::Client::new(&config.url, &config.org, &config.token);
        let queue = WriterQueue::spawn(client, config, REQUEST_TIMEOUT);

        Ok(Self { queue })
    }

    /// Test-only constructor pinning a short per-attempt write timeout, so the
    /// stalled-endpoint test proves the timeout drives the retry chain instead
    /// of sleeping out the production 10 s window.
    #[cfg(all(test, feature = "influxdb"))]
    pub(crate) fn new_with_request_timeout(
        config: InfluxDbConfig,
        request_timeout: std::time::Duration,
    ) -> Result<Self, ExportError> {
        checked_client_url(&config.url)?;
        let client = influxdb2::Client::new(&config.url, &config.org, &config.token);
        let queue = WriterQueue::spawn(client, config, request_timeout);

        Ok(Self { queue })
    }

    #[cfg(not(feature = "influxdb"))]
    pub fn new(_config: InfluxDbConfig) -> Result<Self, ExportError> {
        Ok(Self { _phantom: () })
    }

    /// The queue/worker behind this exporter: counters and lifecycle. Public
    /// so tests pin the overflow policy and the drain-exit contract instead
    /// of reimplementing the queue shape.
    #[cfg(feature = "influxdb")]
    pub fn writer(&self) -> &WriterQueue {
        &self.queue
    }

    /// Create from app config.
    ///
    /// With the `influxdb` feature compiled out, `enabled: true` is a hard
    /// error (task 80 [A-M9]): a config that demands InfluxDB against a binary
    /// that cannot speak it must fail loudly at construction, never settle for
    /// a stub whose `export()` would report success while dropping every
    /// sample. `enabled: false` constructs a live-but-inert exporter; the
    /// export backstop below keeps that honest even if one is hand-built.
    pub fn from_config(config: &crate::config::InfluxDbConfig) -> Result<Self, ExportError> {
        #[cfg(not(feature = "influxdb"))]
        if config.enabled {
            return Err(feature_off_error());
        }
        Self::new(InfluxDbConfig {
            url: config.url.clone(),
            org: config.org.clone(),
            bucket: config.bucket.clone(),
            token: config.token.clone(),
        })
    }
}

/// Build the `llama_request` point for one sample. Pure so the wire shape is
/// testable without a network: an absent token count (`None`) omits its field
/// entirely rather than writing a fabricated 0 into the aggregate.
#[cfg(feature = "influxdb")]
fn build_data_point(metrics: &RequestMetrics) -> Result<Option<influxdb2::models::DataPoint>, ExportError> {
    // Unrepresentable nanosecond timestamp (chrono i64 overflow: years ~2262+ and
    // pre-1677) => skip the point entirely. Epoch 0 is a real, wrong timestamp;
    // a skipped point is recoverable ignorance, a fabricated one is silent
    // corruption of the time series (task 83 [A-L8]).
    let Some(ts_nanos) = metrics.timestamp.timestamp_nanos_opt() else {
        return Ok(None);
    };
    use influxdb2::models::DataPoint;

    let mut builder = DataPoint::builder("llama_request")
        .tag("model", tag_value("model", &metrics.model))
        .tag("streaming", metrics.streaming.to_string())
        .tag("finish_reason", tag_value("finish_reason", &metrics.finish_reason))
        .tag("stream_end", metrics.stream_end_label());

    if let Some(ref client_id) = metrics.client_id {
        // Shipped as a tag straight from a request header once a producer
        // fills the field, so it gets the same last-line treatment as the
        // response-sourced tags, whatever feeds it later.
        builder = builder.tag("client_id", tag_value("client_id", client_id.as_str()));
    }

    if let Some(ref conv_id) = metrics.conversation_id {
        builder = builder.tag("conversation_id", tag_value("conversation_id", conv_id.as_str()));
    }

    if let Some(ref group_name) = metrics.group_name {
        // Config-sourced, so it gets the same last-line treatment as every
        // other tag: a group_name like "prod\nllama_request,model=x 1" would
        // otherwise forge a second measurement line (F12 R7, M8 residual).
        builder = builder.tag("group_name", tag_value("group_name", group_name.as_str()));
    }

    let mut point = builder
        .field("total_tokens", metrics.total_tokens as f64)
        .field("prompt_tps", metrics.prompt_tps)
        .field("total_tps", metrics.total_tps)
        .field("has_timing_split", metrics.has_timing_split)
        .field("generation_tps", metrics.generation_tps)
        .field("prompt_ms", metrics.prompt_ms)
        .field("generation_ms", metrics.generation_ms)
        .field("duration_ms", metrics.duration_ms)
        .field("stream_incomplete", metrics.stream_incomplete)
        .field("input_len", metrics.input_len as f64)
        .field("output_len", metrics.output_len as f64);

    if let Some(prompt) = metrics.prompt_tokens {
        point = point.field("prompt_tokens", prompt as f64);
    }
    if let Some(completion) = metrics.completion_tokens {
        point = point.field("completion_tokens", completion as f64);
    }

    // Add extended token details if present (Opencode/Copilot extensions)
    if let Some(reasoning) = metrics.reasoning_tokens {
        point = point.field("reasoning_tokens", reasoning as f64);
    }
    if let Some(accepted) = metrics.accepted_prediction_tokens {
        point = point.field("accepted_prediction_tokens", accepted as f64);
    }
    if let Some(rejected) = metrics.rejected_prediction_tokens {
        point = point.field("rejected_prediction_tokens", rejected as f64);
    }

    // Context window metrics
    if let Some(ctx_used) = metrics.context_used {
        point = point.field("context_used", ctx_used as f64);
    }
    if let Some(ctx_total) = metrics.context_total {
        point = point.field("context_total", ctx_total as f64);
    }
    if let Some(ctx_pct) = metrics.context_percent {
        point = point.field("context_percent", ctx_pct);
    }

    point
        .timestamp(ts_nanos)
        .build()
        .map(Some)
        .map_err(|e| ExportError::Write(format!("Failed to build data point: {}", e)))
}

#[async_trait]
impl MetricsExporter for InfluxDbExporter {
    /// Hand the sample to the bounded queue and return immediately - the
    /// request path never waits on InfluxDB, and overflow refuses instead of
    /// growing (see [`worker`]).
    #[cfg(feature = "influxdb")]
    async fn export(&self, metrics: &RequestMetrics) -> Result<(), ExportError> {
        match self.queue.enqueue(metrics.clone()) {
            SendOutcome::Queued => Ok(()),
            SendOutcome::Dropped => Ok(()),
            SendOutcome::ShutDown => Err(ExportError::Other("exporter shut down".to_string())),
        }
    }

    #[cfg(not(feature = "influxdb"))]
    async fn export(&self, _metrics: &RequestMetrics) -> Result<(), ExportError> {
        Err(feature_off_error())
    }

    /// Blocks until every sample accepted before this call is delivered or
    /// exhaustion-dropped; an already-shut-down exporter drained at its exit.
    #[cfg(feature = "influxdb")]
    async fn flush(&self) -> Result<(), ExportError> {
        self.queue.flush()
    }

    #[cfg(feature = "influxdb")]
    async fn shutdown(&self) -> Result<(), ExportError> {
        self.queue.shutdown();
        Ok(())
    }

    fn name(&self) -> &str {
        "influxdb"
    }
}

#[cfg(all(test, feature = "influxdb"))]
mod wire_tests {
    use super::build_data_point;
    use crate::stats::RequestMetrics;
    use influxdb2::models::WriteDataPoint;

    /// Serialize the real point through influxdb2's own line-protocol writer.
    fn line(metrics: &RequestMetrics) -> String {
        let point = build_data_point(metrics).unwrap().unwrap();
        let mut buf = Vec::new();
        point.write_data_point_to(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn measurable() -> RequestMetrics {
        let mut m = RequestMetrics::new();
        m.total_tokens = 150;
        m.total_tps = 125.0;
        m
    }

    #[test]
    fn absent_token_counts_omit_their_fields() {
        let line = line(&measurable());
        assert!(!line.contains("prompt_tokens"), "fabricated field: {line}");
        assert!(!line.contains("completion_tokens"), "fabricated field: {line}");
        assert!(line.contains("total_tokens=150"), "measured total kept: {line}");
    }

    #[test]
    fn representable_timestamp_is_written_verbatim() {
        let mut m = measurable();
        m.timestamp = chrono::DateTime::parse_from_rfc3339("2026-09-21T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let nanos = m.timestamp.timestamp_nanos_opt().unwrap();
        let line = line(&m);
        assert!(
            line.trim_end().ends_with(&format!(" {nanos}")),
            "the point's own timestamp belongs on the wire: {line}"
        );
    }

    #[test]
    fn unrepresentable_timestamp_builds_no_point() {
        // chrono's i64-ns window ends in 2262; the baseline (@ f1efa80) wrote a
        // fabricated epoch-0 line for exactly this sample.
        let mut m = measurable();
        m.timestamp = chrono::DateTime::parse_from_rfc3339("2300-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(
            build_data_point(&m).unwrap().is_none(),
            "skip the point - never fabricate a timestamp"
        );
    }

    #[test]
    fn present_token_counts_are_written() {
        let mut m = measurable();
        m.prompt_tokens = Some(100);
        m.completion_tokens = Some(0);
        let line = line(&m);
        assert!(line.contains("prompt_tokens=100"), "got: {line}");
        assert!(line.contains("completion_tokens=0"), "measured zero written: {line}");
    }

    /// big-fix F12-R4 [MAJOR 8]: `model` and `finish_reason` arrive VERBATIM
    /// from the backend response body, and influxdb2 0.5 escapes only
    /// `,`/`=`/` ` in tag values — `TAG_VALUE_DELIMITERS` (data_point.rs:312)
    /// never touches `\n`, so the raw newline reached the line-protocol body
    /// and the backend forged a second measurement into the operator's
    /// bucket. Red baseline captured the 2-physical-line wire raw.
    /// Serialize the tests that sanitize tags. The counter is process-global
    /// and these tests run on parallel harness threads, so an exact delta is
    /// only assertable while the sanitizing tests take turns (no other test
    /// in the binary reaches `tag_value` with unsafe input).
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn sanitizer_turn() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn newline_in_model_cannot_forge_a_second_measurement_line() {
        let _turn = sanitizer_turn();
        let mut m = measurable();
        m.model = "m\nforged_measurement 1".to_string();
        let wire = line(&m);
        let body = wire.trim_end();
        assert_eq!(
            body.lines().count(),
            1,
            "one sample must be ONE physical line, got {}:\n{body}",
            body.lines().count()
        );
        assert!(
            body.starts_with("llama_request,"),
            "the measurement name is untouched: {body}"
        );
        assert!(
            body.contains("model=m_forged_measurement_1,"),
            "control chars and delimiters replace to '_': {body}"
        );
    }

    #[test]
    fn carriage_return_and_raw_control_chars_are_neutralized() {
        let _turn = sanitizer_turn();
        let mut m = measurable();
        m.model = "a\r\nb\tc\u{1}d".to_string();
        let body = line(&m).trim_end().to_string();
        assert_eq!(body.lines().count(), 1, "got:\n{body}");
        assert!(body.contains("model=a__b_c_d,"), "got: {body}");
    }

    #[test]
    fn finish_reason_header_and_conversation_tags_are_sanitized() {
        let _turn = sanitizer_turn();
        let mut m = measurable();
        m.finish_reason = "stop\nforged 2".to_string();
        m.client_id = Some("cli\rent".to_string());
        m.conversation_id = Some("conv=1,x 3".to_string());
        let body = line(&m).trim_end().to_string();
        assert_eq!(body.lines().count(), 1, "got:\n{body}");
        assert!(body.contains("finish_reason=stop_forged_2,"), "got: {body}");
        assert!(body.contains("client_id=cli_ent,"), "got: {body}");
        assert!(body.contains("conversation_id=conv_1_x_3,"), "got: {body}");
    }

    #[test]
    fn clean_tag_values_pass_through_byte_identical() {
        let mut m = measurable();
        m.model = "Qwen3-14B-128K-Q3_K_S.gguf".to_string();
        m.finish_reason = "tool_calls".to_string();
        let body = line(&m).trim_end().to_string();
        assert!(body.contains("model=Qwen3-14B-128K-Q3_K_S.gguf,"), "got: {body}");
        assert!(body.contains("finish_reason=tool_calls,"), "got: {body}");
    }

    /// The keep-the-sample policy is reconciled by a counter, not by a
    /// whisper: every rewritten tag value increments it. The counter is
    /// process-global, so this test takes the sanitizer_turn() serialization
    /// shared by every sanitizing test in the binary (the exact-delta
    /// validity condition).
    #[test]
    fn sanitized_tags_are_counted_not_silently_rewritten() {
        let _turn = sanitizer_turn();
        let before = super::TAG_SANITIZE_TOTAL.load(std::sync::atomic::Ordering::SeqCst);
        let mut m = measurable();
        m.model = "m\nx".to_string();
        m.finish_reason = "a b".to_string();
        let _ = line(&m);
        let after = super::TAG_SANITIZE_TOTAL.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(after - before, 2, "two rewritten tag values must count as two");

        let mut clean = measurable();
        clean.model = "plain-model".to_string();
        clean.finish_reason = "stop".to_string();
        let _ = line(&clean);
        assert_eq!(
            super::TAG_SANITIZE_TOTAL.load(std::sync::atomic::Ordering::SeqCst),
            after,
            "a clean sample must not touch the counter"
        );
    }

    /// big-fix F12 R7 [M8 residual]: `group_name` was the ONE tag not routed
    /// through `tag_value`, so the config-sourced vector round 1 named
    /// (`"prod\nllama_request,model=x 1"`) still forged a second
    /// line-protocol measurement into the operator's bucket. Red baseline
    /// captured the 2-physical-line wire and the un-bumped counter.
    #[test]
    fn group_name_tag_is_sanitized_like_every_other_tag() {
        let _turn = sanitizer_turn();
        let before = super::TAG_SANITIZE_TOTAL.load(std::sync::atomic::Ordering::SeqCst);
        let mut m = measurable();
        m.group_name = Some("prod\nllama_request,model=x 1".to_string());
        let body = line(&m).trim_end().to_string();
        assert_eq!(
            body.lines().count(),
            1,
            "one sample must be ONE physical line, got {}:\n{body}",
            body.lines().count()
        );
        assert!(
            body.contains("group_name=prod_llama_request_model_x_1"),
            "control chars and delimiters replace to '_': {body}"
        );
        let after = super::TAG_SANITIZE_TOTAL.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(after - before, 1, "the rewritten group_name must count");
    }
}

#[cfg(all(test, feature = "influxdb"))]
mod queue_tests {
    //! Task 81 acceptance: fixed spawn count, bounded connections, overflow
    //! policy, retries, drain-exit join, clean post-shutdown errors.
    //!
    //! The fake InfluxDB runs on its OWN OS thread with blocking std IO: the
    //! worker's flush() parks the calling thread by contract, so a fake living
    //! on the test's current_thread runtime would deadlock against it. A
    //! thread-per-fake has no runtime to starve.

    use super::worker::CAPACITY;
    use super::{InfluxDbConfig, InfluxDbExporter, MetricsExporter};
    use crate::stats::RequestMetrics;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[derive(Clone)]
    struct FakeInflux {
        url: String,
        conns: Arc<AtomicU64>,
        bodies: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl FakeInflux {
        fn spawn(mode: &'static str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let fake = Self {
                url: format!("http://{addr}"),
                conns: Arc::new(AtomicU64::new(0)),
                bodies: Arc::new(std::sync::Mutex::new(Vec::new())),
            };
            let (conns, bodies) = (Arc::clone(&fake.conns), Arc::clone(&fake.bodies));
            std::thread::spawn(move || {
                for conn in listener.incoming() {
                    let Ok(mut sock) = conn else { break };
                    conns.fetch_add(1, Ordering::Relaxed);
                    let bodies = Arc::clone(&bodies);
                    std::thread::spawn(move || {
                        let _ = serve_conn(mode, &mut sock, &bodies);
                    });
                }
            });
            fake
        }

        fn exporter(&self) -> InfluxDbExporter {
            InfluxDbExporter::new(InfluxDbConfig {
                url: self.url.clone(),
                org: "o".into(),
                bucket: "b".into(),
                token: "t".into(),
            })
            .unwrap()
        }
    }

    fn serve_conn(mode: &str, sock: &mut TcpStream, bodies: &Arc<std::sync::Mutex<Vec<String>>>) -> std::io::Result<()> {
        if mode == "stalled" {
            // Park, then let the connection die so no fake thread outlives
            // the test binary's useful work.
            sock.set_read_timeout(Some(Duration::from_secs(1)))?;
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut sink = [0u8; 4096];
            while Instant::now() < deadline {
                match sock.read(&mut sink) {
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(_) => break,
                }
            }
            return Ok(());
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = sock.read(&mut chunk)?;
            if n == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let header_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
        let mut body: Vec<u8> = Vec::new();
        if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
            let mut rest = buf.drain(header_end..).collect::<Vec<u8>>();
            'chunks: loop {
                loop {
                    if rest.starts_with(b"0\r\n") {
                        break 'chunks;
                    }
                    match parse_chunk(&rest) {
                        Some((data_start, next)) => {
                            body.extend_from_slice(&rest[data_start..next]);
                            rest.drain(..next);
                        }
                        None => break,
                    }
                }
                let n = sock.read(&mut chunk)?;
                if n == 0 {
                    break;
                }
                rest.extend_from_slice(&chunk[..n]);
            }
        } else {
            let declared: usize = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap_or(0))
                })
                .unwrap_or(0);
            body = buf[header_end..].to_vec();
            while body.len() < declared {
                let n = sock.read(&mut chunk)?;
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..n]);
            }
        }
        bodies.lock().unwrap().push(String::from_utf8_lossy(&body).into_owned());
        let status = if mode == "500" {
            "HTTP/1.1 500 Internal Server Error"
        } else {
            "HTTP/1.1 204 No Content"
        };
        sock.write_all(format!("{status}\r\ncontent-length: 0\r\n\r\n").as_bytes())?;
        Ok(())
    }

    /// One complete HTTP chunk at the front of `buf`: (data_start, next_start).
    fn parse_chunk(buf: &[u8]) -> Option<(usize, usize)> {
        let line_end = buf.windows(2).position(|w| w == b"\r\n")?;
        let size = usize::from_str_radix(std::str::from_utf8(&buf[..line_end]).ok()?, 16).ok()?;
        let data_start = line_end + 2;
        let next = data_start + size + 2;
        if buf.len() < next {
            return None;
        }
        Some((data_start, next))
    }

    /// Stalled-fake teardown: bounded-budget shutdown only - the join budget
    /// IS the behavior under test (shutdown is never wedged by a stuck write).
    struct Guard {
        exporter: InfluxDbExporter,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            self.exporter.writer().shutdown_after(Duration::from_millis(50));
        }
    }

    fn sample(model: &str) -> RequestMetrics {
        let mut m = RequestMetrics::new();
        m.model = model.to_string();
        m.total_tokens = 150;
        m.total_tps = 125.0;
        m
    }

    #[tokio::test(flavor = "current_thread")]
    async fn overflow_refuses_past_capacity_without_blocking_the_caller() {
        let fake = FakeInflux::spawn("stalled");
        let guard = Guard {
            exporter: fake.exporter(),
        };
        let exporter = &guard.exporter;

        exporter.export(&sample("head-of-queue")).await.unwrap();
        for i in 0..CAPACITY + 8 {
            exporter
                .export(&sample(&format!("flood{i}")))
                .await
                .expect("export() must never fail on a full queue, only refuse");
        }

        let dropped = exporter.writer().dropped_total();
        assert!(dropped >= 1, "queue past {CAPACITY} must refuse-and-count");
        assert!(dropped <= 9, "refusals cannot exceed the overshoot, got {dropped}");
        assert_eq!(
            exporter.writer().retries_total(),
            0,
            "the worker is parked on the stalled head - it retried nothing yet"
        );
        assert_eq!(
            exporter.writer().dropped_total(),
            dropped,
            "the refusal count is a stable policy fact, not a race"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stalled_shutdown_proceeds_on_its_budget() {
        let fake = FakeInflux::spawn("stalled");
        let exporter = fake.exporter();
        exporter.export(&sample("wedged")).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let started = std::time::Instant::now();
        exporter.writer().shutdown_after(Duration::from_millis(50));
        let took = started.elapsed();
        assert!(
            took < Duration::from_secs(2),
            "shutdown must return on its 50 ms budget, took {took:?}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dead_endpoint_retries_three_times_then_counts_the_drop() {
        let exporter = InfluxDbExporter::new(InfluxDbConfig {
            url: "http://127.0.0.1:1".into(),
            org: "o".into(),
            bucket: "b".into(),
            token: "t".into(),
        })
        .unwrap();
        exporter.export(&sample("retry-me")).await.unwrap();
        exporter.writer().flush().unwrap();

        assert_eq!(exporter.writer().retries_total(), 3, "100/500/2000 ms");
        assert_eq!(exporter.writer().dropped_total(), 1);
        exporter.writer().shutdown();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn http_500_answers_also_drive_the_full_retry_chain() {
        let fake = FakeInflux::spawn("500");
        let exporter = fake.exporter();
        exporter.export(&sample("500-me")).await.unwrap();
        exporter.writer().flush().unwrap();

        assert_eq!(exporter.writer().retries_total(), 3);
        assert_eq!(exporter.writer().dropped_total(), 1);
        assert_eq!(
            exporter.writer().retries_total(),
            3,
            "retries stop at the third - no fifth attempt exists"
        );
        exporter.writer().shutdown();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn flush_drains_delivered_samples_and_join_is_clean() {
        let fake = FakeInflux::spawn("204");
        let exporter = fake.exporter();
        for i in 0..5 {
            exporter.export(&sample(&format!("delivered{i}"))).await.unwrap();
        }
        exporter.writer().flush().unwrap();

        let bodies = fake.bodies.lock().unwrap().join("");
        for i in 0..5 {
            assert!(
                bodies.contains(&format!("model=delivered{i}")),
                "sample {i} missing from the wire:\n{bodies}"
            );
        }
        assert_eq!(exporter.writer().dropped_total(), 0);
        exporter.writer().shutdown();
        let conns = fake.conns.load(Ordering::Relaxed);
        assert!(
            conns <= 8,
            "one writer thread kept the connection count bounded, got {conns} for 5 samples"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_drains_the_queue_before_the_join_returns() {
        let fake = FakeInflux::spawn("204");
        let exporter = fake.exporter();
        for i in 0..3 {
            exporter.export(&sample(&format!("bye{i}"))).await.unwrap();
        }
        exporter.writer().shutdown();

        let bodies = fake.bodies.lock().unwrap().join("");
        for i in 0..3 {
            assert!(
                bodies.contains(&format!("model=bye{i}")),
                "shutdown discarded an accepted sample:\n{bodies}"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unrepresentable_timestamp_is_skipped_never_epoch_zero() {
        // Baseline wrongness @ f1efa80: this sample wrote a real epoch-0 line
        // (`timestamp_nanos_opt().unwrap_or(0)`). Now the point is skipped:
        // counted, debugged, and NEVER on the wire.
        let fake = FakeInflux::spawn("204");
        let exporter = fake.exporter();
        let mut m = sample("far-future");
        m.timestamp = chrono::DateTime::parse_from_rfc3339("2300-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        exporter.export(&m).await.unwrap();
        exporter.writer().flush().unwrap();

        assert_eq!(
            fake.bodies.lock().unwrap().len(),
            0,
            "an unrepresentable timestamp must skip the point, not fabricate epoch 0"
        );
        assert_eq!(exporter.writer().dropped_total(), 1, "the skip is counted");
        exporter.writer().shutdown();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stalled_writes_are_cut_by_the_request_timeout() {
        // The stalled fake accepts and never answers. Without the per-attempt
        // timeout (baseline) the worker parked forever; with it, each attempt
        // dies at the timeout, the retry chain runs to exhaustion, and flush
        // returns - bounded by timeouts, not by sleeps.
        let fake = FakeInflux::spawn("stalled");
        let exporter = InfluxDbExporter::new_with_request_timeout(
            InfluxDbConfig {
                url: fake.url.clone(),
                org: "o".into(),
                bucket: "b".into(),
                token: "t".into(),
            },
            std::time::Duration::from_millis(50),
        )
        .unwrap();

        let started = std::time::Instant::now();
        exporter.export(&sample("stalled-write")).await.unwrap();
        exporter.writer().flush().unwrap();
        let took = started.elapsed();

        assert!(
            took < std::time::Duration::from_secs(15),
            "4 attempts x 50 ms timeouts + 2.6 s backoff must finish in seconds, took {took:?}"
        );
        assert_eq!(exporter.writer().retries_total(), 3);
        assert_eq!(exporter.writer().dropped_total(), 1);
        exporter.writer().shutdown();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn post_shutdown_export_reports_shut_down_cleanly() {
        let exporter = InfluxDbExporter::new(InfluxDbConfig {
            url: "http://127.0.0.1:1".into(),
            org: "o".into(),
            bucket: "b".into(),
            token: "t".into(),
        })
        .unwrap();
        exporter.writer().shutdown();

        let err = exporter
            .export(&sample("late"))
            .await
            .expect_err("a shut-down exporter must not accept");
        assert_eq!(err.to_string(), "exporter shut down");
        exporter.writer().flush().unwrap();
        exporter.writer().shutdown();
        let second = exporter.export(&sample("later")).await;
        assert!(second.is_err(), "second shutdown stays idempotent");
    }
}

/// big-fix F12-R4 [MAJOR 9]: `influxdb2::Client::new` panics
/// (`Url::parse(..).unwrap_or_else(panic)`) on an unparseable url, so an
/// unvalidated `influxdb.url` took the process down at startup and main's
/// `Err -> warn` arm was unreachable. Construction now checks the URL first
/// and returns `ExportError::Config`. Feature-gated: without the feature
/// there is no client and no panic to guard.
#[cfg(all(test, feature = "influxdb"))]
mod url_gate_tests {
    use super::{ExportError, InfluxDbConfig, InfluxDbExporter};

    fn config_with_url(url: &str) -> InfluxDbConfig {
        InfluxDbConfig {
            url: url.to_string(),
            org: "o".into(),
            bucket: "b".into(),
            token: "t".into(),
        }
    }

    #[test]
    fn unparseable_or_unusable_urls_return_config_errors_never_panic() {
        for url in ["", "not-a-url", "http://", "mailto:ops@example.com", "ftp://influx:8086"] {
            let outcome = std::panic::catch_unwind(|| InfluxDbExporter::new(config_with_url(url)));
            let err = match outcome {
                Ok(Err(e)) => e,
                Ok(Ok(_)) => panic!("url {url:?} must not construct an exporter"),
                Err(_) => panic!("url {url:?} must return Err, not panic through Client::new"),
            };
            assert!(
                matches!(err, ExportError::Config(_)),
                "typed config error expected for {url:?}, got {err:?}"
            );
            assert!(
                err.to_string().contains("influxdb.url"),
                "the error must name the offending field, got {err}"
            );
        }
    }

    #[test]
    fn parseable_urls_still_construct() {
        let exporter =
            InfluxDbExporter::new(config_with_url("http://127.0.0.1:1")).expect("a parseable http url must keep constructing");
        exporter.writer().shutdown();
    }
}
