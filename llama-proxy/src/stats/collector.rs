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

    // Extended token details (Opencode/Copilot extensions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_prediction_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_prediction_tokens: Option<u64>,
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
            reasoning_tokens: None,
            accepted_prediction_tokens: None,
            rejected_prediction_tokens: None,
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

        // Debug: Log the response structure
        tracing::debug!("Extracting metrics from response: {}", serde_json::to_string(response).unwrap_or_else(|_| "invalid".to_string()));

        // Extract model
        if let Some(model) = response.get("model").and_then(|m| m.as_str()) {
            metrics.model = model.to_string();
        }

        // Extract usage (support both OpenAI and Anthropic formats)
        if let Some(usage) = response.get("usage") {
            tracing::debug!("Found usage: {:?}", usage);

            // Try OpenAI format first
            if let Some(prompt) = usage.get("prompt_tokens").and_then(|t| t.as_u64()) {
                metrics.prompt_tokens = prompt;
                metrics.completion_tokens = usage
                    .get("completion_tokens")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0);
                metrics.total_tokens = usage
                    .get("total_tokens")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(metrics.prompt_tokens + metrics.completion_tokens);
            }
            // Try Anthropic format
            else if let Some(input) = usage.get("input_tokens").and_then(|t| t.as_u64()) {
                metrics.prompt_tokens = input;
                metrics.completion_tokens = usage
                    .get("output_tokens")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0);
                metrics.total_tokens = metrics.prompt_tokens + metrics.completion_tokens;
            }

            // Extract extended usage details (Opencode/Copilot extensions)
            if let Some(details) = usage.get("completion_tokens_details") {
                metrics.reasoning_tokens = details
                    .get("reasoning_tokens")
                    .and_then(|t| t.as_u64());
                metrics.accepted_prediction_tokens = details
                    .get("accepted_prediction_tokens")
                    .and_then(|t| t.as_u64());
                metrics.rejected_prediction_tokens = details
                    .get("rejected_prediction_tokens")
                    .and_then(|t| t.as_u64());
            }
        } else {
            tracing::debug!("No usage field found in response");
        }

        // Extract timings (llama.cpp specific)
        if let Some(timings) = response.get("timings") {
            tracing::debug!("Found timings: {:?}", timings);
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
        } else {
            tracing::debug!("No timings field found in response");

            // If no timings, estimate TPS from duration and token counts
            if duration_ms > 0.0 && metrics.total_tokens > 0 {
                // Estimate: assume 20% of time for prompt, 80% for generation
                let estimated_prompt_ms = duration_ms * 0.2;
                let estimated_generation_ms = duration_ms * 0.8;

                if metrics.prompt_tokens > 0 && estimated_prompt_ms > 0.0 {
                    metrics.prompt_tps = (metrics.prompt_tokens as f64 / estimated_prompt_ms) * 1000.0;
                    metrics.prompt_ms = estimated_prompt_ms;
                }

                if metrics.completion_tokens > 0 && estimated_generation_ms > 0.0 {
                    metrics.generation_tps =
                        (metrics.completion_tokens as f64 / estimated_generation_ms) * 1000.0;
                    metrics.generation_ms = estimated_generation_ms;
                }

                tracing::debug!(
                    "Estimated TPS - prompt: {:.2}, generation: {:.2}",
                    metrics.prompt_tps,
                    metrics.generation_tps
                );
            }
        }

        // Extract finish reason and output length (support both OpenAI and Anthropic formats)
        // Try OpenAI format first
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
        // Try Anthropic format
        else if let Some(stop_reason) = response.get("stop_reason").and_then(|f| f.as_str()) {
            metrics.finish_reason = stop_reason.to_string();

            // Extract output length from content array
            if let Some(content_array) = response.get("content").and_then(|c| c.as_array()) {
                metrics.output_len = content_array
                    .iter()
                    .filter_map(|item| {
                        // Sum up text content lengths
                        item.get("text").and_then(|t| t.as_str()).map(|s| s.len())
                    })
                    .sum();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_metrics_new() {
        let metrics = RequestMetrics::new();

        assert!(!metrics.request_id.is_empty());
        assert_eq!(metrics.model, "unknown");
        assert!(metrics.client_id.is_none());
        assert!(metrics.conversation_id.is_none());
        assert_eq!(metrics.prompt_tokens, 0);
        assert_eq!(metrics.completion_tokens, 0);
        assert_eq!(metrics.total_tokens, 0);
        assert_eq!(metrics.prompt_tps, 0.0);
        assert_eq!(metrics.generation_tps, 0.0);
        assert!(!metrics.streaming);
        assert_eq!(metrics.finish_reason, "unknown");
    }

    #[test]
    fn test_request_metrics_default() {
        let metrics = RequestMetrics::default();
        assert_eq!(metrics.model, "unknown");
    }

    #[test]
    fn test_request_metrics_from_response_basic() {
        let response = serde_json::json!({
            "model": "test-model",
            "choices": [{
                "finish_reason": "stop",
                "message": {"content": "Hello world"}
            }]
        });

        let request = serde_json::json!({
            "messages": [{"role": "user", "content": "Hi"}]
        });

        let metrics = RequestMetrics::from_response(&response, &request, false, 100.0);

        assert_eq!(metrics.model, "test-model");
        assert_eq!(metrics.finish_reason, "stop");
        assert_eq!(metrics.output_len, 11); // "Hello world"
        assert!(!metrics.streaming);
        assert_eq!(metrics.duration_ms, 100.0);
    }

    #[test]
    fn test_request_metrics_from_response_with_usage() {
        let response = serde_json::json!({
            "model": "test-model",
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150
            },
            "choices": [{"finish_reason": "stop"}]
        });

        let request = serde_json::json!({});

        let metrics = RequestMetrics::from_response(&response, &request, true, 200.0);

        assert_eq!(metrics.prompt_tokens, 100);
        assert_eq!(metrics.completion_tokens, 50);
        assert_eq!(metrics.total_tokens, 150);
        assert!(metrics.streaming);
    }

    #[test]
    fn test_request_metrics_from_response_with_timings() {
        let response = serde_json::json!({
            "model": "test-model",
            "timings": {
                "prompt_ms": 50.5,
                "predicted_ms": 100.25,
                "prompt_per_second": 198.0,
                "predicted_per_second": 99.75,
                "cache_n": 10
            },
            "choices": [{"finish_reason": "stop"}]
        });

        let request = serde_json::json!({});

        let metrics = RequestMetrics::from_response(&response, &request, false, 150.0);

        assert_eq!(metrics.prompt_ms, 50.5);
        assert_eq!(metrics.generation_ms, 100.25);
        assert_eq!(metrics.prompt_tps, 198.0);
        assert_eq!(metrics.generation_tps, 99.75);
        assert_eq!(metrics.context_used, Some(10));
    }

    #[test]
    fn test_request_metrics_from_response_with_messages() {
        let response = serde_json::json!({
            "model": "test-model",
            "choices": [{"finish_reason": "stop"}]
        });

        let request = serde_json::json!({
            "messages": [
                {"role": "system", "content": "You are helpful"},
                {"role": "user", "content": "Hello there"}
            ]
        });

        let metrics = RequestMetrics::from_response(&response, &request, false, 50.0);

        assert_eq!(metrics.input_messages, 2);
        // "You are helpful" (15) + "Hello there" (11) = 26
        assert_eq!(metrics.input_len, 26);
    }

    #[test]
    fn test_request_metrics_from_response_multimodal_content() {
        let response = serde_json::json!({
            "model": "test-model",
            "choices": [{"finish_reason": "stop"}]
        });

        let request = serde_json::json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What's in this image?"},
                    {"type": "image_url", "image_url": {"url": "http://example.com/image.png"}}
                ]
            }]
        });

        let metrics = RequestMetrics::from_response(&response, &request, false, 50.0);

        assert_eq!(metrics.input_messages, 1);
        assert_eq!(metrics.input_len, 21); // "What's in this image?"
    }

    #[test]
    fn test_request_metrics_from_response_no_choices() {
        let response = serde_json::json!({
            "model": "test-model"
        });

        let request = serde_json::json!({});

        let metrics = RequestMetrics::from_response(&response, &request, false, 50.0);

        assert_eq!(metrics.finish_reason, "unknown");
        assert_eq!(metrics.output_len, 0);
    }

    #[test]
    fn test_request_metrics_calculate_context_percent() {
        let mut metrics = RequestMetrics::new();
        metrics.context_used = Some(50);
        metrics.context_total = Some(100);

        metrics.calculate_context_percent();

        assert_eq!(metrics.context_percent, Some(50.0));
    }

    #[test]
    fn test_request_metrics_calculate_context_percent_zero_total() {
        let mut metrics = RequestMetrics::new();
        metrics.context_used = Some(50);
        metrics.context_total = Some(0);

        metrics.calculate_context_percent();

        assert_eq!(metrics.context_percent, None);
    }

    #[test]
    fn test_request_metrics_calculate_context_percent_missing_values() {
        let mut metrics = RequestMetrics::new();

        metrics.calculate_context_percent();

        assert_eq!(metrics.context_percent, None);
    }

    #[test]
    fn test_request_metrics_serialize() {
        let metrics = RequestMetrics::new();
        let json = serde_json::to_string(&metrics);
        assert!(json.is_ok());
        assert!(json.unwrap().contains("request_id"));
    }

    #[test]
    fn test_context_info_from_slots_response() {
        let response = serde_json::json!([
            {
                "id": 0,
                "n_ctx": 4096,
                "n_tokens": 100,
                "is_processing": true
            },
            {
                "id": 1,
                "n_ctx": 4096,
                "n_tokens": 50,
                "is_processing": false
            }
        ]);

        let info = ContextInfo::from_slots_response(&response);

        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.total_context, 8192);
        assert_eq!(info.used_context, 150);
        assert_eq!(info.slots.len(), 2);
        assert_eq!(info.slots[0].slot_id, 0);
        assert!(info.slots[0].is_processing);
        assert!(!info.slots[1].is_processing);
    }

    #[test]
    fn test_context_info_from_slots_response_empty() {
        let response = serde_json::json!([]);
        let info = ContextInfo::from_slots_response(&response);

        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.total_context, 0);
        assert_eq!(info.used_context, 0);
        assert!(info.slots.is_empty());
    }

    #[test]
    fn test_context_info_from_slots_response_not_array() {
        let response = serde_json::json!({"not": "array"});
        let info = ContextInfo::from_slots_response(&response);
        assert!(info.is_none());
    }

    #[test]
    fn test_context_info_from_slots_response_partial_data() {
        let response = serde_json::json!([
            {"id": 0}, // Missing other fields
            {"n_ctx": 2048, "n_tokens": 25}
        ]);

        let info = ContextInfo::from_slots_response(&response);

        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.slots.len(), 2);
        assert_eq!(info.slots[0].n_ctx, 0); // Default
        assert_eq!(info.slots[1].slot_id, 0); // Default
    }

    #[test]
    fn test_extended_usage_extraction() {
        let response = serde_json::json!({
            "choices": [{
                "message": {"role": "assistant", "content": "test"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150,
                "completion_tokens_details": {
                    "reasoning_tokens": 20,
                    "accepted_prediction_tokens": 5
                }
            }
        });

        let metrics = RequestMetrics::from_response(
            &response,
            &serde_json::json!({"messages": []}),
            false,
            100.0
        );

        assert_eq!(metrics.reasoning_tokens, Some(20));
        assert_eq!(metrics.accepted_prediction_tokens, Some(5));
        assert_eq!(metrics.rejected_prediction_tokens, None);
    }
}
