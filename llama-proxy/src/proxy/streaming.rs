//! Streaming response handling (SSE)

use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Instant;

use crate::exporters::ExporterManager;
use crate::fixes::FixRegistry;

/// Handle a streaming (SSE) response from the backend
#[allow(clippy::too_many_arguments)]
pub async fn handle_streaming_response(
    backend_response: reqwest::Response,
    fix_registry: Arc<FixRegistry>,
    stats_enabled: bool,
    exporter_manager: Arc<ExporterManager>,
    request_json: Option<serde_json::Value>,
    start: Instant,
) -> Response {
    let status = backend_response.status();
    let headers = backend_response.headers().clone();

    // Create a stream that processes each SSE event
    let stream = backend_response.bytes_stream();

    // Accumulate response for stats
    let accumulated = Arc::new(tokio::sync::Mutex::new(String::new()));
    let accumulated_clone = accumulated.clone();

    let processed_stream = stream.then(move |chunk_result| {
        let fix_registry = fix_registry.clone();
        let accumulated = accumulated_clone.clone();
        let stats_enabled = stats_enabled;

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

            // Process each SSE event
            let mut output = Vec::new();

            for line in chunk_str.lines() {
                if line.starts_with("data: ") {
                    let data = &line[6..];

                    if data == "[DONE]" {
                        output.push(line.to_string());
                        continue;
                    }

                    // Try to parse as JSON and apply fixes
                    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(data) {
                        json = fix_registry.apply_fixes_stream(json);

                        // Accumulate for stats
                        if stats_enabled {
                            let mut acc = accumulated.lock().await;
                            acc.push_str(&serde_json::to_string(&json).unwrap_or_default());
                            acc.push('\n');
                        }

                        output.push(format!(
                            "data: {}",
                            serde_json::to_string(&json).unwrap_or_default()
                        ));
                    } else {
                        output.push(line.to_string());
                    }
                } else {
                    output.push(line.to_string());
                }
            }

            Ok(Bytes::from(output.join("\n")))
        }
    });

    // Spawn task to collect stats after stream completes
    if stats_enabled {
        tokio::spawn(async move {
            // Wait a bit for stream to complete
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            let acc = accumulated.lock().await;
            if !acc.is_empty() {
                // Try to reconstruct final response for stats
                // This is a best-effort approach for streaming
                tracing::debug!(
                    duration_ms = start.elapsed().as_millis() as u64,
                    request_json = ?request_json,
                    "Streaming request completed"
                );

                // Export to remote systems if enabled
                let _ = exporter_manager; // Suppress unused warning
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
#[allow(dead_code)]
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

            acc
        }
    }
}
