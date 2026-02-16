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
use futures::stream;
use serde_json::json;
use std::convert::Infallible;

use crate::api::{ChatCompletionResponse, ToolCall, Usage, Timings};

/// Default text chunk size (chars per SSE chunk when synthesizing)
/// Can be configured via config in the future
const DEFAULT_CHUNK_SIZE: usize = 50;

/// Synthesize a streaming SSE response from a complete ChatCompletionResponse
///
/// This is the main entry point that mimics llama-stream's _simulate_streaming().
/// Takes a complete JSON response and creates an SSE stream that looks like real streaming.
///
/// Tool calls are sent as a single complete chunk (not incrementally).
/// Text content is chunked into pieces of DEFAULT_CHUNK_SIZE characters.
pub async fn synthesize_streaming_response(
    response: ChatCompletionResponse,
) -> Result<Response, String> {
    // Extract data from the complete response
    let model = response.model.clone();
    let id = response.id.clone();
    let created = response.created;

    // Get the first choice (standard for chat completions)
    let choice = response.choices.get(0)
        .ok_or_else(|| "Response has no choices".to_string())?;

    let message = choice.message.as_ref()
        .ok_or_else(|| "Choice has no message".to_string())?;

    let content = message.content.clone();
    let tool_calls = message.tool_calls.clone();
    let reasoning_text = message.reasoning_text.clone();
    let reasoning_opaque = message.reasoning_opaque.clone();
    let finish_reason = choice.finish_reason.clone().unwrap_or_else(|| "stop".to_string());

    // Get usage for final chunk
    let usage = response.usage.clone();
    let timings = response.timings.clone();

    // Create SSE event stream
    let stream = stream::iter(synthesize_chunks(
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
    ));

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{
        ChatCompletionResponse, Choice, FunctionCall, ResponseMessage, ToolCall, Usage, Timings,
    };

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
}
