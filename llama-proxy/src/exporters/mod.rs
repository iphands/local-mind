//! Pluggable metrics exporters

mod influxdb;

use async_trait::async_trait;
use std::sync::Arc;

use crate::stats::RequestMetrics;

pub use influxdb::InfluxDbExporter;

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

    #[tokio::test]
    async fn test_exporter_manager_export_all_success() {
        let mut manager = ExporterManager::new();
        let exporter = Arc::new(MockExporter::new("test", false));
        manager.add(Arc::clone(&exporter) as Arc<dyn MetricsExporter>);

        let metrics = RequestMetrics::new();
        manager.export_all(&metrics).await;

        assert_eq!(exporter.get_export_count(), 1);
    }

    #[tokio::test]
    async fn test_exporter_manager_export_all_failure() {
        let mut manager = ExporterManager::new();
        let exporter = Arc::new(MockExporter::new("test", true));
        manager.add(Arc::clone(&exporter) as Arc<dyn MetricsExporter>);

        let metrics = RequestMetrics::new();
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

        let metrics = RequestMetrics::new();
        manager.export_all(&metrics).await;

        assert_eq!(exporter1.get_export_count(), 1);
        assert_eq!(exporter2.get_export_count(), 1);
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
