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
        Self {
            exporters: Vec::new(),
        }
    }

    pub fn add(&mut self, exporter: Arc<dyn MetricsExporter>) {
        self.exporters.push(exporter);
    }

    pub async fn export_all(&self, metrics: &RequestMetrics) {
        for exporter in &self.exporters {
            match exporter.export(metrics).await {
                Ok(()) => {
                    tracing::debug!(
                        exporter = exporter.name(),
                        "Metrics exported successfully"
                    );
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
