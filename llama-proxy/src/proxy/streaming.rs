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
use crate::fixes::FixRegistry;
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

    // Create oneshot channel for stream completion signaling
    let (completion_tx, completion_rx) = tokio::sync::oneshot::channel::<()>();
    let completion_tx = Arc::new(tokio::sync::Mutex::new(Some(completion_tx)));
    let completion_tx_clone = completion_tx.clone();

    let processed_stream = stream.then(move |chunk_result| {
        let fix_registry = fix_registry.clone();
        let accumulated = accumulated_clone.clone();
        let stats_enabled = stats_enabled;
        let completion_tx = completion_tx_clone.clone();

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

            // Process each SSE event, preserving the exact format including blank lines
            let mut output = String::new();

            // Split by newlines but preserve empty strings (blank lines)
            let lines: Vec<&str> = chunk_str.split('\n').collect();

            for (i, line) in lines.iter().enumerate() {
                if line.starts_with("data: ") {
                    let data = &line[6..];

                    if data == "[DONE]" {
                        // Signal completion to metrics task
                        if stats_enabled {
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

                    // Try to parse as JSON and apply fixes
                    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(data) {
                        json = fix_registry.apply_fixes_stream(json);

                        // Accumulate for stats
                        if stats_enabled {
                            let mut acc = accumulated.lock().await;
                            acc.push_str("data: ");
                            acc.push_str(&serde_json::to_string(&json).unwrap_or_default());
                            acc.push('\n');
                        }

                        output.push_str("data: ");
                        output.push_str(&serde_json::to_string(&json).unwrap_or_default());
                    } else {
                        output.push_str(line);
                    }
                } else {
                    // Preserve empty lines and other SSE format lines (event:, id:, etc.)
                    output.push_str(line);
                }

                // Add newline unless this is the last item and it's empty
                if i < lines.len() - 1 {
                    output.push('\n');
                }
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
            // Wait for stream completion signal with timeout fallback
            let wait_result = tokio::time::timeout(
                tokio::time::Duration::from_secs(30),
                completion_rx
            ).await;

            match wait_result {
                Ok(Ok(())) => {
                    tracing::debug!("Stream completion signal received");
                },
                Ok(Err(_)) => {
                    tracing::warn!("Stream completion channel closed unexpectedly");
                },
                Err(_) => {
                    tracing::warn!("Stream completion timeout after 30s, extracting metrics anyway");
                },
            }

            let acc = accumulated.lock().await;
            if !acc.is_empty() {
                // Parse final SSE event
                if let Some(final_event) = parse_accumulated_sse(&acc) {
                    // Extract metrics from final event
                    let mut metrics = if let Some(ref req_json) = request_json {
                        RequestMetrics::from_response(
                            &final_event,
                            req_json,
                            true, // streaming
                            start.elapsed().as_millis() as f64,
                        )
                    } else {
                        tracing::debug!(
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
                    tracing::info!("\n{}", formatted);

                    // Export to remote systems
                    exporter_manager.export_all(&metrics).await;
                } else {
                    tracing::debug!(
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

/// Parse accumulated SSE data into a complete response for stats
fn parse_accumulated_sse(data: &str) -> Option<serde_json::Value> {
    let mut combined: Option<serde_json::Value> = None;

    for line in data.lines() {
        if line.starts_with("data: ") {
            let json_str = &line[6..];
            if json_str == "[DONE]" {
                continue;
            }

            if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(json_str) {
                combined = Some(merge_chunk(combined, chunk));
            }
        }
    }

    combined
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
}
