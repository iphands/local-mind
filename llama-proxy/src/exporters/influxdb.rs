//! InfluxDB v2 metrics exporter

use async_trait::async_trait;
#[cfg(feature = "influxdb")]
use futures::stream;

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

/// The single honest answer for "requested while compiled out". One string for
/// both gates (config-time [`InfluxDbExporter::from_config`] and the export
/// backstop) so no path can downgrade it to a silent success.
#[cfg(not(feature = "influxdb"))]
fn feature_off_error() -> ExportError {
    ExportError::Config("influxdb enabled in config but binary built without the influxdb feature".to_string())
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
        Ok(Self { config, _phantom: () })
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
    #[cfg(feature = "influxdb")]
    async fn export(&self, metrics: &RequestMetrics) -> Result<(), ExportError> {
        let client = match &self.client {
            Some(c) => c,
            None => return Ok(()),
        };

        let point = build_data_point(metrics)?;

        client
            .write(&self.config.bucket, stream::iter(vec![point]))
            .await
            .map_err(|e| ExportError::Write(e.to_string()))?;

        Ok(())
    }

    #[cfg(not(feature = "influxdb"))]
    async fn export(&self, _metrics: &RequestMetrics) -> Result<(), ExportError> {
        Err(feature_off_error())
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
