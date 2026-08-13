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
    /// Backend group name (only set in multi-backend mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
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
    /// Prompt processing tokens per second. Only meaningful when the backend
    /// reported a real prefill/decode split (llama.cpp `timings` or vLLM
    /// `metrics`); 0.0 otherwise.
    pub prompt_tps: f64,
    /// Generation tokens per second. Same caveat as `prompt_tps`.
    pub generation_tps: f64,
    /// Prompt processing time in ms. 0.0 when the backend did not report it.
    pub prompt_ms: f64,
    /// Generation time in ms. 0.0 when the backend did not report it.
    pub generation_ms: f64,
    /// total_tokens / wall clock. Always computable, and the only throughput
    /// figure available when the backend reports no split at all.
    pub total_tps: f64,
    /// True when prompt_tps/generation_tps came from the backend rather than
    /// being absent. Lets consumers tell "no split available" from "zero".
    pub has_timing_split: bool,
    /// Time the request spent queued before the engine scheduled it, in ms
    /// (vLLM only). This is the direct answer to "is concurrency making
    /// requests wait?" -- a rising queue time means saturation, which a falling
    /// per-stream decode rate on its own does not establish.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_ms: Option<f64>,
    /// Mean inter-token latency in ms (vLLM only). Its reciprocal is
    /// steady-state decode speed with prefill excluded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_itl_ms: Option<f64>,
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

    /// Number of in-flight requests at the time this request completed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrent_requests: Option<usize>,
}

impl RequestMetrics {
    /// Create a new metrics instance with defaults
    pub fn new() -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            model: "unknown".to_string(),
            group_name: None,
            client_id: None,
            conversation_id: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            prompt_tps: 0.0,
            generation_tps: 0.0,
            prompt_ms: 0.0,
            generation_ms: 0.0,
            total_tps: 0.0,
            has_timing_split: false,
            queue_ms: None,
            mean_itl_ms: None,
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
            concurrent_requests: None,
        }
    }

    /// Extract metrics from response and request
    pub fn from_response(response: &Value, request: &Value, streaming: bool, duration_ms: f64) -> Self {
        let mut metrics = Self::new();
        metrics.streaming = streaming;
        metrics.duration_ms = duration_ms;

        // Debug: Log the response structure
        tracing::debug!(
            "Extracting metrics from response: {}",
            serde_json::to_string(response).unwrap_or_else(|_| "invalid".to_string())
        );

        // Extract model (check both top-level and nested in message)
        if let Some(model) = response.get("model").and_then(|m| m.as_str()) {
            tracing::debug!("Found model at top level: {}", model);
            metrics.model = model.to_string();
        } else if let Some(model) = response.get("message").and_then(|m| m.get("model")).and_then(|m| m.as_str()) {
            tracing::debug!("Found model in message object: {}", model);
            metrics.model = model.to_string();
        } else {
            tracing::debug!("No model field found in response");
        }

        // Extract usage (support both OpenAI and Anthropic formats)
        if let Some(usage) = response.get("usage") {
            tracing::debug!("Found usage: {:?}", usage);

            // Try OpenAI format first
            if let Some(prompt) = usage.get("prompt_tokens").and_then(|t| t.as_u64()) {
                metrics.prompt_tokens = prompt;
                metrics.completion_tokens = usage.get("completion_tokens").and_then(|t| t.as_u64()).unwrap_or(0);
                metrics.total_tokens = usage
                    .get("total_tokens")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(metrics.prompt_tokens + metrics.completion_tokens);
            }
            // Try Anthropic format
            else if let Some(input) = usage.get("input_tokens").and_then(|t| t.as_u64()) {
                metrics.prompt_tokens = input;
                metrics.completion_tokens = usage.get("output_tokens").and_then(|t| t.as_u64()).unwrap_or(0);
                metrics.total_tokens = metrics.prompt_tokens + metrics.completion_tokens;
            }

            // Extract extended usage details (Opencode/Copilot extensions)
            if let Some(details) = usage.get("completion_tokens_details") {
                metrics.reasoning_tokens = details.get("reasoning_tokens").and_then(|t| t.as_u64());
                metrics.accepted_prediction_tokens = details.get("accepted_prediction_tokens").and_then(|t| t.as_u64());
                metrics.rejected_prediction_tokens = details.get("rejected_prediction_tokens").and_then(|t| t.as_u64());
            }
        } else {
            tracing::debug!("No usage field found in response");
        }

        // Extract timings (llama.cpp specific)
        if let Some(timings) = response.get("timings") {
            tracing::debug!("Found timings: {:?}", timings);
            metrics.prompt_ms = timings.get("prompt_ms").and_then(|t| t.as_f64()).unwrap_or(0.0);
            metrics.generation_ms = timings.get("predicted_ms").and_then(|t| t.as_f64()).unwrap_or(0.0);
            metrics.prompt_tps = timings.get("prompt_per_second").and_then(|t| t.as_f64()).unwrap_or(0.0);
            metrics.generation_tps = timings.get("predicted_per_second").and_then(|t| t.as_f64()).unwrap_or(0.0);
            metrics.has_timing_split = true;

            // Context info - use prompt_n for actual context consumption
            if let Some(prompt_n) = timings.get("prompt_n").and_then(|t| t.as_u64()) {
                metrics.context_used = Some(prompt_n);
            }

            // Fallback to timings for token counts when usage is missing (e.g., timeout scenarios)
            if metrics.prompt_tokens == 0 && metrics.completion_tokens == 0 {
                metrics.prompt_tokens = timings.get("prompt_n").and_then(|t| t.as_u64()).unwrap_or(0);
                metrics.completion_tokens = timings.get("predicted_n").and_then(|t| t.as_u64()).unwrap_or(0);
                metrics.total_tokens = metrics.prompt_tokens + metrics.completion_tokens;

                tracing::debug!(
                    "Using timings fallback for token counts: prompt={}, completion={}",
                    metrics.prompt_tokens,
                    metrics.completion_tokens
                );
            }
        } else {
            tracing::debug!("No timings field found in response");

            // If no timings, use prompt_tokens as context_used fallback
            // (Anthropic format responses have usage.input_tokens but no timings)
            if metrics.prompt_tokens > 0 {
                metrics.context_used = Some(metrics.prompt_tokens);
                tracing::debug!("Using prompt_tokens as context_used: {}", metrics.prompt_tokens);
            }

            // No `timings` means the backend did not tell us where the time went.
            // `timings` is llama.cpp-specific (prompt_per_second/predicted_per_second),
            // so every vLLM response lands here.
            //
            // This used to split the wall clock 20% prompt / 80% generation and
            // divide the token counts by those. That is not a measurement, and it
            // reads as one: on a vLLM server whose real prefill was measured at
            // ~14.5k tok/s it reported 113,344 tok/s, purely because a 39k-token
            // prompt was divided by 20% of a 1.7s request. The two figures were
            // also locked to each other -- prompt_tps/generation_tps was always
            // exactly 4 * prompt_tokens/completion_tokens -- so they carried no
            // information the token counts did not already carry, while making
            // short-completion requests look like a throughput collapse.
            //
            // Report only what a single duration can support: total throughput.
            // A real split needs either backend timings or a measured TTFT, which
            // is only observable on streaming responses.
            // vLLM reports the same information under `metrics`, but only when
            // the server was started with --enable-per-request-metrics. Without
            // that flag the key is present and null, which is exactly how this
            // code ended up estimating instead of measuring.
            //
            // Field semantics are vLLM's own: generation_time_ms is the decode
            // interval alone (first output token -> last), excluding queue wait
            // and prefill; time_to_first_token_ms is measured from scheduling,
            // so it excludes queue wait too.
            if let Some(vm) = response.get("metrics").filter(|v| !v.is_null()) {
                let f = |k: &str| vm.get(k).and_then(|v| v.as_f64()).filter(|v| *v > 0.0);

                metrics.queue_ms = f("queue_time_ms");
                metrics.mean_itl_ms = f("mean_itl_ms");

                if let Some(ttft) = f("time_to_first_token_ms") {
                    metrics.prompt_ms = ttft;
                    if metrics.prompt_tokens > 0 {
                        metrics.prompt_tps = (metrics.prompt_tokens as f64 / ttft) * 1000.0;
                    }
                }
                if let Some(gen_ms) = f("generation_time_ms") {
                    metrics.generation_ms = gen_ms;
                    if metrics.completion_tokens > 0 {
                        metrics.generation_tps = (metrics.completion_tokens as f64 / gen_ms) * 1000.0;
                    }
                }
                metrics.has_timing_split = metrics.prompt_ms > 0.0 || metrics.generation_ms > 0.0;

                tracing::debug!(
                    "vLLM per-request metrics: ttft={:.1}ms gen={:.1}ms queue={:?}ms itl={:?}ms",
                    metrics.prompt_ms,
                    metrics.generation_ms,
                    metrics.queue_ms,
                    metrics.mean_itl_ms
                );
            } else if duration_ms > 0.0 && metrics.total_tokens > 0 {
                tracing::debug!(
                    "No backend timings and no vLLM metrics (is the server missing \
                     --enable-per-request-metrics?); reporting total throughput only: {:.2} tok/s",
                    (metrics.total_tokens as f64 / duration_ms) * 1000.0
                );
            }
        }

        // Always available, whether or not the backend reported a split.
        if duration_ms > 0.0 && metrics.total_tokens > 0 {
            metrics.total_tps = (metrics.total_tokens as f64 / duration_ms) * 1000.0;
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
            let is_processing = slot.get("is_processing").and_then(|p| p.as_bool()).unwrap_or(false);

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
                "prompt_n": 42,
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
        assert_eq!(metrics.context_used, Some(42)); // Uses prompt_n, not cache_n
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

        let metrics = RequestMetrics::from_response(&response, &serde_json::json!({"messages": []}), false, 100.0);

        assert_eq!(metrics.reasoning_tokens, Some(20));
        assert_eq!(metrics.accepted_prediction_tokens, Some(5));
        assert_eq!(metrics.rejected_prediction_tokens, None);
    }

    #[test]
    fn test_request_metrics_from_response_timings_only() {
        // Scenario: streaming timeout where usage is missing but timings has token counts
        let response = serde_json::json!({
            "model": "test-model",
            "timings": {
                "prompt_n": 538,
                "predicted_n": 983,
                "prompt_ms": 316.829,
                "predicted_ms": 29669.411,
                "prompt_per_second": 1698.07,
                "predicted_per_second": 33.13
            },
            "prompt_progress": {"total": 562},
            "choices": [{"finish_reason": null, "delta": {"content": null}}]
        });

        let metrics = RequestMetrics::from_response(&response, &serde_json::json!({"messages": []}), true, 30181.0);

        assert_eq!(metrics.prompt_tokens, 538);
        assert_eq!(metrics.completion_tokens, 983);
        assert_eq!(metrics.total_tokens, 1521);
        assert_eq!(metrics.context_used, Some(538)); // Uses prompt_n, not cache_n
    }

    #[test]
    fn test_request_metrics_from_anthropic_merged() {
        // This simulates the merged response from Anthropic SSE events
        let response = serde_json::json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "Qwen3-14B-128K-Q3_K_S.gguf",
            "content": [
                {"type": "text", "text": "Hello world"}
            ],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 124,
                "output_tokens": 273
            }
        });

        let request = serde_json::json!({
            "messages": [{"role": "user", "content": "Hi"}]
        });

        let metrics = RequestMetrics::from_response(&response, &request, true, 8000.0);

        assert_eq!(metrics.model, "Qwen3-14B-128K-Q3_K_S.gguf");
        assert_eq!(metrics.prompt_tokens, 124);
        assert_eq!(metrics.completion_tokens, 273);
        assert_eq!(metrics.total_tokens, 397);
        assert_eq!(metrics.finish_reason, "end_turn");
        assert_eq!(metrics.output_len, 11); // "Hello world"
        assert!(metrics.streaming);
        // Verify context_used fallback (Anthropic format has no timings)
        assert_eq!(metrics.context_used, Some(124));
    }

    #[test]
    fn test_request_metrics_from_anthropic_with_thinking() {
        // Anthropic response with thinking content block
        let response = serde_json::json!({
            "id": "msg_2",
            "type": "message",
            "role": "assistant",
            "model": "claude-3",
            "content": [
                {"type": "thinking", "thinking": "Let me reason..."},
                {"type": "text", "text": "The answer is 42"}
            ],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 200
            }
        });

        let request = serde_json::json!({
            "messages": [{"role": "user", "content": "What is the answer?"}]
        });

        let metrics = RequestMetrics::from_response(&response, &request, false, 5000.0);

        assert_eq!(metrics.model, "claude-3");
        assert_eq!(metrics.prompt_tokens, 100);
        assert_eq!(metrics.completion_tokens, 200);
        // Output len should only count text content, not thinking
        assert_eq!(metrics.output_len, 16); // "The answer is 42"
        assert_eq!(metrics.finish_reason, "end_turn");
        // Verify context_used fallback (Anthropic format has no timings)
        assert_eq!(metrics.context_used, Some(100));
    }

    /// vLLM responses carry `usage` but no llama.cpp `timings`. That path used to
    /// fabricate a prefill/decode split from a hardcoded 20%/80% division of the
    /// wall clock. Assert it reports total throughput and leaves the split empty.
    #[test]
    fn test_no_timings_reports_total_only() {
        let response = serde_json::json!({
            "model": "cosmo-6000",
            "usage": {"prompt_tokens": 39240, "completion_tokens": 146, "total_tokens": 39386},
            "choices": [{"finish_reason": "tool_calls", "message": {"content": ""}}]
        });
        let request = serde_json::json!({"messages": []});
        let m = RequestMetrics::from_response(&response, &request, false, 1731.0);

        assert!(!m.has_timing_split);
        assert_eq!(m.prompt_tps, 0.0, "must not invent a prefill rate");
        assert_eq!(m.generation_tps, 0.0, "must not invent a decode rate");
        // the old code produced 113344.89 here, on a server measured at ~14.5k tok/s
        assert!((m.total_tps - 22753.32).abs() < 0.1, "total_tps was {}", m.total_tps);
    }

    /// A backend that does report timings keeps the real split.
    #[test]
    fn test_timings_present_keeps_split() {
        let response = serde_json::json!({
            "model": "m",
            "usage": {"prompt_tokens": 100, "completion_tokens": 50, "total_tokens": 150},
            "timings": {"prompt_per_second": 200.5, "predicted_per_second": 42.5,
                        "prompt_ms": 500.0, "predicted_ms": 1176.0},
            "choices": [{"finish_reason": "stop", "message": {"content": ""}}]
        });
        let request = serde_json::json!({"messages": []});
        let m = RequestMetrics::from_response(&response, &request, false, 1676.0);

        assert!(m.has_timing_split);
        assert_eq!(m.prompt_tps, 200.5);
        assert_eq!(m.generation_tps, 42.5);
        assert!(m.total_tps > 0.0, "total_tps should still be populated");
    }

    /// vLLM with --enable-per-request-metrics returns a `metrics` object. It must
    /// be used instead of the total-throughput fallback, and must produce a real
    /// split rather than the old 20/80 estimate.
    #[test]
    fn test_vllm_per_request_metrics_are_used() {
        let response = serde_json::json!({
            "model": "cosmo-6000",
            "usage": {"prompt_tokens": 16, "completion_tokens": 120, "total_tokens": 136},
            "metrics": {
                "time_to_first_token_ms": 291.44680208992213,
                "generation_time_ms": 760.3732609422877,
                "queue_time_ms": 12.5,
                "mean_itl_ms": 6.3896912684225855,
                "tokens_per_second": 114.0879549816357
            },
            "choices": [{"finish_reason": "stop", "message": {"content": ""}}]
        });
        let request = serde_json::json!({"messages": []});
        let m = RequestMetrics::from_response(&response, &request, false, 1100.0);

        assert!(m.has_timing_split, "vLLM metrics must count as a real split");
        assert!((m.prompt_ms - 291.4468).abs() < 0.01);
        assert!((m.generation_ms - 760.3733).abs() < 0.01);
        // 120 tokens over the decode interval -- matches the 157.6 tok/s measured
        // independently by scripts/qwen-conc-sweep for this config.
        assert!((m.generation_tps - 157.82).abs() < 0.1, "got {}", m.generation_tps);
        assert_eq!(m.queue_ms, Some(12.5));
        assert!((m.mean_itl_ms.unwrap() - 6.3897).abs() < 0.01);
    }

    /// A null `metrics` (server without --enable-per-request-metrics) must fall
    /// back to total throughput, NOT to an invented split.
    #[test]
    fn test_null_vllm_metrics_falls_back() {
        let response = serde_json::json!({
            "model": "cosmo-6000",
            "usage": {"prompt_tokens": 39240, "completion_tokens": 146, "total_tokens": 39386},
            "metrics": serde_json::Value::Null,
            "choices": [{"finish_reason": "tool_calls", "message": {"content": ""}}]
        });
        let request = serde_json::json!({"messages": []});
        let m = RequestMetrics::from_response(&response, &request, false, 1731.0);

        assert!(!m.has_timing_split);
        assert_eq!(m.prompt_tps, 0.0);
        assert_eq!(m.generation_tps, 0.0);
        assert_eq!(m.queue_ms, None);
        assert!((m.total_tps - 22753.32).abs() < 0.1);
    }
}
