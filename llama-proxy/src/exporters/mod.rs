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

/// WARN every Nth rejected sample. Counter exact, log sampled - the same split
/// `proxy::streaming` uses for unrepaired fixes.
const DROP_WARN_INTERVAL: u64 = 100;

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
pub fn log_sample_and_should_export(metrics: &RequestMetrics, format: StatsFormat) -> bool {
    if metrics.has_throughput_signal() {
        let formatted = format_metrics(metrics, format);
        if format == StatsFormat::Compact {
            tracing::info!("{}", formatted);
        } else {
            tracing::info!("\n{}", formatted);
        }
        return true;
    }

    let skipped = EXPORTS_SKIPPED_TOTAL.fetch_add(1, Ordering::Relaxed) + 1;
    if skipped == 1 || skipped.is_multiple_of(DROP_WARN_INTERVAL) {
        let prompt_tokens = match metrics.prompt_tokens {
            Some(tokens) => tokens.to_string(),
            None => "n/a".to_string(),
        };
        tracing::warn!(
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

        let keep = log_sample_and_should_export(&metrics, StatsFormat::Compact);

        assert!(keep);
    }

    /// The whole point of the change: an unmeasured sample is logged at WARN and
    /// never reaches an exporter, whatever the configured format.
    ///
    /// This is the ONLY test that takes the drop path, because it must be: the
    /// counter is process-global, the harness runs tests in threads, and a delta
    /// assertion is only exact while nothing else increments.
    #[test]
    fn test_gate_drops_unmeasurable_sample_in_every_format() {
        let before = EXPORTS_SKIPPED_TOTAL.load(Ordering::SeqCst);

        for format in [StatsFormat::Compact, StatsFormat::Json, StatsFormat::Pretty] {
            let keep = log_sample_and_should_export(&RequestMetrics::new(), format);
            assert!(!keep, "a zero-filled sample must not be exported ({format:?})");
        }

        assert_eq!(EXPORTS_SKIPPED_TOTAL.load(Ordering::SeqCst) - before, 3);
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
