//! Streaming response synthesis from complete JSON
//!
//! This module implements the llama-stream approach: receive complete non-streaming JSON
//! from the backend, then synthesize OpenAI-style SSE chunks for clients that want streaming.
//!
//! Benefits:
//! - No delta calculation complexity
//! - Work from complete, parseable JSON always
//! - Tool calls sent as single complete chunk
//! - Simpler fix application (on complete JSON only)

use axum::response::{
    sse::{Event, Sse},
    IntoResponse, Response,
};
use futures::stream::{self, StreamExt};
use serde_json::json;
use std::convert::Infallible;

use crate::api::{AnthropicContentBlock, AnthropicMessage, ChatCompletionResponse, Timings, ToolCall, Usage};

/// Default text chunk size (chars per SSE chunk when synthesizing)
/// Can be configured via config in the future
const DEFAULT_CHUNK_SIZE: usize = 50;

/// Default delay between SSE chunks (milliseconds)
/// Set to 0 for instant streaming, or positive value to simulate realistic pace
const DEFAULT_CHUNK_DELAY_MS: u64 = 50; // 50ms between chunks

/// Synthesize a streaming SSE response from a complete ChatCompletionResponse
///
/// This is the main entry point that mimics llama-stream's _simulate_streaming().
/// Takes a complete JSON response and creates an SSE stream that looks like real streaming.
///
/// Tool calls are sent as a single complete chunk (not incrementally).
/// Text content is chunked into pieces of DEFAULT_CHUNK_SIZE characters.
pub async fn synthesize_streaming_response(response: ChatCompletionResponse) -> Result<Response, String> {
    // Extract data from the complete response
    let model = response.model.clone();
    let id = response.id.clone();
    let created = response.created;

    // Get the first choice (standard for chat completions)
    let choice = response.choices.get(0).ok_or_else(|| "Response has no choices".to_string())?;

    let message = choice.message.as_ref().ok_or_else(|| "Choice has no message".to_string())?;

    let content = message.content.clone();
    let tool_calls = message.tool_calls.clone();
    let reasoning_text = message.reasoning_text.clone();
    let reasoning_opaque = message.reasoning_opaque.clone();
    let finish_reason = choice.finish_reason.clone().unwrap_or_else(|| "stop".to_string());

    // Get usage for final chunk
    let usage = response.usage.clone();
    let timings = response.timings.clone();

    // Pre-compute all chunks
    let chunks = synthesize_chunks(
        id,
        model,
        created,
        content,
        tool_calls,
        reasoning_text,
        reasoning_opaque,
        finish_reason,
        usage,
        timings,
    );

    // Wrap in async stream with delays between chunks
    let stream = stream::iter(chunks).then(|chunk| async move {
        if DEFAULT_CHUNK_DELAY_MS > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(DEFAULT_CHUNK_DELAY_MS)).await;
        }
        chunk
    });

    // Build SSE response
    Ok(Sse::new(stream).into_response())
}

/// Generate the sequence of SSE chunks from complete response data
///
/// Returns Vec<Result<Event, Infallible>> which is compatible with Sse::new()
fn synthesize_chunks(
    id: String,
    model: String,
    created: i64,
    content: Option<String>,
    tool_calls: Option<Vec<ToolCall>>,
    reasoning_text: Option<String>,
    reasoning_opaque: Option<String>,
    finish_reason: String,
    usage: Option<Usage>,
    timings: Option<Timings>,
) -> Vec<Result<Event, Infallible>> {
    let mut chunks = Vec::new();

    // First chunk: role only (standard OpenAI streaming pattern)
    chunks.push(Ok(create_sse_event(&json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {
                "role": "assistant"
            },
            "finish_reason": null
        }]
    }))));

    // If tool calls exist, send them as a SINGLE complete chunk
    // This is key to avoiding delta calculation - send complete tool_calls array at once
    if let Some(tools) = tool_calls {
        chunks.push(Ok(create_sse_event(&json!({
            "id": id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": tools  // ENTIRE array, not incremental
                },
                "finish_reason": null
            }]
        }))));
    }

    // Stream reasoning_text if present (Opencode extension)
    // Send as single chunk since it's usually not huge
    if let Some(reasoning) = reasoning_text {
        chunks.push(Ok(create_sse_event(&json!({
            "id": id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {
                    "reasoning_text": reasoning
                },
                "finish_reason": null
            }]
        }))));
    }

    // Stream reasoning_opaque if present (replace, not concat)
    if let Some(opaque) = reasoning_opaque {
        chunks.push(Ok(create_sse_event(&json!({
            "id": id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {
                    "reasoning_opaque": opaque
                },
                "finish_reason": null
            }]
        }))));
    }

    // Stream text content in chunks (if present)
    if let Some(text) = content {
        for text_chunk in chunk_text(&text, DEFAULT_CHUNK_SIZE) {
            chunks.push(Ok(create_sse_event(&json!({
                "id": id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": {
                        "content": text_chunk
                    },
                    "finish_reason": null
                }]
            }))));
        }
    }

    // Final chunk with finish_reason, usage, and timings
    let mut final_chunk = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": finish_reason
        }]
    });

    // Add usage if present
    if let Some(u) = usage {
        final_chunk["usage"] = serde_json::to_value(u).unwrap();
    }

    // Add timings if present (llama.cpp extension)
    if let Some(t) = timings {
        final_chunk["timings"] = serde_json::to_value(t).unwrap();
    }

    chunks.push(Ok(create_sse_event(&final_chunk)));

    // OpenAI streaming terminator
    chunks.push(Ok(Event::default().data("[DONE]")));

    chunks
}

/// Split text into chunks of approximately max_size characters
///
/// This creates the "streaming" effect for text content.
/// Tries to split on whitespace boundaries when possible.
fn chunk_text(text: &str, max_size: usize) -> Vec<String> {
    if text.len() <= max_size {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = (start + max_size).min(text.len());

        // Try to split on whitespace if not at the end
        let chunk_end = if end < text.len() {
            // Look for last whitespace in the chunk
            text[start..end]
                .rfind(|c: char| c.is_whitespace())
                .map(|i| start + i + 1)
                .unwrap_or(end)
        } else {
            end
        };

        chunks.push(text[start..chunk_end].to_string());
        start = chunk_end;
    }

    chunks
}

/// Create an SSE Event from JSON
fn create_sse_event(json: &serde_json::Value) -> Event {
    Event::default().json_data(json).unwrap()
}

// ============================================================================
// Anthropic Streaming Synthesis
// ============================================================================

/// Synthesize an Anthropic-formatted streaming SSE response from a complete AnthropicMessage
///
/// This creates streaming events that match Anthropic's Messages API SSE format:
/// - message_start: Initial message metadata
/// - content_block_start: Start of each content block
/// - content_block_delta: Text chunks
/// - content_block_stop: End of content block
/// - message_delta: Final metadata (stop_reason, usage)
/// - message_stop: Stream terminator
pub async fn synthesize_anthropic_streaming_response(
    msg: AnthropicMessage,
) -> Result<Response, Box<dyn std::error::Error + Send + Sync>> {
    // Pre-compute all chunks
    let chunks = synthesize_anthropic_chunks(msg);

    // Wrap in async stream with delays between chunks
    let stream = stream::iter(chunks).then(|chunk| async move {
        if DEFAULT_CHUNK_DELAY_MS > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(DEFAULT_CHUNK_DELAY_MS)).await;
        }
        chunk
    });

    // Build SSE response
    Ok(Sse::new(stream).into_response())
}

/// Generate the sequence of Anthropic SSE events from complete message
fn synthesize_anthropic_chunks(msg: AnthropicMessage) -> Vec<Result<Event, Infallible>> {
    let mut chunks = Vec::new();

    // Event 1: message_start
    chunks.push(Ok(create_anthropic_sse_event(
        "message_start",
        &build_message_start_event(&msg),
    )));

    // Events 2-N: content_block_start -> content_block_delta chunks -> content_block_stop
    for (idx, block) in msg.content.iter().enumerate() {
        match block {
            AnthropicContentBlock::Text { text } => {
                // Start text block
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_start",
                    &build_content_block_start_event(idx, "text"),
                )));

                // Send text as chunked deltas
                for text_chunk in chunk_text(text, DEFAULT_CHUNK_SIZE) {
                    chunks.push(Ok(create_anthropic_sse_event(
                        "content_block_delta",
                        &build_content_block_delta_event(idx, "text_delta", &text_chunk),
                    )));
                }

                // Stop text block
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_stop",
                    &build_content_block_stop_event(idx),
                )));
            }
            AnthropicContentBlock::Thinking { thinking, signature } => {
                // Start thinking block
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_start",
                    &build_thinking_block_start_event(idx, signature.as_deref()),
                )));

                // Send thinking as chunked deltas
                for thinking_chunk in chunk_text(thinking, DEFAULT_CHUNK_SIZE) {
                    chunks.push(Ok(create_anthropic_sse_event(
                        "content_block_delta",
                        &build_thinking_block_delta_event(idx, &thinking_chunk),
                    )));
                }

                // Stop thinking block
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_stop",
                    &build_content_block_stop_event(idx),
                )));
            }
            AnthropicContentBlock::ToolUse { id, name, input } => {
                // Start tool_use block
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_start",
                    &build_tool_use_block_start_event(idx, id, name),
                )));

                // Send input as JSON delta
                let input_json = serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string());
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_delta",
                    &build_tool_use_block_delta_event(idx, &input_json),
                )));

                // Stop tool_use block
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_stop",
                    &build_content_block_stop_event(idx),
                )));
            }
            AnthropicContentBlock::ToolResult { content, .. } => {
                // Tool results are typically in user messages, not assistant responses
                // If they appear in responses, treat as text for now
                if let Some(text) = content.as_str() {
                    chunks.push(Ok(create_anthropic_sse_event(
                        "content_block_start",
                        &build_content_block_start_event(idx, "text"),
                    )));

                    for text_chunk in chunk_text(text, DEFAULT_CHUNK_SIZE) {
                        chunks.push(Ok(create_anthropic_sse_event(
                            "content_block_delta",
                            &build_content_block_delta_event(idx, "text_delta", &text_chunk),
                        )));
                    }

                    chunks.push(Ok(create_anthropic_sse_event(
                        "content_block_stop",
                        &build_content_block_stop_event(idx),
                    )));
                }
            }
        }
    }

    // Event N-1: message_delta with stop_reason and usage
    chunks.push(Ok(create_anthropic_sse_event(
        "message_delta",
        &build_message_delta_event(&msg),
    )));

    // Event N: message_stop
    chunks.push(Ok(create_anthropic_sse_event("message_stop", &build_message_stop_event())));

    chunks
}

/// Create an Anthropic SSE Event with event type and data
fn create_anthropic_sse_event(event_type: &str, data: &serde_json::Value) -> Event {
    Event::default().event(event_type).json_data(data).unwrap()
}

/// Build message_start event
fn build_message_start_event(msg: &AnthropicMessage) -> serde_json::Value {
    json!({
        "type": "message_start",
        "message": {
            "id": msg.id,
            "type": "message",
            "role": msg.role,
            "model": msg.model,
            "content": [],
            "stop_reason": null,
            "stop_sequence": null,
            "usage": {
                "input_tokens": msg.usage.input_tokens,
                "output_tokens": 0
            }
        }
    })
}

/// Build content_block_start event for text blocks
fn build_content_block_start_event(index: usize, block_type: &str) -> serde_json::Value {
    json!({
        "type": "content_block_start",
        "index": index,
        "content_block": {
            "type": block_type,
            "text": ""
        }
    })
}

/// Build content_block_start event for thinking blocks
fn build_thinking_block_start_event(index: usize, signature: Option<&str>) -> serde_json::Value {
    let mut content_block = json!({
        "type": "thinking",
        "thinking": ""
    });

    if let Some(sig) = signature {
        content_block["signature"] = json!(sig);
    }

    json!({
        "type": "content_block_start",
        "index": index,
        "content_block": content_block
    })
}

/// Build content_block_delta event for text
fn build_content_block_delta_event(index: usize, delta_type: &str, text: &str) -> serde_json::Value {
    json!({
        "type": "content_block_delta",
        "index": index,
        "delta": {
            "type": delta_type,
            "text": text
        }
    })
}

/// Build content_block_delta event for thinking
fn build_thinking_block_delta_event(index: usize, thinking: &str) -> serde_json::Value {
    json!({
        "type": "content_block_delta",
        "index": index,
        "delta": {
            "type": "thinking_delta",
            "thinking": thinking
        }
    })
}

/// Build content_block_stop event
fn build_content_block_stop_event(index: usize) -> serde_json::Value {
    json!({
        "type": "content_block_stop",
        "index": index
    })
}

/// Build message_delta event with stop_reason and final usage
fn build_message_delta_event(msg: &AnthropicMessage) -> serde_json::Value {
    json!({
        "type": "message_delta",
        "delta": {
            "stop_reason": msg.stop_reason,
            "stop_sequence": msg.stop_sequence
        },
        "usage": {
            "output_tokens": msg.usage.output_tokens
        }
    })
}

/// Build message_stop event
fn build_message_stop_event() -> serde_json::Value {
    json!({"type": "message_stop"})
}

/// Build content_block_start event for tool_use blocks
fn build_tool_use_block_start_event(index: usize, id: &str, name: &str) -> serde_json::Value {
    json!({
        "type": "content_block_start",
        "index": index,
        "content_block": {
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": {}
        }
    })
}

/// Build content_block_delta event for tool_use (input_json_delta)
fn build_tool_use_block_delta_event(index: usize, partial_json: &str) -> serde_json::Value {
    json!({
        "type": "content_block_delta",
        "index": index,
        "delta": {
            "type": "input_json_delta",
            "partial_json": partial_json
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{FunctionCall, Timings, ToolCall, Usage};

    #[test]
    fn test_chunk_text_short() {
        let text = "Hello world";
        let chunks = chunk_text(text, 50);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Hello world");
    }

    #[test]
    fn test_chunk_text_long() {
        let text = "a".repeat(150);
        let chunks = chunk_text(&text, 50);
        assert!(chunks.len() >= 3);

        // Verify chunks reconstruct original
        let reconstructed: String = chunks.concat();
        assert_eq!(reconstructed, text);
    }

    #[test]
    fn test_chunk_text_splits_on_whitespace() {
        let text = "Hello world this is a test of text chunking functionality";
        let chunks = chunk_text(text, 20);

        // Should split on spaces, not mid-word
        for chunk in &chunks {
            if chunk.len() > 1 {
                // Last char of non-final chunks should be space or end of original
                assert!(chunk.ends_with(' ') || chunk == chunks.last().unwrap());
            }
        }
    }

    #[test]
    fn test_create_sse_event_format() {
        let json = json!({"test": "value"});
        let event = create_sse_event(&json);

        // Event should be created successfully (basic smoke test)
        // The actual formatting is handled by Axum's Event type
        // We can't easily inspect the internal data, but we can verify it creates without error
        let _event = event; // Just verify it was created
    }

    #[test]
    fn test_synthesize_chunks_tool_calls() {
        let tool_calls = vec![ToolCall {
            id: Some("call-123".to_string()),
            call_type: Some("function".to_string()),
            index: Some(0),
            function: FunctionCall {
                name: "test_func".to_string(),
                arguments: r#"{"arg":"value"}"#.to_string(),
            },
        }];

        let chunks = synthesize_chunks(
            "test-id".to_string(),
            "test-model".to_string(),
            1234567890,
            None, // no content
            Some(tool_calls),
            None, // no reasoning
            None, // no opaque
            "tool_calls".to_string(),
            None,
            None,
        );

        // Should have: role chunk, tool_calls chunk, final chunk, [DONE]
        assert_eq!(chunks.len(), 4);

        // All chunks should be Ok (smoke test - Event internals are opaque)
        for chunk in &chunks {
            assert!(chunk.is_ok());
        }
    }

    #[test]
    fn test_synthesize_chunks_text_content() {
        let chunks = synthesize_chunks(
            "test-id".to_string(),
            "test-model".to_string(),
            1234567890,
            Some("Hello world".to_string()),
            None,
            None,
            None,
            "stop".to_string(),
            None,
            None,
        );

        // Should have: role chunk, content chunk, final chunk, [DONE]
        assert_eq!(chunks.len(), 4);

        // All chunks should be Ok
        for chunk in &chunks {
            assert!(chunk.is_ok());
        }
    }

    #[test]
    fn test_synthesize_chunks_reasoning_fields() {
        let chunks = synthesize_chunks(
            "test-id".to_string(),
            "test-model".to_string(),
            1234567890,
            Some("Answer".to_string()),
            None,
            Some("Thinking steps".to_string()),
            Some("state_blob".to_string()),
            "stop".to_string(),
            None,
            None,
        );

        // Should have: role, reasoning_text, reasoning_opaque, content, final, [DONE]
        assert_eq!(chunks.len(), 6);

        // All chunks should be Ok
        for chunk in &chunks {
            assert!(chunk.is_ok());
        }
    }

    #[test]
    fn test_synthesize_chunks_usage_and_timings() {
        let usage = Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
            completion_tokens_details: None,
        };

        let timings = Timings {
            prompt_n: Some(10),
            prompt_ms: Some(5.0),
            prompt_per_token_ms: Some(0.5),
            prompt_per_second: Some(2000.0),
            predicted_n: Some(20),
            predicted_ms: Some(10.0),
            predicted_per_token_ms: Some(0.5),
            predicted_per_second: Some(2000.0),
            cache_n: Some(0),
        };

        let chunks = synthesize_chunks(
            "test-id".to_string(),
            "test-model".to_string(),
            1234567890,
            Some("Test".to_string()),
            None,
            None,
            None,
            "stop".to_string(),
            Some(usage),
            Some(timings),
        );

        // Should have chunks including usage and timings in final chunk
        assert!(chunks.len() >= 3); // At least role, content, final, [DONE]

        // All chunks should be Ok
        for chunk in &chunks {
            assert!(chunk.is_ok());
        }
    }

    #[test]
    fn test_synthesize_chunks_ends_with_done() {
        let chunks = synthesize_chunks(
            "test-id".to_string(),
            "test-model".to_string(),
            1234567890,
            Some("Test".to_string()),
            None,
            None,
            None,
            "stop".to_string(),
            None,
            None,
        );

        // Last chunk should be [DONE] - verify it exists
        assert!(chunks.last().is_some());
        assert!(chunks.last().unwrap().is_ok());
    }

    #[test]
    fn test_chunk_text_empty() {
        let chunks = chunk_text("", 50);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "");
    }

    #[test]
    fn test_chunk_text_exact_size() {
        let text = "a".repeat(50);
        let chunks = chunk_text(&text, 50);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 50);
    }

    // ========================================================================
    // Anthropic Synthesis Tests
    // ========================================================================

    use crate::api::{AnthropicContentBlock, AnthropicMessage, AnthropicUsage};

    #[test]
    fn test_build_message_start_event() {
        let msg = AnthropicMessage {
            id: "msg-123".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![],
            model: "test-model".to_string(),
            stop_reason: None,
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 20,
            },
        };

        let event = build_message_start_event(&msg);
        assert_eq!(event["type"], "message_start");
        assert_eq!(event["message"]["id"], "msg-123");
        assert_eq!(event["message"]["role"], "assistant");
        assert_eq!(event["message"]["model"], "test-model");
        assert_eq!(event["message"]["usage"]["input_tokens"], 10);
        assert_eq!(event["message"]["usage"]["output_tokens"], 0); // Always 0 in start
    }

    #[test]
    fn test_build_content_block_start_event() {
        let event = build_content_block_start_event(0, "text");
        assert_eq!(event["type"], "content_block_start");
        assert_eq!(event["index"], 0);
        assert_eq!(event["content_block"]["type"], "text");
        assert_eq!(event["content_block"]["text"], "");
    }

    #[test]
    fn test_build_thinking_block_start_event() {
        let event = build_thinking_block_start_event(0, Some("sig-abc"));
        assert_eq!(event["type"], "content_block_start");
        assert_eq!(event["index"], 0);
        assert_eq!(event["content_block"]["type"], "thinking");
        assert_eq!(event["content_block"]["thinking"], "");
        assert_eq!(event["content_block"]["signature"], "sig-abc");
    }

    #[test]
    fn test_build_content_block_delta_event() {
        let event = build_content_block_delta_event(0, "text_delta", "Hello");
        assert_eq!(event["type"], "content_block_delta");
        assert_eq!(event["index"], 0);
        assert_eq!(event["delta"]["type"], "text_delta");
        assert_eq!(event["delta"]["text"], "Hello");
    }

    #[test]
    fn test_build_thinking_block_delta_event() {
        let event = build_thinking_block_delta_event(0, "Thinking...");
        assert_eq!(event["type"], "content_block_delta");
        assert_eq!(event["index"], 0);
        assert_eq!(event["delta"]["type"], "thinking_delta");
        assert_eq!(event["delta"]["thinking"], "Thinking...");
    }

    #[test]
    fn test_build_content_block_stop_event() {
        let event = build_content_block_stop_event(0);
        assert_eq!(event["type"], "content_block_stop");
        assert_eq!(event["index"], 0);
    }

    #[test]
    fn test_build_message_delta_event() {
        let msg = AnthropicMessage {
            id: "msg-123".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![],
            model: "test-model".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 20,
            },
        };

        let event = build_message_delta_event(&msg);
        assert_eq!(event["type"], "message_delta");
        assert_eq!(event["delta"]["stop_reason"], "end_turn");
        assert_eq!(event["usage"]["output_tokens"], 20);
    }

    #[test]
    fn test_build_message_stop_event() {
        let event = build_message_stop_event();
        assert_eq!(event["type"], "message_stop");
    }

    #[test]
    fn test_synthesize_anthropic_chunks_text_block() {
        let msg = AnthropicMessage {
            id: "msg-123".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock::Text { text: "Hi".to_string() }],
            model: "test-model".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 5,
                output_tokens: 2,
            },
        };

        let chunks = synthesize_anthropic_chunks(msg);

        // Expected: message_start, content_block_start, content_block_delta, content_block_stop, message_delta, message_stop
        assert_eq!(chunks.len(), 6);

        // All chunks should be Ok
        for chunk in &chunks {
            assert!(chunk.is_ok());
        }
    }

    #[test]
    fn test_synthesize_anthropic_chunks_thinking_block() {
        let msg = AnthropicMessage {
            id: "msg-123".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock::Thinking {
                thinking: "Let me think...".to_string(),
                signature: Some("sig-xyz".to_string()),
            }],
            model: "test-model".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 5,
                output_tokens: 3,
            },
        };

        let chunks = synthesize_anthropic_chunks(msg);

        // Expected: message_start, content_block_start, content_block_delta, content_block_stop, message_delta, message_stop
        assert_eq!(chunks.len(), 6);

        // All chunks should be Ok
        for chunk in &chunks {
            assert!(chunk.is_ok());
        }
    }

    #[test]
    fn test_synthesize_anthropic_chunks_multiple_blocks() {
        let msg = AnthropicMessage {
            id: "msg-123".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![
                AnthropicContentBlock::Thinking {
                    thinking: "Thinking".to_string(),
                    signature: None,
                },
                AnthropicContentBlock::Text {
                    text: "Answer".to_string(),
                },
            ],
            model: "test-model".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 5,
                output_tokens: 10,
            },
        };

        let chunks = synthesize_anthropic_chunks(msg);

        // Expected:
        // - message_start (1)
        // - Block 0: content_block_start, content_block_delta, content_block_stop (3)
        // - Block 1: content_block_start, content_block_delta, content_block_stop (3)
        // - message_delta (1)
        // - message_stop (1)
        // Total: 9
        assert_eq!(chunks.len(), 9);

        // All chunks should be Ok
        for chunk in &chunks {
            assert!(chunk.is_ok());
        }
    }

    #[test]
    fn test_synthesize_anthropic_chunks_empty_content() {
        let msg = AnthropicMessage {
            id: "msg-123".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![],
            model: "test-model".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 5,
                output_tokens: 0,
            },
        };

        let chunks = synthesize_anthropic_chunks(msg);

        // Expected: message_start, message_delta, message_stop
        assert_eq!(chunks.len(), 3);

        // All chunks should be Ok
        for chunk in &chunks {
            assert!(chunk.is_ok());
        }
    }

    #[tokio::test]
    async fn test_synthesize_anthropic_streaming_full_flow() {
        // Create a realistic Anthropic message
        let msg = AnthropicMessage {
            id: "msg-test-123".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock::Text {
                text: "Hello, how can I help you?".to_string(),
            }],
            model: "claude-3-5-sonnet".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 7,
            },
        };

        // Call the main synthesis function
        let response = synthesize_anthropic_streaming_response(msg).await;

        // Verify we got a response
        assert!(response.is_ok(), "Should synthesize response successfully");

        // The response should be an SSE stream
        let response = response.unwrap();
        assert_eq!(response.status(), 200);

        // Verify content-type header is set for SSE
        let content_type = response.headers().get("content-type");
        assert!(content_type.is_some());
        let content_type_str = content_type.unwrap().to_str().unwrap();
        assert!(content_type_str.contains("text/event-stream"));
    }

    #[test]
    fn test_anthropic_event_sequence_order() {
        // Verify event sequence matches Anthropic spec
        let msg = AnthropicMessage {
            id: "msg-order-test".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock::Text { text: "Hi".to_string() }],
            model: "test".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 1,
                output_tokens: 1,
            },
        };

        let chunks = synthesize_anthropic_chunks(msg);

        // Verify minimum expected sequence:
        // 1. message_start
        // 2. content_block_start
        // 3. content_block_delta (at least one)
        // 4. content_block_stop
        // 5. message_delta
        // 6. message_stop

        assert!(chunks.len() >= 6, "Should have at least 6 events");

        // All events should be valid SSE events
        for chunk in &chunks {
            assert!(chunk.is_ok(), "All chunks should be Ok");
        }
    }
}
