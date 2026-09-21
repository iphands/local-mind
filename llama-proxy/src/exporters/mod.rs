//! Pluggable metrics exporters

mod influxdb;

use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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

    /// Shutdown the exporter gracefully
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

    #[test]
    fn test_influxdb_exporter_new_without_feature() {
        // Test that InfluxDbExporter::new works without the influxdb feature
        let config = crate::exporters::influxdb::InfluxDbConfig {
            url: "http://localhost:8086".to_string(),
            org: "test".to_string(),
            bucket: "test".to_string(),
            token: "test".to_string(),
            batch_size: 10,
            flush_interval_seconds: 5,
        };

        let exporter = InfluxDbExporter::new(config);
        assert!(exporter.is_ok());
    }

    #[tokio::test]
    async fn test_influxdb_exporter_export_without_feature() {
        let config = crate::exporters::influxdb::InfluxDbConfig {
            url: "http://localhost:8086".to_string(),
            org: "test".to_string(),
            bucket: "test".to_string(),
            token: "test".to_string(),
            batch_size: 10,
            flush_interval_seconds: 5,
        };

        let exporter = InfluxDbExporter::new(config).unwrap();
        let metrics = RequestMetrics::new();

        // Without the influxdb feature, this should succeed silently
        let result = exporter.export(&metrics).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_influxdb_exporter_name() {
        let config = crate::exporters::influxdb::InfluxDbConfig {
            url: "http://localhost:8086".to_string(),
            org: "test".to_string(),
            bucket: "test".to_string(),
            token: "test".to_string(),
            batch_size: 10,
            flush_interval_seconds: 5,
        };

        let exporter = InfluxDbExporter::new(config).unwrap();
        assert_eq!(exporter.name(), "influxdb");
    }
}
