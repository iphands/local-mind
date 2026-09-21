//! InfluxDB v2 metrics exporter

use async_trait::async_trait;

use super::{ExportError, MetricsExporter};
use crate::stats::RequestMetrics;

#[cfg(feature = "influxdb")]
mod worker;
#[cfg(feature = "influxdb")]
use worker::{SendOutcome, WriterQueue};

/// InfluxDB v2 exporter configuration
#[derive(Debug, Clone)]
pub struct InfluxDbConfig {
    pub url: String,
    pub org: String,
    pub bucket: String,
    pub token: String,
    pub batch_size: usize,
    pub flush_interval_seconds: u64,
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
    #[cfg(not(feature = "influxdb"))]
    #[allow(dead_code)] // constructed by from_config even when compiled out
    config: InfluxDbConfig,
    #[cfg(feature = "influxdb")]
    queue: WriterQueue,
    #[cfg(not(feature = "influxdb"))]
    _phantom: (),
}

impl InfluxDbExporter {
    /// Create a new InfluxDB exporter. The bounded writer thread starts here
    /// and here only - one per exporter, for the exporter's whole life.
    #[cfg(feature = "influxdb")]
    pub fn new(config: InfluxDbConfig) -> Result<Self, ExportError> {
        let client = influxdb2::Client::new(&config.url, &config.org, &config.token);
        let queue = WriterQueue::spawn(client, config);

        Ok(Self { queue })
    }

    #[cfg(not(feature = "influxdb"))]
    pub fn new(config: InfluxDbConfig) -> Result<Self, ExportError> {
        Ok(Self { config, _phantom: () })
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
            batch_size: config.batch_size,
            flush_interval_seconds: config.flush_interval_seconds,
        })
    }
}

/// Build the `llama_request` point for one sample. Pure so the wire shape is
/// testable without a network: an absent token count (`None`) omits its field
/// entirely rather than writing a fabricated 0 into the aggregate.
#[cfg(feature = "influxdb")]
fn build_data_point(metrics: &RequestMetrics) -> Result<influxdb2::models::DataPoint, ExportError> {
    use influxdb2::models::DataPoint;

    let mut builder = DataPoint::builder("llama_request")
        .tag("model", &metrics.model)
        .tag("streaming", metrics.streaming.to_string())
        .tag("finish_reason", &metrics.finish_reason)
        .tag("stream_end", metrics.stream_end_label());

    if let Some(ref client_id) = metrics.client_id {
        builder = builder.tag("client_id", client_id.as_str());
    }

    if let Some(ref conv_id) = metrics.conversation_id {
        builder = builder.tag("conversation_id", conv_id.as_str());
    }

    if let Some(ref group_name) = metrics.group_name {
        builder = builder.tag("group_name", group_name.as_str());
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

    let point = point.timestamp(metrics.timestamp.timestamp_nanos_opt().unwrap_or(0));

    point
        .build()
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
        let point = build_data_point(metrics).unwrap();
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
    fn present_token_counts_are_written() {
        let mut m = measurable();
        m.prompt_tokens = Some(100);
        m.completion_tokens = Some(0);
        let line = line(&m);
        assert!(line.contains("prompt_tokens=100"), "got: {line}");
        assert!(line.contains("completion_tokens=0"), "measured zero written: {line}");
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
                batch_size: 10,
                flush_interval_seconds: 5,
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
            batch_size: 10,
            flush_interval_seconds: 5,
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
    async fn post_shutdown_export_reports_shut_down_cleanly() {
        let exporter = InfluxDbExporter::new(InfluxDbConfig {
            url: "http://127.0.0.1:1".into(),
            org: "o".into(),
            bucket: "b".into(),
            token: "t".into(),
            batch_size: 10,
            flush_interval_seconds: 5,
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
