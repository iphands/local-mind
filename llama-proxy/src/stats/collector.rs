//! Metrics collection from LLM responses

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// Collected metrics from a request/response cycle
#[derive(Debug, Clone, Serialize)]
pub struct RequestMetrics {
    /// Unique request ID
    pub request_id: String,
    /// Timestamp of the request
    pub timestamp: DateTime<Utc>,
    /// Model name
    pub model: String,
    /// Client identifier (from header or generated)
    pub client_id: Option<String>,
    /// Conversation/session ID
    pub conversation_id: Option<String>,
    /// Number of prompt tokens
    pub prompt_tokens: u64,
    /// Number of completion tokens
    pub completion_tokens: u64,
    /// Total tokens
    pub total_tokens: u64,
    /// Prompt processing tokens per second
    pub prompt_tps: f64,
    /// Generation tokens per second
    pub generation_tps: f64,
    /// Prompt processing time in ms
    pub prompt_ms: f64,
    /// Generation time in ms
    pub generation_ms: f64,
    /// Total context size (n_ctx)
    pub context_total: Option<u64>,
    /// Context tokens used
    pub context_used: Option<u64>,
    /// Context usage percentage
    pub context_percent: Option<f64>,
    /// Input message count
    pub input_messages: usize,
    /// Input length (approximate characters)
    pub input_len: usize,
    /// Output length (characters)
    pub output_len: usize,
    /// Whether this was a streaming request
    pub streaming: bool,
    /// Finish reason
    pub finish_reason: String,
    /// Request duration in ms
    pub duration_ms: f64,
}

impl RequestMetrics {
    /// Create a new metrics instance with defaults
    pub fn new() -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            model: "unknown".to_string(),
            client_id: None,
            conversation_id: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            prompt_tps: 0.0,
            generation_tps: 0.0,
            prompt_ms: 0.0,
            generation_ms: 0.0,
            context_total: None,
            context_used: None,
            context_percent: None,
            input_messages: 0,
            input_len: 0,
            output_len: 0,
            streaming: false,
            finish_reason: "unknown".to_string(),
            duration_ms: 0.0,
        }
    }

    /// Extract metrics from response and request
    pub fn from_response(
        response: &Value,
        request: &Value,
        streaming: bool,
        duration_ms: f64,
    ) -> Self {
        let mut metrics = Self::new();
        metrics.streaming = streaming;
        metrics.duration_ms = duration_ms;

        // Extract model
        if let Some(model) = response.get("model").and_then(|m| m.as_str()) {
            metrics.model = model.to_string();
        }

        // Extract usage
        if let Some(usage) = response.get("usage") {
            metrics.prompt_tokens = usage
                .get("prompt_tokens")
                .and_then(|t| t.as_u64())
                .unwrap_or(0);
            metrics.completion_tokens = usage
                .get("completion_tokens")
                .and_then(|t| t.as_u64())
                .unwrap_or(0);
            metrics.total_tokens = usage
                .get("total_tokens")
                .and_then(|t| t.as_u64())
                .unwrap_or(0);
        }

        // Extract timings (llama.cpp specific)
        if let Some(timings) = response.get("timings") {
            metrics.prompt_ms = timings
                .get("prompt_ms")
                .and_then(|t| t.as_f64())
                .unwrap_or(0.0);
            metrics.generation_ms = timings
                .get("predicted_ms")
                .and_then(|t| t.as_f64())
                .unwrap_or(0.0);
            metrics.prompt_tps = timings
                .get("prompt_per_second")
                .and_then(|t| t.as_f64())
                .unwrap_or(0.0);
            metrics.generation_tps = timings
                .get("predicted_per_second")
                .and_then(|t| t.as_f64())
                .unwrap_or(0.0);

            // Context info
            if let Some(cache_n) = timings.get("cache_n").and_then(|t| t.as_u64()) {
                metrics.context_used = Some(cache_n);
            }
        }

        // Extract finish reason
        if let Some(choices) = response.get("choices").and_then(|c| c.as_array()) {
            if let Some(first_choice) = choices.first() {
                metrics.finish_reason = first_choice
                    .get("finish_reason")
                    .and_then(|f| f.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                // Extract output length
                if let Some(content) = first_choice
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                {
                    metrics.output_len = content.len();
                }
            }
        }

        // Extract request info
        if let Some(messages) = request.get("messages").and_then(|m| m.as_array()) {
            metrics.input_messages = messages.len();
            metrics.input_len = messages
                .iter()
                .map(|m| {
                    m.get("content")
                        .and_then(|c| match c {
                            Value::String(s) => Some(s.len()),
                            Value::Array(arr) => Some(
                                arr.iter()
                                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                                    .map(|s| s.len())
                                    .sum(),
                            ),
                            _ => None,
                        })
                        .unwrap_or(0)
                })
                .sum();
        }

        metrics
    }

    /// Calculate context percentage
    pub fn calculate_context_percent(&mut self) {
        if let (Some(used), Some(total)) = (self.context_used, self.context_total) {
            if total > 0 {
                self.context_percent = Some((used as f64 / total as f64) * 100.0);
            }
        }
    }
}

impl Default for RequestMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Context information from slots endpoint
#[derive(Debug, Clone)]
pub struct ContextInfo {
    pub total_context: u64,
    pub used_context: u64,
    pub slots: Vec<SlotMetrics>,
}

/// Per-slot metrics
#[derive(Debug, Clone)]
pub struct SlotMetrics {
    pub slot_id: u32,
    pub n_tokens: u64,
    pub n_ctx: u64,
    pub is_processing: bool,
}

impl ContextInfo {
    /// Parse from /slots response
    pub fn from_slots_response(response: &Value) -> Option<Self> {
        let slots = response.as_array()?;
        let mut total_context = 0;
        let mut used_context = 0;
        let mut slot_metrics = Vec::new();

        for slot in slots {
            let slot_id = slot.get("id").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
            let n_ctx = slot.get("n_ctx").and_then(|n| n.as_u64()).unwrap_or(0);
            let n_tokens = slot.get("n_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
            let is_processing = slot
                .get("is_processing")
                .and_then(|p| p.as_bool())
                .unwrap_or(false);

            total_context += n_ctx;
            used_context += n_tokens;

            slot_metrics.push(SlotMetrics {
                slot_id,
                n_tokens,
                n_ctx,
                is_processing,
            });
        }

        Some(Self {
            total_context,
            used_context,
            slots: slot_metrics,
        })
    }
}
