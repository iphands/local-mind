//! Streaming response handling (SSE)

use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Instant;

use crate::config::StatsFormat;
use crate::exporters::ExporterManager;
use crate::fixes::{FixRegistry, ToolCallAccumulator};
use crate::proxy::fetch_context_total;
use crate::stats::{format_metrics, RequestMetrics};

/// Handle a streaming (SSE) response from the backend
#[allow(clippy::too_many_arguments)]
pub async fn handle_streaming_response(
    backend_response: reqwest::Response,
    fix_registry: Arc<FixRegistry>,
    stats_enabled: bool,
    stats_format: StatsFormat,
    exporter_manager: Arc<ExporterManager>,
    request_json: Option<serde_json::Value>,
    start: Instant,
    http_client: reqwest::Client,
    backend_url: String,
) -> Response {
    let status = backend_response.status();
    let headers = backend_response.headers().clone();

    // Create a stream that processes each SSE event
    let stream = backend_response.bytes_stream();

    // Accumulate response for stats
    let accumulated = Arc::new(tokio::sync::Mutex::new(String::new()));
    let accumulated_clone = accumulated.clone();

    // Accumulator for tool call argument fixing
    let tool_call_accumulator =
        Arc::new(tokio::sync::Mutex::new(ToolCallAccumulator::new()));
    let tool_call_accumulator_clone = tool_call_accumulator.clone();

    // Create oneshot channel for stream completion signaling
    let (completion_tx, completion_rx) = tokio::sync::oneshot::channel::<()>();
    let completion_tx = Arc::new(tokio::sync::Mutex::new(Some(completion_tx)));
    let completion_tx_clone = completion_tx.clone();

    // Activity signaling for adaptive timeout (watch channel)
    let (activity_tx, activity_rx) = tokio::sync::watch::channel(());
    let activity_tx = Arc::new(tokio::sync::Mutex::new(activity_tx));
    let activity_tx_clone = activity_tx.clone();

    // Clone request_json for use in stream processing
    let request_json_for_stream = request_json.clone();

    let processed_stream = stream.then(move |chunk_result| {
        let fix_registry = fix_registry.clone();
        let accumulated = accumulated_clone.clone();
        let tool_call_accumulator = tool_call_accumulator_clone.clone();
        let stats_enabled = stats_enabled;
        let completion_tx = completion_tx_clone.clone();
        let activity_tx = activity_tx_clone.clone();
        let request_json = request_json_for_stream.clone();

        async move {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(error = %e, "Error reading stream chunk");
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ));
                }
            };

            let chunk_str = String::from_utf8_lossy(&chunk);

            tracing::trace!("Raw SSE chunk ({} bytes): {:?}", chunk.len(), chunk_str.as_ref());

            // Process each SSE event, preserving the exact format including blank lines
            let mut output = String::new();

            // Split by newlines but preserve empty strings (blank lines)
            let lines: Vec<&str> = chunk_str.split('\n').collect();

            // Track current event type for Anthropic format
            let mut current_event_type: Option<String> = None;

            for (i, line) in lines.iter().enumerate() {
                // Handle event: lines (Anthropic format uses these)
                if line.starts_with("event: ") {
                    current_event_type = Some(line[7..].to_string());
                    output.push_str(line);
                    if i < lines.len() - 1 {
                        output.push('\n');
                    }
                    continue;
                }

                if line.starts_with("data: ") {
                    let data = &line[6..];

                    tracing::trace!(
                        "SSE data (event={:?}): {}",
                        current_event_type,
                        data
                    );

                    // Check for Anthropic completion event (message_stop)
                    if current_event_type.as_deref() == Some("message_stop") {
                        // Signal completion for Anthropic streams
                        if stats_enabled {
                            tracing::trace!("Anthropic stream completion detected (message_stop event)");
                            if let Some(tx) = completion_tx.lock().await.take() {
                                let _ = tx.send(());
                            }
                        }
                    }

                    if data == "[DONE]" {
                        // Signal completion to metrics task (OpenAI format)
                        if stats_enabled {
                            tracing::trace!("OpenAI stream completion detected ([DONE] marker)");
                            if let Some(tx) = completion_tx.lock().await.take() {
                                let _ = tx.send(());
                            }
                        }

                        output.push_str(line);
                        if i < lines.len() - 1 {
                            output.push('\n');
                        }
                        continue;
                    }

                    // Parse and apply fixes
                    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(data) {
                        // Apply fixes with accumulation support
                        json = if let Some(ref req_json) = request_json {
                            let mut acc = tool_call_accumulator.lock().await;
                            fix_registry.apply_fixes_stream_with_accumulation(json, req_json, &mut acc)
                        } else {
                            let mut acc = tool_call_accumulator.lock().await;
                            fix_registry.apply_fixes_stream_with_accumulation_default(json, &mut acc)
                        };

                        // Accumulate for stats (include event type for Anthropic format)
                        if stats_enabled {
                            let mut acc = accumulated.lock().await;
                            // Store event type if present (Anthropic format)
                            if let Some(ref event_type) = current_event_type {
                                acc.push_str("event: ");
                                acc.push_str(event_type);
                                acc.push('\n');
                            }
                            acc.push_str("data: ");
                            acc.push_str(&serde_json::to_string(&json).unwrap_or_default());
                            acc.push('\n');
                        }

                        output.push_str("data: ");
                        output.push_str(&serde_json::to_string(&json).unwrap_or_default());
                    } else {
                        // Parse failed - pass through unchanged
                        output.push_str(line);
                    }
                    // Reset event type after using it
                    current_event_type = None;
                } else {
                    // Preserve empty lines and other SSE format lines (event:, id:, etc.)
                    output.push_str(line);
                }

                // Add newline unless this is the last item and it's empty
                if i < lines.len() - 1 {
                    output.push('\n');
                }
            }

            // Signal activity for adaptive timeout (only if we have meaningful output)
            if !output.is_empty() {
                let _ = activity_tx.lock().await.send(());
            }

            Ok(Bytes::from(output))
        }
    });

    // Spawn task to collect stats after stream completes
    if stats_enabled {
        let exporter_manager = exporter_manager.clone();
        let client = http_client.clone();
        let backend_url = backend_url.clone();

        tokio::spawn(async move {
            // Adaptive timeout constants
            const ACTIVITY_TIMEOUT_SECS: u64 = 90;   // Reset on each chunk
            const ABSOLUTE_TIMEOUT_SECS: u64 = 600;  // 10 minutes hard limit

            // Wait for stream completion with adaptive timeout
            let absolute_deadline = tokio::time::Instant::now()
                + tokio::time::Duration::from_secs(ABSOLUTE_TIMEOUT_SECS);

            let mut completion_rx = completion_rx;
            let mut activity_rx = activity_rx;
            let mut completed = false;

            loop {
                // Create fresh activity timeout each iteration (resets on activity)
                let activity_timeout_sleep = tokio::time::sleep(
                    tokio::time::Duration::from_secs(ACTIVITY_TIMEOUT_SECS)
                );
                tokio::pin!(activity_timeout_sleep);

                // Calculate remaining time until absolute deadline
                let absolute_remaining = absolute_deadline
                    .checked_duration_since(tokio::time::Instant::now())
                    .unwrap_or(tokio::time::Duration::ZERO);
                let absolute_timeout_sleep = tokio::time::sleep(absolute_remaining);
                tokio::pin!(absolute_timeout_sleep);

                tokio::select! {
                    // Completion signal from stream
                    result = &mut completion_rx => {
                        match result {
                            Ok(()) => {
                                tracing::trace!("Stream completion signal received");
                                completed = true;
                            }
                            Err(_) => {
                                tracing::debug!("Stream completion channel closed unexpectedly");
                            }
                        }
                        break;
                    }

                    // Activity detected - reset the activity timeout by continuing loop
                    res = activity_rx.changed() => {
                        if res.is_ok() {
                            tracing::trace!("Stream activity detected, resetting timeout");
                        } else {
                            // Channel closed - stream ended without completion signal
                            tracing::trace!("Activity channel closed, stream ended");
                        }
                        // Continue loop with fresh timers
                        continue;
                    }

                    // 90s inactivity timeout
                    _ = &mut activity_timeout_sleep => {
                        tracing::warn!(
                            "Stream inactivity timeout ({}s since last chunk), extracting metrics",
                            ACTIVITY_TIMEOUT_SECS
                        );
                        break;
                    }

                    // 5 minute absolute timeout
                    _ = absolute_timeout_sleep => {
                        tracing::warn!(
                            "Stream absolute timeout reached ({}s total), extracting metrics",
                            ABSOLUTE_TIMEOUT_SECS
                        );
                        break;
                    }
                }
            }

            // Log the reason we're extracting metrics
            if completed {
                tracing::trace!(
                    duration_ms = start.elapsed().as_millis() as u64,
                    "Stream completed normally"
                );
            }

            let acc = accumulated.lock().await;
            tracing::trace!("Accumulated SSE data length: {} bytes", acc.len());
            if !acc.is_empty() {
                let preview = if acc.len() > 1000 { &acc[..1000] } else { &acc[..] };
                tracing::trace!("Accumulated SSE preview:\n{}", preview);
                
                if let Some(final_event) = parse_accumulated_sse(&acc) {
                    tracing::trace!(
                        "Successfully merged SSE events into final response with keys: {:?}",
                        final_event.as_object().map(|o| o.keys().collect::<Vec<_>>())
                    );
                    // Extract metrics from final event
                    let mut metrics = if let Some(ref req_json) = request_json {
                        RequestMetrics::from_response(
                            &final_event,
                            req_json,
                            true, // streaming
                            start.elapsed().as_millis() as f64,
                        )
                    } else {
                        tracing::trace!(
                            duration_ms = start.elapsed().as_millis() as u64,
                            "Streaming completed (no request JSON)"
                        );
                        return;
                    };

                    // Fetch and set context_total
                    if let Some(ctx_total) = fetch_context_total(&client, &backend_url).await {
                        metrics.context_total = Some(ctx_total);
                        metrics.calculate_context_percent();
                    }

                    // Format and log stats
                    let formatted = format_metrics(&metrics, stats_format);
                    if stats_format == StatsFormat::Compact {
                        tracing::info!("{}", formatted);
                    } else {
                        tracing::info!("\n{}", formatted);
                    }

                    // Export to remote systems
                    exporter_manager.export_all(&metrics).await;
                } else {
                    tracing::trace!(
                        duration_ms = start.elapsed().as_millis() as u64,
                        "Streaming completed (unable to parse final event)"
                    );
                }
            }
        });
    }

    // Build streaming response
    let mut response = Response::builder().status(status);

    for (name, value) in headers {
        if let Some(name) = name {
            // Skip content-length as we're streaming
            if name != axum::http::header::CONTENT_LENGTH {
                response = response.header(name, value);
            }
        }
    }

    let body = Body::from_stream(processed_stream);
    response.body(body).unwrap().into_response()
}

/// API format detected from SSE stream
#[derive(Debug, Clone, Copy, PartialEq)]
enum StreamFormat {
    OpenAI,
    Anthropic,
    Unknown,
}

/// Detect API format from SSE data
fn detect_format(data: &str) -> StreamFormat {
    // Look for Anthropic markers: event: message_start or "type": "message_start" in data
    for line in data.lines() {
        if line.starts_with("event: ") {
            let event_type = &line[7..];
            if event_type == "message_start" || event_type == "content_block_delta" {
                return StreamFormat::Anthropic;
            }
        }
        if line.starts_with("data: ") {
            let json_str = &line[6..];
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                if json.get("type").is_some() {
                    // Anthropic events have a "type" field at root level
                    return StreamFormat::Anthropic;
                }
                if json.get("choices").is_some() {
                    return StreamFormat::OpenAI;
                }
            }
        }
    }
    StreamFormat::Unknown
}

/// Parsed SSE event with optional event type
struct SseEvent {
    event_type: Option<String>,
    data: serde_json::Value,
}

fn parse_accumulated_sse(data: &str) -> Option<serde_json::Value> {
    tracing::trace!("Parsing accumulated SSE data ({} bytes)", data.len());

    let format = detect_format(data);
    tracing::trace!("Detected stream format: {:?}", format);

    let events = parse_sse_events(data);
    tracing::trace!("Parsed {} SSE events", events.len());

    match format {
        StreamFormat::Anthropic => {
            let merged = merge_anthropic_events(events);
            tracing::trace!("Merged Anthropic response: {:?}", merged);
            Some(merged)
        }
        StreamFormat::OpenAI => {
            let mut combined: Option<serde_json::Value> = None;
            for event in events {
                combined = Some(merge_chunk(combined, event.data));
            }
            tracing::trace!("Merged OpenAI response: {:?}", combined);
            combined
        }
        StreamFormat::Unknown => {
            tracing::warn!("Unknown SSE format, cannot extract metrics");
            None
        }
    }
}

/// Parse SSE events with their types
fn parse_sse_events(data: &str) -> Vec<SseEvent> {
    let mut events = Vec::new();
    let mut current_event_type: Option<String> = None;

    for line in data.lines() {
        if line.starts_with("event: ") {
            current_event_type = Some(line[7..].to_string());
        } else if line.starts_with("data: ") {
            let json_str = &line[6..];
            if json_str == "[DONE]" {
                continue;
            }
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                events.push(SseEvent {
                    event_type: current_event_type.clone(),
                    data: json,
                });
            }
        }
    }

    events
}

/// Merge Anthropic SSE events into a single response
fn merge_anthropic_events(events: Vec<SseEvent>) -> serde_json::Value {
    use serde_json::json;

    let mut message = json!({"content": []});

    for event in &events {
        let event_type = event.event_type.as_deref().unwrap_or_else(|| {
            // Fall back to data's type field if no event line
            event.data.get("type").and_then(|t| t.as_str()).unwrap_or("")
        });

        match event_type {
            "message_start" => {
                // message_start contains the initial message object
                if let Some(msg) = event.data.get("message") {
                    message = msg.clone();
                    // Ensure content array exists
                    if message.get("content").is_none() {
                        message["content"] = json!([]);
                    }
                }
            }
            "content_block_start" => {
                // Create content block at specified index
                let idx = event.data.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                if let Some(block) = event.data.get("content_block") {
                    if let Some(content) = message.get_mut("content").and_then(|c| c.as_array_mut()) {
                        // Ensure array is large enough
                        while content.len() <= idx {
                            content.push(json!(null));
                        }
                        content[idx] = block.clone();
                    }
                }
            }
            "content_block_delta" => {
                // Append delta text/thinking to content block
                let idx = event.data.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                if let Some(delta) = event.data.get("delta") {
                    if let Some(content) = message.get_mut("content").and_then(|c| c.as_array_mut()) {
                        // Ensure array is large enough and block exists with proper type
                        while content.len() <= idx {
                            content.push(json!(null));
                        }
                        
                        // If block is null, initialize it based on delta type
                        if content[idx].is_null() {
                            let delta_type = delta.get("type").and_then(|t| t.as_str()).unwrap_or("text_delta");
                            let block_type = if delta_type == "thinking_delta" { "thinking" } 
                                           else if delta_type == "input_json_delta" { "tool_use" }
                                           else { "text" };
                            content[idx] = json!({"type": block_type});
                        }
                        
                        let block = &mut content[idx];
                        if let Some(obj) = block.as_object_mut() {
                            // Handle text_delta - use in-place mutation for O(n) performance
                            if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                if let Some(serde_json::Value::String(ref mut existing)) = obj.get_mut("text") {
                                    existing.push_str(text);
                                } else {
                                    obj.insert("text".to_string(), json!(text));
                                }
                            }

                            // Handle thinking_delta (for reasoning models)
                            if let Some(thinking) = delta.get("thinking").and_then(|t| t.as_str()) {
                                if let Some(serde_json::Value::String(ref mut existing)) = obj.get_mut("thinking") {
                                    existing.push_str(thinking);
                                } else {
                                    obj.insert("thinking".to_string(), json!(thinking));
                                }
                            }

                            // Handle partial_json for tool_use input
                            if let Some(partial) = delta.get("partial_json").and_then(|p| p.as_str()) {
                                if let Some(serde_json::Value::String(ref mut existing)) = obj.get_mut("input") {
                                    existing.push_str(partial);
                                } else {
                                    obj.insert("input".to_string(), json!(partial));
                                }
                            }
                        }
                    }
                }
            }
            "message_delta" => {
                // Contains stop_reason and output_tokens usage
                if let Some(delta) = event.data.get("delta") {
                    if let Some(stop) = delta.get("stop_reason").and_then(|s| s.as_str()) {
                        message["stop_reason"] = json!(stop);
                    }
                }
                // usage is at top level of message_delta event
                if let Some(usage) = event.data.get("usage") {
                    if let Some(msg_usage) = message.get_mut("usage") {
                        if let Some(obj) = msg_usage.as_object_mut() {
                            if let Some(output_tokens) = usage.get("output_tokens") {
                                obj.insert("output_tokens".to_string(), output_tokens.clone());
                            }
                        }
                    } else {
                        message["usage"] = usage.clone();
                    }
                }
            }
            "content_block_stop" | "message_stop" | "ping" => {
                // These are signals only, no data to merge
            }
            "error" => {
                // Log error events at warn level
                if let Some(err) = event.data.get("error") {
                    tracing::warn!("Anthropic error event: {:?}", err);
                } else {
                    tracing::warn!("Anthropic error event with no error field: {:?}", event.data);
                }
            }
            _ => {
                tracing::trace!("Unknown Anthropic event type: {}", event_type);
            }
        }
    }

    message
}

/// Merge a streaming chunk into accumulated response
fn merge_chunk(acc: Option<serde_json::Value>, chunk: serde_json::Value) -> serde_json::Value {
    match (acc, chunk) {
        (None, chunk) => chunk,
        (Some(mut acc), chunk) => {
            // Merge choices
            if let Some(acc_choices) = acc.get_mut("choices").and_then(|c| c.as_array_mut()) {
                if let Some(chunk_choices) = chunk.get("choices").and_then(|c| c.as_array()) {
                    for (i, choice) in chunk_choices.iter().enumerate() {
                        if let Some(acc_choice) = acc_choices.get_mut(i) {
                            if let Some(delta) = choice.get("delta") {
                                // Merge delta content
                                if let Some(content) = delta.get("content").and_then(|c| c.as_str())
                                {
                                    if let Some(acc_msg) = acc_choice.get_mut("message") {
                                        if let Some(existing) =
                                            acc_msg.get("content").and_then(|c| c.as_str())
                                        {
                                            acc_msg["content"] =
                                                serde_json::Value::String(format!(
                                                    "{}{}",
                                                    existing, content
                                                ));
                                        } else {
                                            acc_msg["content"] =
                                                serde_json::Value::String(content.to_string());
                                        }
                                    }
                                }

                                // Merge reasoning_text (concatenate like content)
                                if let Some(reasoning) =
                                    delta.get("reasoning_text").and_then(|r| r.as_str())
                                {
                                    if let Some(acc_msg) = acc_choice.get_mut("message") {
                                        if let Some(existing) = acc_msg
                                            .get("reasoning_text")
                                            .and_then(|r| r.as_str())
                                        {
                                            acc_msg["reasoning_text"] =
                                                serde_json::Value::String(format!(
                                                    "{}{}",
                                                    existing, reasoning
                                                ));
                                        } else {
                                            acc_msg["reasoning_text"] =
                                                serde_json::Value::String(reasoning.to_string());
                                        }
                                    }
                                }

                                // Merge reasoning_opaque (replace, not concat - it's a state blob)
                                if let Some(opaque) = delta.get("reasoning_opaque") {
                                    if let Some(acc_msg) = acc_choice.get_mut("message") {
                                        acc_msg["reasoning_opaque"] = opaque.clone();
                                    }
                                }

                                // Merge tool calls
                                if let Some(tool_calls) = delta.get("tool_calls") {
                                    if let Some(acc_tc) = acc_choice
                                        .get_mut("message")
                                        .and_then(|m| m.get_mut("tool_calls"))
                                    {
                                        // Append or merge tool calls
                                        if acc_tc.is_null() {
                                            *acc_tc = tool_calls.clone();
                                        } else if let (Some(acc_arr), Some(new_arr)) =
                                            (acc_tc.as_array_mut(), tool_calls.as_array())
                                        {
                                            for new_call in new_arr {
                                                if let Some(idx) =
                                                    new_call.get("index").and_then(|i| i.as_u64())
                                                {
                                                    // Find or create slot for this index
                                                    while acc_arr.len() <= idx as usize {
                                                        acc_arr.push(serde_json::Value::Null);
                                                    }
                                                    if acc_arr[idx as usize].is_null() {
                                                        acc_arr[idx as usize] = new_call.clone();
                                                    } else {
                                                        // Merge function arguments
                                                        if let (Some(acc_func), Some(new_func)) = (
                                                            acc_arr[idx as usize].get_mut("function"),
                                                            new_call.get("function"),
                                                        ) {
                                                            if let (Some(acc_args), Some(new_args)) = (
                                                                acc_func.get("arguments").and_then(|a| a.as_str()),
                                                                new_func.get("arguments").and_then(|a| a.as_str()),
                                                            ) {
                                                                acc_func["arguments"] = serde_json::Value::String(
                                                                    format!("{}{}", acc_args, new_args),
                                                                );
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Update finish reason
                            if let Some(finish) = choice.get("finish_reason") {
                                if !finish.is_null() {
                                    acc_choice["finish_reason"] = finish.clone();
                                }
                            }
                        }
                    }
                }
            }

            // Merge usage if present
            if let Some(usage) = chunk.get("usage") {
                acc["usage"] = usage.clone();
            }

            // Explicitly preserve model field (llama.cpp includes in first chunk)
            if let Some(model) = chunk.get("model") {
                if !model.is_null() {
                    acc["model"] = model.clone();
                }
            }

            // Preserve timings if present (llama.cpp extension, in final chunk)
            if let Some(timings) = chunk.get("timings") {
                acc["timings"] = timings.clone();
            }

            acc
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_merge_chunk_preserves_model() {
        let chunk1 = json!({"model": "qwen3", "choices": []});
        let chunk2 = json!({"choices": [{"delta": {"content": "hi"}}]});

        let merged = merge_chunk(None, chunk1);
        let merged = merge_chunk(Some(merged), chunk2);

        assert_eq!(merged["model"].as_str().unwrap(), "qwen3");
    }

    #[test]
    fn test_merge_chunk_preserves_timings() {
        let chunk1 = json!({"model": "qwen3", "choices": []});
        let chunk2 = json!({
            "usage": {"prompt_tokens": 10},
            "timings": {"prompt_ms": 50.5}
        });

        let merged = merge_chunk(None, chunk1);
        let merged = merge_chunk(Some(merged), chunk2);

        assert!(merged.get("timings").is_some());
        assert_eq!(merged["timings"]["prompt_ms"].as_f64().unwrap(), 50.5);
    }

    #[test]
    fn test_parse_accumulated_sse_complete() {
        let sse = "data: {\"model\":\"qwen3\",\"choices\":[]}\n\
                   data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\
                   data: [DONE]\n";

        let result = parse_accumulated_sse(sse).unwrap();
        assert_eq!(result["model"].as_str().unwrap(), "qwen3");
        assert_eq!(result["usage"]["prompt_tokens"].as_u64().unwrap(), 10);
    }

    #[test]
    fn test_merge_chunk_accumulates_content() {
        let chunk1 = json!({
            "model": "test-model",
            "choices": [{"index": 0, "delta": {"content": "Hello"}, "message": {"content": "Hello"}}]
        });
        let chunk2 = json!({
            "choices": [{"index": 0, "delta": {"content": " World"}}]
        });

        let merged = merge_chunk(None, chunk1);
        let merged = merge_chunk(Some(merged), chunk2);

        assert_eq!(merged["choices"][0]["message"]["content"].as_str().unwrap(), "Hello World");
    }

    #[test]
    fn test_detect_format_anthropic() {
        let sse = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude\"}}\n";
        assert_eq!(detect_format(sse), StreamFormat::Anthropic);

        let sse2 = "data: {\"type\":\"content_block_delta\",\"index\":0}\n";
        assert_eq!(detect_format(sse2), StreamFormat::Anthropic);
    }

    #[test]
    fn test_detect_format_openai() {
        let sse = "data: {\"model\":\"gpt-4\",\"choices\":[]}\n";
        assert_eq!(detect_format(sse), StreamFormat::OpenAI);
    }

    #[test]
    fn test_parse_anthropic_sse() {
        let sse = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-3\",\"stop_reason\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\nevent: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n";

        let result = parse_accumulated_sse(sse).unwrap();
        
        assert_eq!(result["model"].as_str().unwrap(), "claude-3");
        assert_eq!(result["stop_reason"].as_str().unwrap(), "end_turn");
        assert_eq!(result["usage"]["input_tokens"].as_u64().unwrap(), 10);
        assert_eq!(result["usage"]["output_tokens"].as_u64().unwrap(), 5);
        
        let content = result["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["text"].as_str().unwrap(), "Hello world");
    }

    #[test]
    fn test_merge_anthropic_with_thinking() {
        let sse = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-3\",\"usage\":{\"input_tokens\":100,\"output_tokens\":0}}}\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Let me think\"}}\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Answer\"}}\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":50}}\n";

        let result = parse_accumulated_sse(sse).unwrap();

        assert_eq!(result["model"].as_str().unwrap(), "claude-3");
        assert_eq!(result["usage"]["input_tokens"].as_u64().unwrap(), 100);
        assert_eq!(result["usage"]["output_tokens"].as_u64().unwrap(), 50);

        let content = result["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["thinking"].as_str().unwrap(), "Let me think");
        assert_eq!(content[1]["text"].as_str().unwrap(), "Answer");
    }

    // ============================================================
    // PHASE 1.1: High-Level Integration Tests (Bug Reproduction)
    // ============================================================
    // These tests check the full SSE streaming pipeline with fixes applied
    // to reproduce the duplicate filePath bug at a higher level

    mod integration_tests {
        use crate::fixes::{ToolcallBadFilepathFix, FixRegistry, ToolCallAccumulator, ResponseFix};
        use std::sync::Arc;

        #[test]
        fn test_streaming_toolcall_fix_with_sse_chunks() {
            // Test full SSE stream with tool call fix applied
            // This simulates what happens when llama.cpp streams malformed tool calls

            let fix_registry = {
                let mut registry = FixRegistry::new();
                registry.register(Arc::new(ToolcallBadFilepathFix::new(true)));
                Arc::new(registry)
            };

            let mut accumulator = ToolCallAccumulator::new();

            // SSE chunks as they would come from llama.cpp
            let sse_chunk1 = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"content\\\":\\\"code\\\",\"}}]}}]}\n\n";
            let sse_chunk2 = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\":\\\"/path1\\\",\"}}]}}]}\n\n";
            let sse_chunk3 = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\"/path2\\\"}\"}}]}}]}\n\n";
            let _sse_chunk4 = "data: [DONE]\n\n";

            // Track what the client would accumulate
            let mut client_args_accumulated = String::new();

            // Process chunk 1
            for line in sse_chunk1.lines() {
                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(data) {
                        json = fix_registry.apply_fixes_stream_with_accumulation_default(json, &mut accumulator);

                        if let Some(args) = json["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str() {
                            client_args_accumulated.push_str(args);
                            println!("After chunk 1, client has: {}", client_args_accumulated);
                        }
                    }
                }
            }

            // Process chunk 2
            for line in sse_chunk2.lines() {
                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(data) {
                        json = fix_registry.apply_fixes_stream_with_accumulation_default(json, &mut accumulator);

                        if let Some(args) = json["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str() {
                            client_args_accumulated.push_str(args);
                            println!("After chunk 2, client has: {}", client_args_accumulated);
                        }
                    }
                }
            }

            // Process chunk 3 - THIS IS WHERE THE BUG OCCURS
            for line in sse_chunk3.lines() {
                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(data) {
                        json = fix_registry.apply_fixes_stream_with_accumulation_default(json, &mut accumulator);

                        if let Some(args) = json["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str() {
                            println!("Chunk 3 delta from proxy: {}", args);
                            client_args_accumulated.push_str(args);
                            println!("After chunk 3, client has: {}", client_args_accumulated);
                        }
                    }
                }
            }

            // Verify client's final accumulated arguments are valid JSON
            println!("\nFinal client-accumulated arguments: {}", client_args_accumulated);
            assert!(
                serde_json::from_str::<serde_json::Value>(&client_args_accumulated).is_ok(),
                "BUG REPRODUCED: Client-side SSE accumulation resulted in INVALID JSON: {}",
                client_args_accumulated
            );

            // Verify no duplicate filePath fields in the final result
            let filepath_count = client_args_accumulated.matches(r#""filePath""#).count();
            assert!(
                filepath_count <= 1,
                "BUG: Client accumulated {} filePath fields (should be at most 1): {}",
                filepath_count,
                client_args_accumulated
            );
        }

        #[test]
        fn test_opencode_streaming_pattern() {
            // Test with the exact SSE format that opencode receives
            // This reproduces the user's reported bug

            let fix_registry = {
                let mut registry = FixRegistry::new();
                registry.register(Arc::new(ToolcallBadFilepathFix::new(true)));
                Arc::new(registry)
            };

            let mut accumulator = ToolCallAccumulator::new();

            // Real-world SSE chunks from opencode session (with malformed filePath)
            let chunks = vec![
                "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"content\\\":\\\"#!/usr/bin/perl\\\\n# test\\\\n\\\",\"}}]}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\":\\\"/home/iphands/prog/slop/trash/primes.pl\\\",\"}}]}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\"/home/iphands/prog/slop/llama-proxy/trash/primes.pl\\\"}\"}}]}}]}\n\n",
                "data: [DONE]\n\n",
            ];

            let mut client_args = String::new();

            for (i, sse_chunk) in chunks.iter().enumerate() {
                for line in sse_chunk.lines() {
                    if line.starts_with("data: ") {
                        let data = &line[6..];
                        if data == "[DONE]" {
                            continue;
                        }
                        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(data) {
                            json = fix_registry.apply_fixes_stream_with_accumulation_default(json, &mut accumulator);

                            if let Some(args) = json["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str() {
                                println!("Chunk {} delta: {}", i + 1, args);
                                client_args.push_str(args);
                                println!("Client accumulated: {}", client_args);
                            }
                        }
                    }
                }
            }

            println!("\nFinal opencode client arguments: {}", client_args);

            // Critical test: opencode must receive valid JSON
            let parse_result = serde_json::from_str::<serde_json::Value>(&client_args);
            assert!(
                parse_result.is_ok(),
                "BUG REPRODUCED (opencode pattern): Invalid JSON: {}\nParse error: {:?}",
                client_args,
                parse_result.err()
            );

            // Verify structure
            if let Ok(parsed) = parse_result {
                assert!(parsed.get("content").is_some(), "Missing content field");
                assert!(parsed.get("filePath").is_some(), "Missing filePath field");

                // Should only have ONE filePath field
                let json_str = serde_json::to_string(&parsed).unwrap();
                let filepath_count = json_str.matches(r#""filePath""#).count();
                assert_eq!(filepath_count, 1, "Should have exactly 1 filePath field, got {}", filepath_count);
            }
        }

        #[test]
        fn test_client_server_delta_consistency() {
            // This test verifies that:
            // 1. The proxy's delta calculation matches what the client expects
            // 2. Client-side accumulation == server-side fixed JSON

            let fix = ToolcallBadFilepathFix::new(true);
            let mut accumulator = ToolCallAccumulator::new();

            // Simulate streaming chunks
            let chunk1 = serde_json::json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "function": {
                                "arguments": r#"{"content":"test","#
                            }
                        }]
                    }
                }]
            });

            let chunk2 = serde_json::json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "function": {
                                "arguments": r#""filePath":"/path1","#
                            }
                        }]
                    }
                }]
            });

            let chunk3 = serde_json::json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "function": {
                                // Malformed - duplicate filePath without colon
                                "arguments": r#""filePath"/path2"}"#
                            }
                        }]
                    }
                }]
            });

            let (result1, _) = fix.apply_stream_with_accumulation_default(chunk1, &mut accumulator);
            let (result2, _) = fix.apply_stream_with_accumulation_default(chunk2, &mut accumulator);
            let (result3, _) = fix.apply_stream_with_accumulation_default(chunk3, &mut accumulator);

            let delta1 = result1["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();
            let delta2 = result2["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();
            let delta3 = result3["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();

            // Client-side accumulation
            let client_accumulated = format!("{}{}{}", delta1, delta2, delta3);
            println!("Client accumulated: {}", client_accumulated);

            // The accumulated JSON on client should be valid
            assert!(
                serde_json::from_str::<serde_json::Value>(&client_accumulated).is_ok(),
                "Client-server delta consistency FAILED: {}",
                client_accumulated
            );

            // Additional check: verify the deltas make sense
            // Delta 3 should NOT be a full JSON object (shouldn't start with '{')
            if delta3.starts_with('{') {
                println!("WARNING: Delta 3 is full JSON (probable bug): {}", delta3);
                println!("This will duplicate content client already has!");
            }
        }
    }
}
