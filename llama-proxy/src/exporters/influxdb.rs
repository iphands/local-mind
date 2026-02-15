//! InfluxDB v2 metrics exporter

use async_trait::async_trait;

use super::{ExportError, MetricsExporter};
use crate::stats::RequestMetrics;

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

/// InfluxDB v2 metrics exporter
pub struct InfluxDbExporter {
    #[allow(dead_code)] // Used when influxdb feature is enabled
    config: InfluxDbConfig,
    #[cfg(feature = "influxdb")]
    client: Option<influxdb2::Client>,
    #[cfg(not(feature = "influxdb"))]
    _phantom: (),
}

impl InfluxDbExporter {
    /// Create a new InfluxDB exporter
    #[cfg(feature = "influxdb")]
    pub fn new(config: InfluxDbConfig) -> Result<Self, ExportError> {
        let client = influxdb2::Client::new(&config.url, &config.org, &config.token);

        Ok(Self {
            config,
            client: Some(client),
        })
    }

    #[cfg(not(feature = "influxdb"))]
    pub fn new(config: InfluxDbConfig) -> Result<Self, ExportError> {
        tracing::warn!(
            "InfluxDB exporter requested but 'influxdb' feature is not enabled. \
             Enable with --features influxdb"
        );
        Ok(Self {
            config,
            _phantom: (),
        })
    }

    /// Create from app config
    pub fn from_config(
        config: &crate::config::InfluxDbConfig,
    ) -> Result<Self, ExportError> {
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

#[async_trait]
impl MetricsExporter for InfluxDbExporter {
    #[cfg(feature = "influxdb")]
    async fn export(&self, metrics: &RequestMetrics) -> Result<(), ExportError> {
        use influxdb2::models::DataPoint;

        let client = match &self.client {
            Some(c) => c,
            None => return Ok(()),
        };

        let mut builder = DataPoint::builder("llama_request")
            .tag("model", &metrics.model)
            .tag("streaming", metrics.streaming.to_string())
            .tag("finish_reason", &metrics.finish_reason);

        if let Some(ref client_id) = metrics.client_id {
            builder = builder.tag("client_id", client_id.as_str());
        }

        if let Some(ref conv_id) = metrics.conversation_id {
            builder = builder.tag("conversation_id", conv_id.as_str());
        }

        let point = builder
            .field("prompt_tokens", metrics.prompt_tokens as f64)
            .field("completion_tokens", metrics.completion_tokens as f64)
            .field("total_tokens", metrics.total_tokens as f64)
            .field("prompt_tps", metrics.prompt_tps)
            .field("generation_tps", metrics.generation_tps)
            .field("prompt_ms", metrics.prompt_ms)
            .field("generation_ms", metrics.generation_ms)
            .field("duration_ms", metrics.duration_ms)
            .field("input_len", metrics.input_len as f64)
            .field("output_len", metrics.output_len as f64)
            .timestamp(metrics.timestamp.timestamp_nanos_opt().unwrap_or(0));

        let point = match point.build() {
            Ok(p) => p,
            Err(e) => {
                return Err(ExportError::Write(format!(
                    "Failed to build data point: {}",
                    e
                )));
            }
        };

        client
            .write(&self.config.bucket, vec![point])
            .await
            .map_err(|e| ExportError::Write(e.to_string()))?;

        Ok(())
    }

    #[cfg(not(feature = "influxdb"))]
    async fn export(&self, _metrics: &RequestMetrics) -> Result<(), ExportError> {
        // Feature not enabled, silently succeed
        Ok(())
    }

    fn name(&self) -> &str {
        "influxdb"
    }
}
