//! Pluggable metrics exporters

mod influxdb;

use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::config::ExportersConfig;
use crate::config::StatsFormat;
use crate::stats::{format_metrics, RequestMetrics};

pub use influxdb::InfluxDbExporter;

/// Metric samples the export filter rejected. The reconciliation number for the
/// points that are therefore missing from InfluxDB - see
/// [`log_sample_and_should_export`].
pub static EXPORTS_SKIPPED_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Samples whose prefill rate exceeded `stats.max_plausible_prefill_tps`. Counted
/// exactly; the WARN is sampled. This is a CANARY, not a fault counter: a full prefix
/// cache hit is counted un-discounted by design and trips it legitimately. Process
/// global, resets on restart, never exported. See [`log_sample_and_should_export`].
pub static PREFILL_IMPLAUSIBLE_TOTAL: AtomicU64 = AtomicU64::new(0);

/// WARN every Nth occurrence for the two sampled warnings in this module (rejected
/// samples, implausible prefill). Counter exact, log sampled - the same split
/// `proxy::streaming` uses for unrepaired fixes. The two counters are INDEPENDENT
/// globals, so each emits its own first-and-every-Nth: occurrence 100 of one has no
/// relationship to occurrence 100 of the other.
const WARN_SAMPLE_INTERVAL: u64 = 100;

/// The cadence both sampled warnings above share: the first occurrence always warns,
/// because an operator must see a failure the moment it appears, then every
/// [`WARN_SAMPLE_INTERVAL`]th, because a flood must not drown the log. A function rather
/// than an inline expression so the cadence is testable at chosen occurrences - the two
/// counters are process-global atomics, and a test cannot set a global to "occurrence 99"
/// while sibling tests are incrementing it.
fn warns_at(occurrence: u64) -> bool {
    occurrence == 1 || occurrence.is_multiple_of(WARN_SAMPLE_INTERVAL)
}

/// Log one request's metrics line and report whether the sample may be exported.
///
/// Returns false when the sample carries no throughput signal at all (see
/// [`RequestMetrics::has_throughput_signal`]): no token count and no
/// backend-reported rate. That is an absent measurement rather than a slow
/// request, and every throughput field of it is a zero, so the caller must NOT
/// pass it to [`ExporterManager::export_all`].
///
/// The counter stays exact while the WARN is sampled, because a client that
/// interrupts streams (Claude Code's ESC, a cancelled re-prompt) makes this a
/// routine event that would otherwise bury every other warning. The WARN carries
/// the only fields of such a sample that are not zero or absent: how it ended
/// and how long it ran. Absent token counts print `n/a` — the sample's `None`
/// counts ARE the documented skip condition, never reported as measured zeros.
///
/// `max_plausible_prefill_tps` arms the prefill canary: any exported sample whose
/// prefill rate exceeds it is counted in [`PREFILL_IMPLAUSIBLE_TOTAL`] and WARNed on the
/// first occurrence and then every [`WARN_SAMPLE_INTERVAL`]. It is a plain parameter
/// rather than a read of global config so tests can drive the threshold directly. Zero or
/// negative disables the check.
pub fn log_sample_and_should_export(metrics: &RequestMetrics, format: StatsFormat, max_plausible_prefill_tps: f64) -> bool {
    if metrics.has_throughput_signal() {
        let formatted = format_metrics(metrics, format);
        // Explicit target: this is a STATS line that happens to be emitted from
        // the exporter module because that is where the export gate lives. Left
        // implicit it rendered as `INFO llama_proxy::exporters:` and sent
        // operators grepping for their per-request stats to the wrong place.
        // `RUST_LOG=llama_proxy::stats=info` now selects these lines.
        if format == StatsFormat::Compact {
            tracing::info!(target: "llama_proxy::stats", "{}", formatted);
        } else {
            tracing::info!(target: "llama_proxy::stats", "\n{}", formatted);
        }

        // On the healthy path only, and after the format branch so that ONE check covers
        // all three formats - the number under suspicion is a field of the sample, not a
        // rendering. Double-counting with EXPORTS_SKIPPED_TOTAL is impossible here rather
        // than merely avoided: any rate able to trip this is > 0.0, and `prompt_tps > 0.0`
        // is itself a disjunct of has_throughput_signal(), so a sample this check can fire
        // on can never take the skip path. That coupling is what
        // an_implausible_prefill_rate_is_counted_and_never_skipped pins - if the prompt_tps
        // term is ever dropped from has_throughput_signal(), that test is what says so.
        if max_plausible_prefill_tps > 0.0 {
            if let Some(rate) = metrics.prompt_tps.filter(|v| *v > max_plausible_prefill_tps) {
                let hits = PREFILL_IMPLAUSIBLE_TOTAL.fetch_add(1, Ordering::Relaxed) + 1;
                if warns_at(hits) {
                    let n = |v: Option<u64>| v.map(|t| t.to_string()).unwrap_or_else(|| "n/a".to_string());
                    // 1.1x is a rounding blip, 12x is a fabricated number. Numeric rather
                    // than "1.2x": prompt_tps and threshold are numeric too, and a lone
                    // string in that trio is the one field a log pipeline cannot filter on.
                    let over = (rate / max_plausible_prefill_tps * 10.0).round() / 10.0;
                    // No derived cache figure is printed here, deliberately, and re-adding
                    // one needs to answer both of these first:
                    //   * `prompt_tokens - cached_tokens` is only meaningful when cached is
                    //     a SUBSET of prompt. On the Anthropic shape it is DISJOINT
                    //     (extract_usage: input_tokens excludes cache_read_input_tokens,
                    //     which is why context_used there is the SUM), and the collector's
                    //     cache_is_a_subset decision (collector.rs:591) is not carried on
                    //     RequestMetrics, so this closure cannot know which shape it has.
                    //   * any "implied prefill duration" is algebraically prompt_ms: the
                    //     rate is numerator/time_to_first_token_ms and prompt_ms IS that
                    //     ttft (collector.rs:564,644), so dividing the rate back out
                    //     reproduces a field already on this line.
                    // The five raw fields below are the whole story; the operator's own
                    // measured engine rate is the only outside information that judges it.
                    //
                    // Identity fields use `?`, not `%`: `model` arrives verbatim from the
                    // request body and Display writes it raw, so a newline in it forges a
                    // line in the operator's log (same danger class as the F12-R4
                    // line-protocol forgery in influxdb.rs). The pre-existing warn below
                    // still uses `%`; that is a separate fix, not one to replicate here.
                    tracing::warn!(
                        target: "llama_proxy::stats",
                        model = ?metrics.model,
                        group = ?metrics.group_name.as_deref().unwrap_or("n/a"),
                        request_id = ?metrics.request_id,
                        prompt_tokens = %n(metrics.prompt_tokens),
                        cached_tokens = %n(metrics.cached_tokens),
                        prompt_ms = metrics.prompt_ms,
                        duration_ms = metrics.duration_ms,
                        stream_end = metrics.stream_end_label(),
                        prompt_tps = rate,
                        threshold = max_plausible_prefill_tps,
                        over_x = over,
                        occurrences = hits,
                        knob = "stats.max_plausible_prefill_tps",
                        "prefill rate implausible: counted in \
                         llama_proxy_stats_prefill_implausible_total on /proxy/metrics, where \
                         the counter is exact but only 1 in {} logs this line. cached_tokens at \
                         or near prompt_tokens is an EXPECTED trip - a full prefix-cache hit is \
                         counted un-discounted by design - so compare those two before believing \
                         anything is broken; cached_tokens=n/a means no prefix cache is \
                         reported, so judge prompt_tps against your own measured engine rate. \
                         This rate prints as a measured tps= and, when an exporter is enabled, \
                         ships to InfluxDB as-is. If your engine is genuinely this fast, RAISE \
                         the knob",
                        WARN_SAMPLE_INTERVAL
                    );
                }
            }
        }
        return true;
    }

    let skipped = EXPORTS_SKIPPED_TOTAL.fetch_add(1, Ordering::Relaxed) + 1;
    if warns_at(skipped) {
        let prompt_tokens = match metrics.prompt_tokens {
            Some(tokens) => tokens.to_string(),
            None => "n/a".to_string(),
        };
        tracing::warn!(
            target: "llama_proxy::stats",
            model = %metrics.model,
            stream_end = metrics.stream_end_label(),
            finish_reason = %metrics.finish_reason,
            prompt_tokens = %prompt_tokens,
            duration_ms = metrics.duration_ms,
            samples_skipped = skipped,
            "no usage/timings from the backend - excluded from exporters; every drop is \
             counted in llama_proxy_metrics_export_skipped_total on /proxy/metrics, only \
             1 in 100 logs this line"
        );
    }
    false
}

/// Trait for metrics exporters
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait MetricsExporter: Send + Sync {
    /// Export metrics to the destination
    async fn export(&self, metrics: &RequestMetrics) -> Result<(), ExportError>;

    /// Flush any buffered metrics
    async fn flush(&self) -> Result<(), ExportError> {
        Ok(())
    }

    /// Flush, then close the delivery path so nothing is accepted after the
    /// point of no return. Blocking is bounded by the exporter's own budget.
    async fn shutdown(&self) -> Result<(), ExportError> {
        self.flush().await
    }

    /// Name of the exporter
    fn name(&self) -> &str;
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Write error: {0}")]
    Write(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("{0}")]
    Other(String),
}

/// Manager for multiple exporters
pub struct ExporterManager {
    exporters: Vec<Arc<dyn MetricsExporter>>,
}

impl ExporterManager {
    pub fn new() -> Self {
        Self { exporters: Vec::new() }
    }

    pub fn add(&mut self, exporter: Arc<dyn MetricsExporter>) {
        self.exporters.push(exporter);
    }

    /// Build the manager from config. A disabled exporter is absent, never a
    /// stub; an enabled-but-compiled-out influxdb is a hard error (task 80
    /// [A-M9]) so the caller decides (startup failure vs warning) instead of
    /// the manager silently lying about working.
    pub fn from_config(config: &ExportersConfig) -> Result<Self, ExportError> {
        let mut manager = Self::new();
        if config.influxdb.enabled {
            let influx = InfluxDbExporter::from_config(&config.influxdb)?;
            manager.add(Arc::new(influx) as Arc<dyn MetricsExporter>);
        }
        Ok(manager)
    }

    /// Hand the sample to every exporter, unfiltered. Request samples go through
    /// [`log_sample_and_should_export`] first - that filter is what keeps
    /// zero-filled samples out of the remote destination.
    pub async fn export_all(&self, metrics: &RequestMetrics) {
        for exporter in &self.exporters {
            match exporter.export(metrics).await {
                Ok(()) => {
                    tracing::debug!(exporter = exporter.name(), "Metrics exported successfully");
                }
                Err(e) => {
                    tracing::warn!(
                        exporter = exporter.name(),
                        error = %e,
                        "Failed to export metrics"
                    );
                }
            }
        }
    }

    pub async fn flush_all(&self) {
        for exporter in &self.exporters {
            if let Err(e) = exporter.flush().await {
                tracing::warn!(
                    exporter = exporter.name(),
                    error = %e,
                    "Failed to flush exporter"
                );
            }
        }
    }

    /// Drive `shutdown()` on every exporter. This is the PROCESS-SHUTDOWN
    /// hook: call it after the server has drained its connections, so no
    /// request hands a sample in behind it and the influxdb worker's
    /// drain-exit is actually JOINED (its bounded budget is documented on
    /// `worker::WriterQueue::shutdown_after`). Until a call site awaits this,
    /// accepted samples can die with the process after `export()` said
    /// queued - see worker.rs' module header.
    pub async fn shutdown_all(&self) {
        for exporter in &self.exporters {
            if let Err(e) = exporter.shutdown().await {
                tracing::warn!(
                    exporter = exporter.name(),
                    error = %e,
                    "Failed to shutdown exporter"
                );
            }
        }
    }
}

impl Default for ExporterManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The export-gate tests below are about filtering, not about the prefill canary.
    /// Arming the canary in them would make their assertions depend on the counter a
    /// sibling test left behind, and would mix a canary WARN into the log the
    /// stats-target test captures. Tests that DO exercise the canary pass a real
    /// threshold explicitly.
    const CANARY_OFF: f64 = 0.0;

    /// Both counters in this module are process-global atomics and the harness runs
    /// tests on parallel threads, so a `load -> call -> load` delta is only exact while
    /// one test at a time is driving them. Same idiom as `influxdb.rs`'s `SERIAL`.
    /// Every test here that asserts a counter delta MUST hold this, including the
    /// pre-existing drop-path test: the canary tests add concurrent increments to the
    /// sample path, which would otherwise make that test's delta-3 flaky too.
    static COUNTER_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn counter_turn() -> std::sync::MutexGuard<'static, ()> {
        COUNTER_TURN.lock().unwrap_or_else(|e| e.into_inner())
    }

    // Mock exporter for testing
    struct MockExporter {
        name: String,
        should_fail: bool,
        export_count: std::sync::atomic::AtomicU32,
    }

    impl MockExporter {
        fn new(name: &str, should_fail: bool) -> Self {
            Self {
                name: name.to_string(),
                should_fail,
                export_count: std::sync::atomic::AtomicU32::new(0),
            }
        }

        fn get_export_count(&self) -> u32 {
            self.export_count.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl MetricsExporter for MockExporter {
        async fn export(&self, _metrics: &RequestMetrics) -> Result<(), ExportError> {
            self.export_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.should_fail {
                Err(ExportError::Write("Mock failure".to_string()))
            } else {
                Ok(())
            }
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    #[test]
    fn test_export_error_display() {
        let err = ExportError::Connection("failed to connect".to_string());
        assert!(err.to_string().contains("failed to connect"));

        let err = ExportError::Auth("invalid token".to_string());
        assert!(err.to_string().contains("invalid token"));

        let err = ExportError::Write("write failed".to_string());
        assert!(err.to_string().contains("write failed"));

        let err = ExportError::Config("missing config".to_string());
        assert!(err.to_string().contains("missing config"));

        let err = ExportError::Other("unknown error".to_string());
        assert!(err.to_string().contains("unknown error"));
    }

    #[test]
    fn test_exporter_manager_new() {
        let manager = ExporterManager::new();
        assert!(manager.exporters.is_empty());
    }

    #[test]
    fn test_exporter_manager_default() {
        let manager = ExporterManager::default();
        assert!(manager.exporters.is_empty());
    }

    #[tokio::test]
    async fn test_exporter_manager_add() {
        let mut manager = ExporterManager::new();
        let exporter = Arc::new(MockExporter::new("test", false));
        manager.add(exporter);

        assert_eq!(manager.exporters.len(), 1);
    }

    /// A sample that clears the export filter. `RequestMetrics::new()` is
    /// zero-filled and is filtered out on purpose, so it cannot stand in for
    /// "a request happened" in these tests.
    fn measurable_metrics() -> RequestMetrics {
        let mut metrics = RequestMetrics::new();
        metrics.model = "test-model".to_string();
        metrics.total_tokens = 150;
        metrics.total_tps = 125.0;
        metrics
    }

    #[tokio::test]
    async fn test_exporter_manager_export_all_success() {
        let mut manager = ExporterManager::new();
        let exporter = Arc::new(MockExporter::new("test", false));
        manager.add(Arc::clone(&exporter) as Arc<dyn MetricsExporter>);

        let metrics = measurable_metrics();
        manager.export_all(&metrics).await;

        assert_eq!(exporter.get_export_count(), 1);
    }

    #[tokio::test]
    async fn test_exporter_manager_export_all_failure() {
        let mut manager = ExporterManager::new();
        let exporter = Arc::new(MockExporter::new("test", true));
        manager.add(Arc::clone(&exporter) as Arc<dyn MetricsExporter>);

        let metrics = measurable_metrics();
        // Should not panic even if export fails
        manager.export_all(&metrics).await;

        assert_eq!(exporter.get_export_count(), 1);
    }

    #[tokio::test]
    async fn test_exporter_manager_export_all_multiple() {
        let mut manager = ExporterManager::new();
        let exporter1 = Arc::new(MockExporter::new("test1", false));
        let exporter2 = Arc::new(MockExporter::new("test2", false));

        manager.add(Arc::clone(&exporter1) as Arc<dyn MetricsExporter>);
        manager.add(Arc::clone(&exporter2) as Arc<dyn MetricsExporter>);

        let metrics = measurable_metrics();
        manager.export_all(&metrics).await;

        assert_eq!(exporter1.get_export_count(), 1);
        assert_eq!(exporter2.get_export_count(), 1);
    }

    /// Task 82 [A-M10b] verify: the consumers (server.rs `Arc<ExporterManager>`,
    /// streaming.rs clones) drive the WHOLE lifecycle through one shared handle.
    /// That compiles only while the registry stores `Arc<dyn MetricsExporter>`
    /// and export_all/flush_all/shutdown_all all take `&self` - this test IS the
    /// registry-shape contract; a regression to `&mut self` lifecycle breaks it.
    #[tokio::test]
    async fn arc_shared_manager_drives_lifecycle_through_self_ref() {
        let mut manager = ExporterManager::new();
        let exporter = Arc::new(MockExporter::new("test", false));
        manager.add(Arc::clone(&exporter) as Arc<dyn MetricsExporter>);
        let manager = Arc::new(manager); // Send + Sync through one Arc

        let mut tasks = Vec::new();
        for _ in 0..4 {
            let manager = Arc::clone(&manager);
            tasks.push(tokio::spawn(async move {
                manager.export_all(&measurable_metrics()).await;
                manager.flush_all().await;
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(exporter.get_export_count(), 4);

        manager.shutdown_all().await; // still &self: the Arc stays shared-valid
        drop(manager);
    }

    /// `export_all` is a plain fan-out: it must not filter, so that an exporter
    /// which *wants* zero-filled samples (an audit or request-log exporter) can
    /// still be handed them. The filter lives in the gate below.
    #[tokio::test]
    async fn test_export_all_does_not_filter() {
        let mut manager = ExporterManager::new();
        let exporter = Arc::new(MockExporter::new("audit", false));
        manager.add(Arc::clone(&exporter) as Arc<dyn MetricsExporter>);

        manager.export_all(&RequestMetrics::new()).await;

        assert_eq!(exporter.get_export_count(), 1);
    }

    #[test]
    fn test_gate_passes_measurable_sample() {
        let metrics = measurable_metrics();

        let keep = log_sample_and_should_export(&metrics, StatsFormat::Compact, CANARY_OFF);

        assert!(keep);
    }

    /// The whole point of the change: an unmeasured sample is logged at WARN and
    /// never reaches an exporter, whatever the configured format.
    ///
    /// Its delta-exactness used to rest on being the ONLY test on the drop path. The
    /// canary tests add concurrent increments to the sample path, so like every other
    /// delta assertion here it now takes the module's counter turn.
    #[test]
    fn test_gate_drops_unmeasurable_sample_in_every_format() {
        let _turn = counter_turn();
        let before = EXPORTS_SKIPPED_TOTAL.load(Ordering::SeqCst);

        for format in [StatsFormat::Compact, StatsFormat::Json, StatsFormat::Pretty] {
            let keep = log_sample_and_should_export(&RequestMetrics::new(), format, CANARY_OFF);
            assert!(!keep, "a zero-filled sample must not be exported ({format:?})");
        }

        assert_eq!(EXPORTS_SKIPPED_TOTAL.load(Ordering::SeqCst) - before, 3);
    }

    /// In-file tracing capture (the `backends/node.rs` / `augment.rs` pattern):
    /// thread-local `set_default` keeps this parallel-safe.
    #[derive(Clone)]
    struct CaptureWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureWriter {
        type Writer = Self;
        fn make_writer(&self) -> Self::Writer {
            self.clone()
        }
    }

    /// The compact stats line is a STATS line that merely happens to be emitted
    /// from this module, because this is where the export gate lives. It used to
    /// inherit the implicit `llama_proxy::exporters` target, so
    /// `RUST_LOG=llama_proxy::stats=info` selected nothing and an operator
    /// hunting for per-request throughput was sent to the wrong file. Asserted
    /// through a real formatter so the rename is proven observable, not just
    /// written: an operator's stats-only filter must keep these lines and drop
    /// everything else this module logs.
    #[test]
    fn stats_lines_belong_to_the_stats_target() {
        crate::fixes::pin_interest_cache_for_tests();
        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new("off,llama_proxy::stats=info"))
            .with_ansi(false)
            .with_target(true)
            .with_writer(CaptureWriter(buf.clone()))
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let mut m = RequestMetrics::new();
        m.model = "target-probe".to_string();
        m.total_tokens = 150;
        m.completion_tokens = Some(50);
        m.total_tps = 125.0;
        m.decode_tps_min = 41.67;

        // Must NOT survive a stats-only filter: this module's own target is not
        // the stats target.
        tracing::info!("exporter-internal noise");
        // Must survive, attributed to the stats module.
        let keep = log_sample_and_should_export(&m, StatsFormat::Compact, CANARY_OFF);
        drop(_guard);

        let text = String::from_utf8(std::mem::take(&mut *buf.lock().unwrap())).unwrap();
        assert!(keep, "a measurable sample is still a measurable sample");
        assert!(text.contains("target-probe"), "the stats line vanished: {text}");
        assert!(
            text.contains("llama_proxy::stats"),
            "stats line is not attributable to the stats module: {text}"
        );
        assert!(
            !text.contains("exporter-internal noise"),
            "a stats-only filter leaked a non-stats event, so the target is cosmetic: {text}"
        );
    }

    /// The canary reports, it does not gate: a counted sample is still exported.
    #[test]
    fn prefill_canary_counts_a_rate_above_the_threshold() {
        let _turn = counter_turn();
        let before = PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst);
        let mut m = measurable_metrics();
        m.prompt_tps = Some(60_000.0);
        assert!(
            log_sample_and_should_export(&m, StatsFormat::Compact, 50_000.0),
            "an implausible prefill must not cost the request its export"
        );
        assert_eq!(
            PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst) - before,
            1,
            "one request is one occurrence"
        );
    }

    /// The comparison is strict. A rate exactly AT the ceiling is the ceiling, not an
    /// overrun: an operator who sizes the threshold to their measured peak must not be
    /// warned about that peak.
    #[test]
    fn prefill_canary_leaves_a_rate_at_or_below_the_threshold_alone() {
        let _turn = counter_turn();
        let before = PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst);
        for rate in [50_000.0, 49_999.0, 1.0] {
            let mut m = measurable_metrics();
            m.prompt_tps = Some(rate);
            assert!(log_sample_and_should_export(&m, StatsFormat::Compact, 50_000.0));
        }
        assert_eq!(
            PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst) - before,
            0,
            "a rate at the threshold is the threshold, not an overrun"
        );
    }

    /// Zero disables, and so does any negative value: `rate > -1.0` is true for every
    /// rate, so a negative threshold would count every single request as implausible.
    #[test]
    fn prefill_canary_is_off_at_zero_and_below() {
        let _turn = counter_turn();
        let before = PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst);
        for threshold in [0.0, -1.0] {
            let mut m = measurable_metrics();
            m.prompt_tps = Some(1_000_000_000.0);
            assert!(log_sample_and_should_export(&m, StatsFormat::Compact, threshold));
        }
        assert_eq!(
            PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst) - before,
            0,
            "a non-positive threshold must switch the canary off entirely"
        );
    }

    /// A decode rate without a prefill rate is not a slow prefill. `None` must never be
    /// judged as a number, and a tiny threshold must not turn its absence into a hit.
    #[test]
    fn prefill_canary_says_nothing_about_a_sample_with_no_prefill_rate() {
        let _turn = counter_turn();
        let before = PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst);
        let mut m = measurable_metrics();
        m.generation_tps = Some(40.0);
        assert!(log_sample_and_should_export(&m, StatsFormat::Compact, 1.0));
        assert_eq!(
            PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst) - before,
            0,
            "an absent prefill rate is unmeasured, never implausible"
        );
    }

    /// Guards the skip path itself: nothing may increment the canary from there, or a
    /// single request is counted as both skipped and implausible. It does NOT pin where
    /// the check sits inside the healthy branch - a zero-filled sample carries no
    /// `prompt_tps` at all, so it reads 0 wherever the check lives. The placement and
    /// coupling witness is `an_implausible_prefill_rate_is_counted_and_never_skipped`.
    #[test]
    fn an_unexportable_sample_never_reaches_the_canary() {
        let _turn = counter_turn();
        let skipped_before = EXPORTS_SKIPPED_TOTAL.load(Ordering::SeqCst);
        let canary_before = PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst);
        assert!(!log_sample_and_should_export(
            &RequestMetrics::new(),
            StatsFormat::Compact,
            1.0
        ));
        assert_eq!(
            PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst) - canary_before,
            0,
            "a skipped sample was also counted as an implausible prefill"
        );
        assert_eq!(
            EXPORTS_SKIPPED_TOTAL.load(Ordering::SeqCst) - skipped_before,
            1,
            "control: the sample did take the skip path, so the assertion above tested a real branch"
        );
    }

    /// The witness the placement actually needs, and the reason the two counters cannot
    /// both claim one request. `RequestMetrics::new()` carries `prompt_tps: None`, so it
    /// reads 0 wherever the check sits; THIS is the sample that can move. Its prompt_tps
    /// is the only signal it holds, so it clears the export gate solely via the
    /// `prompt_tps > 0.0` disjunct of `has_throughput_signal()`. Drop that disjunct and
    /// the sample silently becomes a skip - the drift this test exists to catch.
    #[test]
    fn an_implausible_prefill_rate_is_counted_and_never_skipped() {
        let _turn = counter_turn();
        let canary_before = PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst);
        let skipped_before = EXPORTS_SKIPPED_TOTAL.load(Ordering::SeqCst);
        let mut m = RequestMetrics::new();
        m.prompt_tps = Some(60_000.0);
        assert!(
            log_sample_and_should_export(&m, StatsFormat::Compact, 50_000.0),
            "a rate on its own IS a throughput signal; this sample must be exported"
        );
        assert_eq!(
            PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst) - canary_before,
            1,
            "the rate was above the threshold but the canary did not see it"
        );
        assert_eq!(
            EXPORTS_SKIPPED_TOTAL.load(Ordering::SeqCst) - skipped_before,
            0,
            "one request counted as both implausible and skipped: the two counters no \
             longer partition the samples"
        );
    }

    /// The check sits AFTER the format branch, so all three formats are covered by the
    /// same lines. Moving it inside the compact arm would silently stop counting for
    /// json and pretty; this is what makes that edit fail.
    #[test]
    fn prefill_canary_arms_on_every_stats_format() {
        let _turn = counter_turn();
        let before = PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst);
        for format in [StatsFormat::Compact, StatsFormat::Json, StatsFormat::Pretty] {
            let mut m = measurable_metrics();
            m.prompt_tps = Some(90_000.0);
            assert!(log_sample_and_should_export(&m, format, 50_000.0));
        }
        assert_eq!(
            PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst) - before,
            3,
            "a format was not covered by the check"
        );
    }

    #[test]
    fn sampled_warnings_fire_first_then_every_interval() {
        assert!(warns_at(1), "a brand-new failure must be visible immediately");
        // No zeroth case: 0 IS a multiple of 100, so asserting anything about it would
        // be pinning a promise the predicate never made. Callers do `fetch_add(..) + 1`,
        // so 1 is the smallest occurrence that can exist.
        for n in 2..WARN_SAMPLE_INTERVAL {
            assert!(!warns_at(n), "occurrence {n} must stay silent");
        }
        assert!(warns_at(WARN_SAMPLE_INTERVAL));
        assert!(warns_at(2 * WARN_SAMPLE_INTERVAL));
        assert!(!warns_at(WARN_SAMPLE_INTERVAL + 1));
    }

    /// Counter exact, log sampled - asserted the way it can be proven under a shared
    /// global: `WARN_SAMPLE_INTERVAL` consecutive occurrences cannot avoid containing a
    /// multiple of the interval whatever offset the counter started at, so a WARN must
    /// appear, while the counter still records all of them. The field names asserted
    /// are unique to the WARN - the compact stats line prints the model and `cache=`, so
    /// those would pass on the INFO line alone and prove nothing.
    #[test]
    fn a_persistent_implausible_rate_is_counted_always_and_logged_sometimes() {
        crate::fixes::pin_interest_cache_for_tests();
        let _turn = counter_turn();
        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new("off,llama_proxy::stats=info"))
            .with_ansi(false)
            .with_target(true)
            .with_writer(CaptureWriter(buf.clone()))
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let before = PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst);
        let mut m = measurable_metrics();
        m.model = "canary-probe".to_string();
        m.prompt_tps = Some(75_000.0);
        for _ in 0..WARN_SAMPLE_INTERVAL {
            assert!(log_sample_and_should_export(&m, StatsFormat::Compact, 50_000.0));
        }
        drop(_guard);

        let text = String::from_utf8(std::mem::take(&mut *buf.lock().unwrap())).unwrap();
        assert_eq!(
            PREFILL_IMPLAUSIBLE_TOTAL.load(Ordering::SeqCst) - before,
            WARN_SAMPLE_INTERVAL,
            "the counter is the reconciliation number and must be exact"
        );
        assert!(
            text.contains("prefill rate implausible"),
            "a rate implausible for 100 straight requests never reached the log: {text}"
        );
        assert!(
            text.contains("threshold")
                && text.contains("over_x")
                && text.contains("knob")
                && text.contains("occurrences")
                && text.contains("prompt_tokens")
                && text.contains("cached_tokens"),
            "the WARN dropped the fields that let an operator judge the trip: {text}"
        );
        // The cadence is stated in the message, so it must come from the constant: a
        // literal "1 in 100" here would keep telling operators 100 after someone
        // retuned WARN_SAMPLE_INTERVAL.
        assert!(
            text.contains(&format!("1 in {WARN_SAMPLE_INTERVAL} ")),
            "the WARN states a sampling rate that is not WARN_SAMPLE_INTERVAL: {text}"
        );
        // Locked absent, not absent by oversight. `prompt_tokens - cached_tokens` is a
        // wrong subtraction on the Anthropic shape (cached_tokens there is
        // cache_read_input_tokens, which input_tokens already excludes), and an
        // "implied prefill duration" divided back out of the rate is prompt_ms itself.
        assert!(
            !text.contains("uncached_tokens") && !text.contains("implied_prefill_ms"),
            "a derived cache figure came back onto the WARN line: {text}"
        );
    }

    /// The WARN's identity fields are recorded with `?` rather than `%` for a reason:
    /// `model` is client-supplied, and Display writes a newline in it raw, so one request
    /// could forge its own line in the operator's stats log - the same forgery class
    /// F12-R4 killed in the InfluxDB line protocol. Asserted on the WARN line alone on
    /// purpose: the compact INFO line prints the model unescaped too, and fixing that
    /// formatter is not this task's business. Switch any of these back to `%` and the
    /// `find` below still succeeds on the trailing fragment while the escaped literal
    /// disappears, which is what fails this test.
    #[test]
    fn a_newline_in_the_model_name_cannot_forge_a_second_line_in_the_canary_warn() {
        crate::fixes::pin_interest_cache_for_tests();
        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new("off,llama_proxy::stats=info"))
            .with_ansi(false)
            .with_target(true)
            .with_writer(CaptureWriter(buf.clone()))
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let _turn = counter_turn();

        let mut m = measurable_metrics();
        m.model = "canary\nINFO forged 1".to_string();
        m.prompt_tps = Some(60_000.0);
        // A full interval, not one call: the WARN is sampled off a process-global
        // occurrence counter that sibling tests have already advanced, so a single call
        // would only log if its occurrence happened to land on 1 or a multiple of 100.
        // Any `WARN_SAMPLE_INTERVAL` consecutive occurrences must contain a multiple.
        for _ in 0..WARN_SAMPLE_INTERVAL {
            assert!(log_sample_and_should_export(&m, StatsFormat::Compact, 50_000.0));
        }
        drop(_guard);

        let text = String::from_utf8(std::mem::take(&mut *buf.lock().unwrap())).unwrap();
        let warn = text
            .lines()
            .find(|l| l.contains("prefill rate implausible"))
            .unwrap_or_else(|| panic!("the canary WARN never reached the log:\n{text}"));
        assert!(
            warn.contains(r"canary\nINFO forged"),
            "the model's newline was written raw, so the field is Display-formatted and a \
             client can forge a log line:\n{warn}"
        );
    }

    #[tokio::test]
    async fn test_exporter_manager_flush_all() {
        let mut manager = ExporterManager::new();
        let exporter = Arc::new(MockExporter::new("test", false));
        manager.add(exporter as Arc<dyn MetricsExporter>);

        // Should not panic
        manager.flush_all().await;
    }

    #[tokio::test]
    async fn test_exporter_manager_shutdown_all() {
        let mut manager = ExporterManager::new();
        let exporter = Arc::new(MockExporter::new("test", false));
        manager.add(exporter as Arc<dyn MetricsExporter>);

        // Should not panic
        manager.shutdown_all().await;
    }

    #[tokio::test]
    async fn test_exporter_manager_empty() {
        let manager = ExporterManager::new();

        let metrics = RequestMetrics::new();
        // Should not panic with no exporters
        manager.export_all(&metrics).await;
        manager.flush_all().await;
        manager.shutdown_all().await;
    }

    fn app_influx_config(enabled: bool) -> crate::config::InfluxDbConfig {
        crate::config::InfluxDbConfig {
            enabled,
            url: "http://127.0.0.1:1".to_string(),
            org: "test".to_string(),
            bucket: "test".to_string(),
            token: "test".to_string(),
        }
    }

    fn exporters_config_with_influx(enabled: bool) -> ExportersConfig {
        ExportersConfig {
            influxdb: app_influx_config(enabled),
        }
    }

    #[tokio::test]
    async fn manager_from_config_absent_exporter_stays_empty() {
        let manager = ExporterManager::from_config(&exporters_config_with_influx(false)).unwrap();
        assert!(manager.exporters.is_empty());
        manager.export_all(&measurable_metrics()).await;
    }

    /// The whole point of task 80 [A-M9], compiled ONLY in a build without the
    /// influxdb feature: `enabled: true` must fail loudly everywhere it is
    /// asked, and a hand-built stub must refuse instead of lying.
    #[cfg(not(feature = "influxdb"))]
    mod feature_off {
        use super::*;

        #[test]
        fn manager_from_config_hard_errors_when_enabled() {
            let outcome = ExporterManager::from_config(&exporters_config_with_influx(true));
            let err = match outcome {
                Err(e) => e,
                Ok(_) => panic!("enabled influxdb without the feature must not construct"),
            };
            assert!(
                matches!(err, ExportError::Config(_)),
                "typed config error expected, got {err:?}"
            );
            assert_eq!(
                err.to_string(),
                "Configuration error: influxdb enabled in config but binary built without the influxdb feature"
            );
        }

        #[test]
        fn exporter_from_config_hard_errors_when_enabled() {
            let outcome = InfluxDbExporter::from_config(&app_influx_config(true));
            let err = match outcome {
                Err(e) => e,
                Ok(_) => panic!("the direct construction gate must fail too"),
            };
            assert!(err.to_string().contains("without the influxdb feature"));
        }

        #[test]
        fn disabled_config_still_constructs_inert() {
            assert!(InfluxDbExporter::from_config(&app_influx_config(false)).is_ok());
        }

        /// Backstop: even a hand-built compiled-out exporter reports failure
        /// on export - the pre-task-80 stub returned Ok while dropping every
        /// sample, which is the silent lie this task deletes.
        #[tokio::test]
        async fn hand_built_stub_export_errors_not_silent_ok() {
            let exporter = InfluxDbExporter::new(crate::exporters::influxdb::InfluxDbConfig {
                url: "http://127.0.0.1:1".to_string(),
                org: "test".to_string(),
                bucket: "test".to_string(),
                token: "test".to_string(),
            })
            .unwrap();
            let err = exporter
                .export(&measurable_metrics())
                .await
                .expect_err("compiled-out exporter must not report success");
            assert!(err.to_string().contains("without the influxdb feature"));
        }

        /// The manager's `?` path turns the enabled config into one WARN per
        /// startup - never a registered exporter that exports nothing.
        #[tokio::test]
        async fn manager_never_registers_a_compiled_out_exporter() {
            let mut manager = ExporterManager::new();
            let outcome = InfluxDbExporter::from_config(&app_influx_config(true));
            match outcome {
                Ok(exp) => manager.add(Arc::new(exp) as Arc<dyn MetricsExporter>),
                Err(e) => tracing::warn!(error = %e, "Failed to initialize InfluxDbExporter"),
            }
            assert!(manager.exporters.is_empty());
        }
    }

    /// Feature-on side of the matrix: the same config constructs, and `export`
    /// performs a real write attempt (fails against the dead 127.0.0.1:1
    /// endpoint - the honest inverse of the compiled-out stub's fake Ok).
    #[cfg(all(test, feature = "influxdb"))]
    mod feature_on {
        use super::*;

        #[test]
        fn manager_from_config_registers_enabled_exporter() {
            let manager = ExporterManager::from_config(&exporters_config_with_influx(true)).unwrap();
            assert_eq!(manager.exporters.len(), 1);
            assert_eq!(manager.exporters[0].name(), "influxdb");
        }

        /// Task 81 made export() a queue hand-off: a live exporter always
        /// accepts (Ok); delivery failures surface through the worker's
        /// counters, never as a request-path error or a hang.
        #[tokio::test]
        async fn export_hands_off_to_the_queue_without_waiting() {
            let exporter = InfluxDbExporter::from_config(&app_influx_config(true)).unwrap();
            exporter.export(&measurable_metrics()).await.unwrap();
            assert_eq!(exporter.writer().dropped_total(), 0);
            exporter.writer().shutdown();
        }

        #[test]
        fn name_is_stable() {
            let exporter = InfluxDbExporter::from_config(&app_influx_config(true)).unwrap();
            assert_eq!(exporter.name(), "influxdb");
        }
    }
}
