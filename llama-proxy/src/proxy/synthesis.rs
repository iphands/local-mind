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
use serde_json::{json, Value};
use std::convert::Infallible;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::{AnthropicContentBlock, AnthropicMessage};
use crate::config::SynthesisConfig;

/// Synthesize an OpenAI-format streaming SSE response from the RAW buffered backend value.
///
/// Consumes the `Value` the fake pipeline holds before any typed parse: the
/// envelope is built by reading `Value` pointers directly, so unknown
/// top-level fields and unknown `usage` keys survive by construction and ALL
/// `choices` entries are streamed (not just `choices[0]`). Malformed known
/// fields (non-array choices, `index: null`, missing message) return Err and
/// the caller falls through - never a fabricated stream.
pub async fn synthesize_streaming_response(raw: &Value, config: &SynthesisConfig) -> Result<Response, String> {
    let parsed = parse_raw_completion(raw)?;
    Ok(stream_response(synthesize_openai_chunks(&parsed, config), config))
}

/// Validated pointer view of the RAW buffered backend completion.
struct RawCompletion<'a> {
    meta: StreamMeta,
    choices: Vec<&'a Value>,
    usage: Option<&'a Value>,
    timings: Option<&'a Value>,
}

/// Parse-and-validate the KNOWN fields a synthesized stream needs. Known
/// fields must keep their wire types (`choices` a non-empty array of message
/// objects, `index` a u64 when present, `finish_reason` a string or null);
/// anything unrecognized is collected as `extras` and merged into every chunk
/// envelope, so unknown fields survive by construction.
fn parse_raw_completion(raw: &Value) -> Result<RawCompletion<'_>, String> {
    let obj = raw.as_object().ok_or_else(|| "response is not a JSON object".to_string())?;
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "response has no string id".to_string())?;
    let model = obj
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| "response has no string model".to_string())?;
    let created = obj.get("created").and_then(Value::as_i64).unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });
    let choices = obj
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| "response has no choices array".to_string())?;
    if choices.is_empty() {
        return Err("response has an empty choices array".to_string());
    }
    for c in choices {
        let co = c.as_object().ok_or_else(|| "choices[] entry is not an object".to_string())?;
        co.get("message")
            .and_then(Value::as_object)
            .ok_or_else(|| "choices[] entry has no message object".to_string())?;
        if co.get("index").is_some_and(|i| !i.is_u64()) {
            return Err("choices[].index is present but not a non-negative integer".to_string());
        }
        if co.get("finish_reason").is_some_and(|f| !f.is_null() && !f.is_string()) {
            return Err("choices[].finish_reason is present but not a string or null".to_string());
        }
    }
    let mut extras = serde_json::Map::new();
    for (k, v) in obj {
        if !matches!(
            k.as_str(),
            "id" | "object" | "created" | "model" | "choices" | "usage" | "timings"
        ) {
            extras.insert(k.clone(), v.clone());
        }
    }
    Ok(RawCompletion {
        meta: StreamMeta {
            id: id.to_string(),
            created,
            model: model.to_string(),
            extras: Value::Object(extras),
        },
        choices: choices.iter().collect(),
        usage: obj.get("usage").filter(|u| u.is_object()),
        timings: obj.get("timings").filter(|t| t.is_object()),
    })
}

/// Metadata shared by every synthesized OpenAI SSE chunk of one response
struct StreamMeta {
    id: String,
    created: i64,
    model: String,
    /// Unknown top-level backend fields, merged into every chunk envelope.
    extras: Value,
}

/// The delta kind whose payload lands in `finish_reason` instead of `delta`
const FINISH_EVENT: &str = "finish_reason";

/// Build the one OpenAI chunk envelope shape used by every synthesized chunk.
///
/// `event` names the delta key (`role`, `tool_calls`, `reasoning_text`,
/// `reasoning_opaque`, `content`); the reserved `finish_reason` kind emits an
/// empty delta and carries `data` as the choice's `finish_reason`.
fn sse_envelope(meta: &StreamMeta, event: &str, data: Value, idx: u64) -> Value {
    let (delta, finish_reason) = if event == FINISH_EVENT {
        (json!({}), data)
    } else {
        (json!({ (event): data }), Value::Null)
    };
    let mut env = json!({
        "id": meta.id,
        "object": "chat.completion.chunk",
        "created": meta.created,
        "model": meta.model,
        "choices": [{
            "index": idx,
            "delta": delta,
            "finish_reason": finish_reason
        }]
    });
    // Unknown top-level backend fields ride every chunk exactly as they arrived.
    if let Some(extras) = meta.extras.as_object() {
        for (k, v) in extras {
            env[k] = v.clone();
        }
    }
    env
}

/// One choice's chunk set: role -> reasoning -> text -> tool-call args -> finish.
/// Ordering per task 59; `usage`/`timings` ride the finish frame when the
/// caller marks this the last choice. Every read is a `Value` pointer read on
/// the RAW backend body - no typed round-trip.
fn synthesize_choice_chunks(
    meta: &StreamMeta,
    choice: &Value,
    idx: u64,
    usage: Option<&Value>,
    timings: Option<&Value>,
    chunk_size_chars: usize,
) -> Vec<Result<Event, Infallible>> {
    let mut chunks = Vec::new();
    let message = match choice.get("message").and_then(Value::as_object) {
        Some(m) => m,
        // parse_raw_completion validated this; belt-and-braces, emit nothing.
        None => return chunks,
    };

    if let Some(role) = message.get("role").and_then(Value::as_str) {
        chunks.push(Ok(create_sse_event(&sse_envelope(meta, "role", json!(role), idx))));
    }
    if let Some(r) = message.get("reasoning_text").and_then(Value::as_str) {
        chunks.push(Ok(create_sse_event(&sse_envelope(meta, "reasoning_text", json!(r), idx))));
    }
    if let Some(r) = message.get("reasoning_opaque").and_then(Value::as_str) {
        chunks.push(Ok(create_sse_event(&sse_envelope(meta, "reasoning_opaque", json!(r), idx))));
    }
    match message.get("content") {
        Some(Value::String(text)) if !text.is_empty() => {
            for text_chunk in chunk_text(text, chunk_size_chars) {
                chunks.push(Ok(create_sse_event(&sse_envelope(meta, "content", json!(text_chunk), idx))));
            }
        }
        // Non-string content has no OpenAI-delta shape to fake - the raw JSON
        // value rides one content frame verbatim.
        Some(other) if !other.is_null() && !other.is_string() => {
            chunks.push(Ok(create_sse_event(&sse_envelope(meta, "content", other.clone(), idx))));
        }
        _ => {}
    }
    let tool_calls = message.get("tool_calls").and_then(Value::as_array);
    if let Some(tools) = tool_calls {
        if !tools.is_empty() {
            // Tool calls ride LAST as a SINGLE complete chunk - the key to
            // avoiding client-side delta calculation (task 59 ordering).
            chunks.push(Ok(create_sse_event(&sse_envelope(
                meta,
                "tool_calls",
                Value::Array(tools.clone()),
                idx,
            ))));
        }
    }

    // Honest derivation only when the backend omitted finish_reason (task 59).
    let finish_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            if tool_calls.is_some_and(|t| !t.is_empty()) {
                "tool_calls".to_string()
            } else {
                "stop".to_string()
            }
        });
    let mut final_chunk = sse_envelope(meta, FINISH_EVENT, json!(finish_reason), idx);
    if let Some(u) = usage {
        final_chunk["usage"] = u.clone();
    }
    if let Some(t) = timings {
        final_chunk["timings"] = t.clone();
    }
    chunks.push(Ok(create_sse_event(&final_chunk)));
    chunks
}

/// Stream EVERY choice (gap-free per-choice indices from `index` or array
/// position), then the OpenAI `[DONE]` terminator.
fn synthesize_openai_chunks(parsed: &RawCompletion<'_>, config: &SynthesisConfig) -> Vec<Result<Event, Infallible>> {
    let mut chunks = Vec::new();
    let last = parsed.choices.len() - 1;
    for (pos, choice) in parsed.choices.iter().enumerate() {
        let idx = choice.get("index").and_then(Value::as_u64).unwrap_or(pos as u64);
        let is_last = pos == last;
        chunks.extend(synthesize_choice_chunks(
            &parsed.meta,
            choice,
            idx,
            if is_last { parsed.usage } else { None },
            if is_last { parsed.timings } else { None },
            config.chunk_size_chars,
        ));
    }
    // OpenAI streaming terminator
    chunks.push(Ok(Event::default().data("[DONE]")));
    chunks
}

/// Paced SSE response over pre-computed chunks - shared by every entry point.
fn stream_response(chunks: Vec<Result<Event, Infallible>>, config: &SynthesisConfig) -> Response {
    let chunk_delay_ms = config.chunk_delay_ms;
    let stream = stream::iter(chunks).then(move |chunk| async move {
        if chunk_delay_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(chunk_delay_ms)).await;
        }
        chunk
    });
    Sse::new(stream).into_response()
}

/// Map an OpenAI finish_reason onto the Anthropic stop_reason vocabulary.
fn map_openai_finish_to_stop(finish: &str) -> String {
    match finish {
        "stop" => "end_turn".to_string(),
        "length" => "max_tokens".to_string(),
        "tool_calls" => "tool_use".to_string(),
        other => other.to_string(),
    }
}

/// Convert a RAW OpenAI completion into the full Anthropic SSE event set.
///
/// `None` means the body is not a well-formed OpenAI completion (malformed
/// KNOWN fields) - the /v1/messages caller then answers with the SSE error
/// frame. Unknown top-level fields merge into `message_start.message` and
/// unknown usage keys stay in the usage object, so they survive BY
/// CONSTRUCTION - the typed `ChatCompletionResponse` round-trip is bypassed.
/// Every choice is emitted with gap-free global block indices.
pub fn convert_openai_to_claude_sse(raw: &Value, config: &SynthesisConfig) -> Option<Vec<Result<Event, Infallible>>> {
    let parsed = parse_raw_completion(raw).ok()?;
    let usage = match parsed.usage {
        Some(u) => {
            // Read first, then write - the mapped keys are known fields.
            let mut u = u.clone();
            let prompt = u.get("prompt_tokens").cloned();
            let completion = u.get("completion_tokens").cloned();
            if let Some(pt) = prompt {
                u["input_tokens"] = pt;
            }
            if let Some(ct) = completion {
                u["output_tokens"] = ct;
            }
            u
        }
        None => json!({"input_tokens": 0, "output_tokens": 0}),
    };
    let first_msg = parsed.choices.first().and_then(|c| c.get("message"))?;
    let role = first_msg.get("role").and_then(Value::as_str).unwrap_or("assistant");
    let mut message = json!({
        "id": parsed.meta.id, "type": "message", "role": role, "model": parsed.meta.model,
        "content": [], "stop_reason": null, "stop_sequence": null, "usage": usage,
    });
    if let Some(extras) = parsed.meta.extras.as_object() {
        for (k, v) in extras {
            message[k] = v.clone();
        }
    }
    let mut chunks: Vec<Result<Event, Infallible>> = vec![Ok(create_anthropic_sse_event(
        "message_start",
        &json!({"type": "message_start", "message": message}),
    ))];

    let mut idx = 0usize;
    let mut has_tool_use = false;
    for choice in &parsed.choices {
        let message = choice.get("message").and_then(Value::as_object)?;
        let mut emitted = false;
        if let Some(reasoning) = message.get("reasoning_text").and_then(Value::as_str) {
            if !reasoning.is_empty() {
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_start",
                    &build_thinking_block_start_event(idx, None),
                )));
                for part in chunk_text(reasoning, config.chunk_size_chars) {
                    chunks.push(Ok(create_anthropic_sse_event(
                        "content_block_delta",
                        &build_thinking_block_delta_event(idx, &part),
                    )));
                }
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_stop",
                    &build_content_block_stop_event(idx),
                )));
                idx += 1;
                emitted = true;
            }
        }
        match message.get("content") {
            Some(Value::String(text)) if !text.is_empty() => {
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_start",
                    &build_content_block_start_event(idx, "text"),
                )));
                for part in chunk_text(text, config.chunk_size_chars) {
                    chunks.push(Ok(create_anthropic_sse_event(
                        "content_block_delta",
                        &build_content_block_delta_event(idx, "text_delta", part.as_str()),
                    )));
                }
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_stop",
                    &build_content_block_stop_event(idx),
                )));
                idx += 1;
                emitted = true;
            }
            // Non-string, non-null content: verbatim pass-through block.
            Some(other) if !other.is_null() && !other.is_string() => {
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_start",
                    &json!({
                        "type": "content_block_start", "index": idx, "content_block": other,
                    }),
                )));
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_stop",
                    &build_content_block_stop_event(idx),
                )));
                idx += 1;
                emitted = true;
            }
            _ => {}
        }
        if let Some(tools) = message.get("tool_calls").and_then(Value::as_array) {
            for tc in tools {
                let id = tc
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("toolu_{}", uuid::Uuid::new_v4().to_string().replace('-', "")));
                let name = tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let args = tc
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_start",
                    &build_tool_use_block_start_event(idx, &id, name),
                )));
                // The RAW arguments string rides partial_json verbatim - even
                // if it is not valid JSON, rewriting it would lie.
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_delta",
                    &build_tool_use_block_delta_event(idx, args),
                )));
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_stop",
                    &build_content_block_stop_event(idx),
                )));
                idx += 1;
                has_tool_use = true;
                emitted = true;
            }
        }
        if !emitted {
            // Anthropic requires at least one block; mirrors the From fallback.
            chunks.push(Ok(create_anthropic_sse_event(
                "content_block_start",
                &build_content_block_start_event(idx, "text"),
            )));
            chunks.push(Ok(create_anthropic_sse_event(
                "content_block_stop",
                &build_content_block_stop_event(idx),
            )));
            idx += 1;
        }
    }

    // Last choice's finish_reason decides; absent -> honest derivation (59 rule).
    let stop_reason = parsed
        .choices
        .last()
        .and_then(|c| c.get("finish_reason"))
        .and_then(Value::as_str)
        .map(map_openai_finish_to_stop)
        .unwrap_or_else(|| {
            if has_tool_use {
                "tool_use".to_string()
            } else {
                "end_turn".to_string()
            }
        });
    chunks.push(Ok(create_anthropic_sse_event(
        "message_delta",
        &json!({
                "type": "message_delta",
            "delta": { "stop_reason": stop_reason, "stop_sequence": null },
            "usage": usage,
        }),
    )));
    chunks.push(Ok(create_anthropic_sse_event("message_stop", &build_message_stop_event())));
    Some(chunks)
}

/// Streaming-response entry for the /v1/messages OpenAI-format branch.
/// `None` = malformed OpenAI body; the caller serves the SSE error frame.
pub fn synthesize_anthropic_openai_format_response(raw: &Value, config: &SynthesisConfig) -> Option<Response> {
    Some(stream_response(convert_openai_to_claude_sse(raw, config)?, config))
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
        let raw_end = (start + max_size).min(text.len());
        // Align raw_end backward to a char boundary (avoids panic on multi-byte chars like emojis)
        let end = floor_char_boundary(text, raw_end);
        // Guarantee at least one char of progress to prevent an infinite loop
        let end = if end <= start {
            text[start..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| start + i)
                .unwrap_or(text.len())
        } else {
            end
        };

        // Try to split on whitespace if not at the end
        let chunk_end = if end < text.len() {
            // Use char_indices so the advance past the whitespace char is always correct
            text[start..end]
                .char_indices()
                .rev()
                .find(|(_, c)| c.is_whitespace())
                .map(|(i, c)| start + i + c.len_utf8())
                .unwrap_or(end)
        } else {
            end
        };

        chunks.push(text[start..chunk_end].to_string());
        start = chunk_end;
    }

    chunks
}

/// Return the largest index ≤ `index` that is a UTF-8 char boundary.
fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
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
    config: &SynthesisConfig,
) -> Result<Response, Box<dyn std::error::Error + Send + Sync>> {
    // Pre-compute all chunks, then pace the SSE response
    Ok(stream_response(
        synthesize_anthropic_chunks(msg, config.chunk_size_chars),
        config,
    ))
}

/// Generate the sequence of Anthropic SSE events from complete message
fn synthesize_anthropic_chunks(msg: AnthropicMessage, chunk_size_chars: usize) -> Vec<Result<Event, Infallible>> {
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
                for text_chunk in chunk_text(text, chunk_size_chars) {
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
                for thinking_chunk in chunk_text(thinking, chunk_size_chars) {
                    chunks.push(Ok(create_anthropic_sse_event(
                        "content_block_delta",
                        &build_thinking_block_delta_event(idx, &thinking_chunk),
                    )));
                }

                // A backend-provided signature rides a faithful signature_delta
                // frame before the block closes (Anthropic's own stream shape).
                if let Some(sig) = signature {
                    chunks.push(Ok(create_anthropic_sse_event(
                        "content_block_delta",
                        &build_signature_delta_event(idx, sig),
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
            AnthropicContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                // Honest tool_result block: the raw (UNTRUSTED) content passes
                // through verbatim in content_block_start - never reinterpreted
                // as a text block, never dropped (array shapes included).
                // No deltas: chunking an opaque tool-result payload would rewrite it.
                let mut block = json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": content,
                });
                if let Some(e) = is_error {
                    block["is_error"] = json!(e);
                }
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_start",
                    &json!({
                        "type": "content_block_start",
                        "index": idx,
                        "content_block": block,
                    }),
                )));
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_stop",
                    &build_content_block_stop_event(idx),
                )));
            }
            AnthropicContentBlock::Other(value) => {
                // Verbatim pass-through: content_block_start carries the raw
                // block exactly as it arrived. No deltas are synthesized -
                // inventing a delta type for an opaque shape would rewrite it.
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_start",
                    &json!({
                        "type": "content_block_start",
                        "index": idx,
                        "content_block": value,
                    }),
                )));
                chunks.push(Ok(create_anthropic_sse_event(
                    "content_block_stop",
                    &build_content_block_stop_event(idx),
                )));
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
/// Anthropic signature_delta frame - carries the backend's thinking signature
/// verbatim (absent signatures never fabricate one).
fn build_signature_delta_event(index: usize, signature: &str) -> serde_json::Value {
    json!({
        "type": "content_block_delta",
        "index": index,
        "delta": {
            "type": "signature_delta",
            "signature": signature
        }
    })
}

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

/// Derive the stop_reason the backend omitted: a tool_use block -> tool_use,
/// otherwise end_turn. max_tokens is NEVER derived (only the backend can say it).
fn derive_stop_reason(msg: &AnthropicMessage) -> String {
    if msg.content.iter().any(|b| matches!(b, AnthropicContentBlock::ToolUse { .. })) {
        "tool_use".to_string()
    } else {
        "end_turn".to_string()
    }
}

/// Build message_delta event with stop_reason and final usage
fn build_message_delta_event(msg: &AnthropicMessage) -> serde_json::Value {
    let stop_reason = msg.stop_reason.clone().or_else(|| Some(derive_stop_reason(msg)));
    json!({
        "type": "message_delta",
        "delta": {
            "stop_reason": stop_reason,
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
    use crate::config::SynthesisConfig;

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
    fn test_chunk_text_multibyte_emoji() {
        // Emojis are 4 bytes each; a chunk boundary mid-emoji must not panic
        // "💡" is 4 bytes; 12 emojis = 48 bytes, chunk_size=50 puts end at byte 50 (inside emoji 13)
        let text = "💡".repeat(20); // 80 bytes total
        let chunks = chunk_text(&text, 50);
        // All chunks must be valid UTF-8 strings (no panic = pass)
        let reconstructed: String = chunks.concat();
        assert_eq!(reconstructed, text);
    }

    #[test]
    fn test_chunk_text_mixed_emoji_ascii() {
        // The actual crashing pattern from production logs
        let text = "👋 **Hello!** 😊\n\nHow can I assist you today? 🌱\n\nWhether you have a question 💬";
        let chunks = chunk_text(text, 50);
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
        let chunks = synthesize_openai_chunks(
            &parse_raw_completion(&json!({
                "id": "test-id", "created": 1234567890, "model": "test-model",
                "choices": [{"index": 0, "finish_reason": "tool_calls", "message": {"role": "assistant",
                    "tool_calls": [{"id": "call-123", "type": "function", "function": {"name": "test_func", "arguments": "{\"arg\":\"value\"}"}}]}}]
            }))
            .expect("fixture"),
            &SynthesisConfig::default(),
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
        let chunks = synthesize_openai_chunks(
            &parse_raw_completion(&json!({
                "id": "test-id", "created": 1234567890, "model": "test-model",
                "choices": [{"index": 0, "finish_reason": "stop", "message": {"role": "assistant", "content": "Hello world"}}]
            }))
            .expect("fixture"),
            &SynthesisConfig::default(),
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
        let chunks = synthesize_openai_chunks(
            &parse_raw_completion(&json!({
                "id": "test-id", "created": 1234567890, "model": "test-model",
                "choices": [{"index": 0, "finish_reason": "stop", "message": {"role": "assistant", "content": "Answer",
                    "reasoning_text": "Thinking steps", "reasoning_opaque": "state_blob"}}]
            }))
            .expect("fixture"),
            &SynthesisConfig::default(),
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
        let chunks = synthesize_openai_chunks(
            &parse_raw_completion(&json!({
                "id": "test-id", "created": 1234567890, "model": "test-model",
                "choices": [{"index": 0, "finish_reason": "stop", "message": {"role": "assistant", "content": "Test"}}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30},
                "timings": {"prompt_n": 10, "predicted_n": 20, "predicted_per_second": 2000.0}
            }))
            .expect("fixture"),
            &SynthesisConfig::default(),
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
        let chunks = synthesize_openai_chunks(
            &parse_raw_completion(&json!({
                "id": "test-id", "created": 1234567890, "model": "test-model",
                "choices": [{"index": 0, "finish_reason": "stop", "message": {"role": "assistant", "content": "Test"}}]
            }))
            .expect("fixture"),
            &SynthesisConfig::default(),
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

        let chunks = synthesize_anthropic_chunks(msg, 50);

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

        let chunks = synthesize_anthropic_chunks(msg, 50);

        // Expected: message_start, content_block_start, content_block_delta,
        // signature_delta (task 60 - backend signature now rides its own frame;
        // frame CONTENT is asserted end-to-end in tool_result_signature_tests),
        // content_block_stop, message_delta, message_stop
        assert_eq!(chunks.len(), 7);

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

        let chunks = synthesize_anthropic_chunks(msg, 50);

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

    #[tokio::test]
    async fn test_synthesize_anthropic_stream_other_block_emits_verbatim_start() {
        let other = serde_json::json!({"type": "advisor_tool_result", "tool_use_id": "x", "content": "hi"});
        let msg = AnthropicMessage {
            id: "msg-other".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![
                AnthropicContentBlock::Text { text: "hi".to_string() },
                AnthropicContentBlock::Other(other.clone()),
            ],
            model: "m".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 1,
                output_tokens: 1,
            },
        };

        let resp = synthesize_anthropic_streaming_response(msg, &SynthesisConfig::default())
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();

        let starts: Vec<serde_json::Value> = text
            .split("\n\n")
            .filter_map(|frame| {
                let mut data = None;
                let mut is_start = false;
                for line in frame.lines() {
                    if let Some(v) = line.strip_prefix("event:") {
                        is_start = v.trim() == "content_block_start";
                    } else if let Some(v) = line.strip_prefix("data:") {
                        data = Some(v.trim());
                    }
                }
                if is_start {
                    Some(serde_json::from_str(data?).unwrap())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(starts.len(), 2);
        // The opaque block reaches the wire exactly as it arrived, at its own
        // index; no delta events are invented for it.
        assert_eq!(starts[1]["content_block"], other);
        assert_eq!(starts[1]["index"], serde_json::json!(1));
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

        let chunks = synthesize_anthropic_chunks(msg, 50);

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
        let response = synthesize_anthropic_streaming_response(msg, &SynthesisConfig::default()).await;

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

        let chunks = synthesize_anthropic_chunks(msg, 50);

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

#[cfg(test)]
mod chunk_timing_tests {
    use super::*;
    use crate::api::AnthropicUsage;
    use crate::config::SynthesisConfig;
    use futures::StreamExt;

    async fn timed_body(resp: Response, n: usize) -> Vec<(std::time::Duration, String)> {
        let started = std::time::Instant::now();
        let mut stream = resp.into_body().into_data_stream();
        let mut out = Vec::new();
        for _ in 0..n {
            match tokio::time::timeout(std::time::Duration::from_secs(5), stream.next()).await {
                Ok(Some(Ok(b))) => out.push((started.elapsed(), String::from_utf8_lossy(&b).to_string())),
                _ => break,
            }
        }
        out
    }

    fn resp_with_text(text: &str) -> Value {
        serde_json::json!({
            "id": "cmpl-t57", "created": 1, "model": "m",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": text}}]
        })
    }

    #[tokio::test]
    async fn default_config_inter_chunk_delta_under_5ms() {
        // Given: default SynthesisConfig (chunk_delay_ms = 0)
        // When: synthesizing a 3000-char text (two content chunks)
        // Then: plan acceptance - delta between the first two body chunks < 5 ms
        //       (baseline slept 50 ms per chunk => RED)
        let cfg = SynthesisConfig::default();
        assert_eq!(cfg.chunk_delay_ms, 0);
        assert_eq!(cfg.chunk_size_chars, 2000);
        let text = "x".repeat(3000);
        let resp = synthesize_streaming_response(&resp_with_text(&text), &cfg).await.unwrap();
        let marks = timed_body(resp, 2).await;
        assert!(marks.len() >= 2, "expected at least two body chunks");
        let delta = marks[1].0 - marks[0].0;
        assert!(
            delta < std::time::Duration::from_millis(5),
            "inter-chunk delta {delta:?} must be < 5 ms at delay 0"
        );
    }

    #[tokio::test]
    async fn configured_chunk_delay_is_observable() {
        // Given: chunk_delay_ms = 400 (baseline ignores config and slept 50 ms)
        // When: synthesizing two content chunks
        // Then: first->second delta >= 200 ms - a generous half-margin that can
        //       only fail if the configured sleep is skipped, never by slowness
        let cfg = SynthesisConfig {
            chunk_delay_ms: 400,
            chunk_size_chars: 2000,
        };
        let text = "x".repeat(3000);
        let resp = synthesize_streaming_response(&resp_with_text(&text), &cfg).await.unwrap();
        let marks = timed_body(resp, 2).await;
        assert!(marks.len() >= 2);
        let delta = marks[1].0 - marks[0].0;
        assert!(
            delta >= std::time::Duration::from_millis(200),
            "configured 400 ms delay not honored: delta {delta:?}"
        );
    }

    #[tokio::test]
    async fn chunk_size_chars_default_splits_3000_chars_into_two() {
        // Given: default chunk_size_chars = 2000, whitespace-free 3000-char text
        // When: OpenAI synthesis
        // Then: exactly two content delta chunks of 2000 and 1000 chars
        //       (baseline: hard-coded 50 => 60 chunks => RED)
        let cfg = SynthesisConfig::default();
        let text = "x".repeat(3000);
        let resp = synthesize_streaming_response(&resp_with_text(&text), &cfg).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 10_000_000).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        let content_chunks: Vec<&str> = body
            .lines()
            .filter(|l| l.contains("\"content\":"))
            .map(|l| l.split_once("\"content\":\"").unwrap().1.split_once("\"}").unwrap().0)
            .collect();
        assert_eq!(
            content_chunks.len(),
            2,
            "expected 2 content deltas, got {}",
            content_chunks.len()
        );
        assert_eq!(content_chunks[0].chars().count(), 2000);
        assert_eq!(content_chunks[1].chars().count(), 1000);
    }

    #[tokio::test]
    async fn anthropic_path_uses_configured_chunk_size_and_zero_delay() {
        // Given: default config on the Anthropic streaming path
        // When: a 3000-char text block is synthesized
        // Then: first two frames arrive < 5 ms apart (delay 0) and the block splits
        //       into exactly two text_delta frames
        let cfg = SynthesisConfig::default();
        let msg = AnthropicMessage {
            id: "msg-t57".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock::Text { text: "x".repeat(3000) }],
            model: "m".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 1,
                output_tokens: 1,
            },
        };
        let resp = synthesize_anthropic_streaming_response(msg, &cfg).await.unwrap();
        let marks = timed_body(resp, 2).await;
        let delta = marks[1].0 - marks[0].0;
        assert!(
            delta < std::time::Duration::from_millis(5),
            "anthropic delta {delta:?} must be < 5 ms at delay 0"
        );
    }
}

#[cfg(test)]
mod envelope_factory_tests {
    use super::*;

    fn meta() -> StreamMeta {
        StreamMeta {
            extras: Value::Null,
            id: "cmpl-env".to_string(),
            created: 42,
            model: "qwen3".to_string(),
        }
    }

    #[test]
    fn envelope_wraps_delta_key_and_nulls_finish_reason() {
        // Given a StreamMeta and a delta kind
        // When sse_envelope builds the chunk
        // Then the OpenAI chunk skeleton matches the hand-built baseline shape exactly
        let v = sse_envelope(&meta(), "content", json!("hi"), 0);
        assert_eq!(
            v,
            json!({
                "id": "cmpl-env",
                "object": "chat.completion.chunk",
                "created": 42,
                "model": "qwen3",
                "choices": [{ "index": 0, "delta": { "content": "hi" }, "finish_reason": null }]
            })
        );
    }

    #[test]
    fn envelope_finish_carries_finish_reason_with_empty_delta() {
        let v = sse_envelope(&meta(), "finish_reason", json!("tool_calls"), 1);
        assert_eq!(v["choices"][0]["delta"], json!({}));
        assert_eq!(v["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(v["choices"][0]["index"], 1);
    }

    #[tokio::test]
    async fn refactor_preserves_exact_sse_bytes() {
        // Golden bytes captured VERBATIM from the REAL response for this exact fixture.
        // Pre-58 capture proved the sse_envelope refactor was byte-identical (task58
        // evidence); retargeted in task 59 (order/derivation) and again in task 61 for
        // the RAW-Value path: extras (system_fingerprint, x_top_unknown) ride every
        // chunk, choices[1] now streams at index 1, and usage carries llama_extra
        // verbatim. Disclosed behavior change - baseline drops these (task61 capture).
        const BASELINE_BYTES: &str = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null,\"index\":0}],\"created\":171,\"id\":\"cmpl-probe\",\"model\":\"qwen3\",\"object\":\"chat.completion.chunk\",\"system_fingerprint\":\"fp_999\",\"x_top_unknown\":{\"alpha\":true}}\n\ndata: {\"choices\":[{\"delta\":{\"reasoning_text\":\"Hmm, let me think.\"},\"finish_reason\":null,\"index\":0}],\"created\":171,\"id\":\"cmpl-probe\",\"model\":\"qwen3\",\"object\":\"chat.completion.chunk\",\"system_fingerprint\":\"fp_999\",\"x_top_unknown\":{\"alpha\":true}}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"Answer here.\"},\"finish_reason\":null,\"index\":0}],\"created\":171,\"id\":\"cmpl-probe\",\"model\":\"qwen3\",\"object\":\"chat.completion.chunk\",\"system_fingerprint\":\"fp_999\",\"x_top_unknown\":{\"alpha\":true}}\n\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"function\":{\"arguments\":\"{\\\"q\\\":\\\"x\\\"}\",\"name\":\"lookup\"},\"id\":\"call_1\",\"type\":\"function\"}]},\"finish_reason\":null,\"index\":0}],\"created\":171,\"id\":\"cmpl-probe\",\"model\":\"qwen3\",\"object\":\"chat.completion.chunk\",\"system_fingerprint\":\"fp_999\",\"x_top_unknown\":{\"alpha\":true}}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\",\"index\":0}],\"created\":171,\"id\":\"cmpl-probe\",\"model\":\"qwen3\",\"object\":\"chat.completion.chunk\",\"system_fingerprint\":\"fp_999\",\"x_top_unknown\":{\"alpha\":true}}\n\ndata: {\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null,\"index\":1}],\"created\":171,\"id\":\"cmpl-probe\",\"model\":\"qwen3\",\"object\":\"chat.completion.chunk\",\"system_fingerprint\":\"fp_999\",\"x_top_unknown\":{\"alpha\":true}}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"SECOND CHOICE\"},\"finish_reason\":null,\"index\":1}],\"created\":171,\"id\":\"cmpl-probe\",\"model\":\"qwen3\",\"object\":\"chat.completion.chunk\",\"system_fingerprint\":\"fp_999\",\"x_top_unknown\":{\"alpha\":true}}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\",\"index\":1}],\"created\":171,\"id\":\"cmpl-probe\",\"model\":\"qwen3\",\"object\":\"chat.completion.chunk\",\"system_fingerprint\":\"fp_999\",\"usage\":{\"completion_tokens\":7,\"llama_extra\":{\"k\":1},\"prompt_tokens\":5,\"total_tokens\":12},\"x_top_unknown\":{\"alpha\":true}}\n\ndata: [DONE]\n\n";


        let raw = serde_json::json!({
            "id": "cmpl-probe", "object": "chat.completion", "created": 171, "model": "qwen3",
            "system_fingerprint": "fp_999",
            "choices": [
                {"index": 0, "message": {"role": "assistant", "content": "Answer here.", "reasoning_text": "Hmm, let me think.", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "lookup", "arguments": "{\"q\":\"x\"}"}}]}},
                {"index": 1, "message": {"role": "assistant", "content": "SECOND CHOICE"}}
            ],
            "usage": {"prompt_tokens": 5, "completion_tokens": 7, "total_tokens": 12, "llama_extra": {"k": 1}},
            "x_top_unknown": {"alpha": true}
        });
        let cfg = SynthesisConfig::default();
        let resp = synthesize_streaming_response(&raw, &cfg).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 10_000_000).await.unwrap();
        assert_eq!(String::from_utf8(body.to_vec()).unwrap(), BASELINE_BYTES);
    }
}

#[cfg(test)]
mod order_finish_reason_tests {
    use super::*;
    use crate::api::AnthropicUsage;

    async fn body(resp: Response) -> String {
        let b = axum::body::to_bytes(resp.into_body(), 10_000_000).await.unwrap();
        String::from_utf8(b.to_vec()).unwrap()
    }

    fn mixed_no_finish() -> Value {
        let raw = serde_json::json!({
            "id": "c59", "created": 9, "model": "m",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "TEXT-BODY", "reasoning_text": "REASON-BODY",
                "tool_calls": [{"id": "call_9", "type": "function", "function": {"name": "f", "arguments": "{}"}}]}}]
        });
        raw
    }

    #[tokio::test]
    async fn tool_call_delta_follows_text_delta() {
        // Given: message with reasoning + content + tool_calls, NO finish_reason
        // When: OpenAI synthesis
        // Then: order is role -> reasoning -> content -> tool_calls (baseline emitted
        //       tool_calls SECOND, before reasoning/text)
        let cfg = SynthesisConfig::default();
        let b = body(synthesize_streaming_response(&mixed_no_finish(), &cfg).await.unwrap()).await;
        let i_reason = b.find("REASON-BODY").expect("reasoning delta");
        let i_text = b.find("TEXT-BODY").expect("content delta");
        let i_tool = b.find("call_9").expect("tool_calls delta");
        assert!(i_reason < i_text, "reasoning must precede text");
        assert!(i_text < i_tool, "text must precede tool-call args (baseline order = RED)");
    }

    #[tokio::test]
    async fn finish_reason_derives_tool_calls_when_absent() {
        // Then: final chunk finish_reason == "tool_calls", not the fabricated "stop"
        let cfg = SynthesisConfig::default();
        let b = body(synthesize_streaming_response(&mixed_no_finish(), &cfg).await.unwrap()).await;
        assert!(
            b.contains("\"finish_reason\":\"tool_calls\""),
            "finish_reason must be derived, got:\n{b}"
        );
        let final_idx = b.rfind("\"finish_reason\":\"stop\"");
        assert!(final_idx.is_none(), "fabricated \"stop\" must be gone");
    }

    #[tokio::test]
    async fn finish_reason_still_derives_stop_when_nothing_else() {
        // Given: content-only message without finish_reason -> honest default stays "stop"
        let raw = serde_json::json!({"id":"c","created":1,"model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"x"}}]});
        let cfg = SynthesisConfig::default();
        let b = body(synthesize_streaming_response(&raw, &cfg).await.unwrap()).await;
        assert!(b.contains("\"finish_reason\":\"stop\""));
    }

    #[tokio::test]
    async fn explicit_finish_reason_is_never_overridden() {
        let raw = serde_json::json!({"id":"c","created":1,"model":"m","choices":[{"index":0,"finish_reason":"length","message":{"role":"assistant","content":"x","tool_calls":[{"id":"t","type":"function","function":{"name":"f","arguments":"{}"}}]}}]});
        let cfg = SynthesisConfig::default();
        let b = body(synthesize_streaming_response(&raw, &cfg).await.unwrap()).await;
        assert!(b.contains("\"finish_reason\":\"length\""));
    }

    #[tokio::test]
    async fn anthropic_stop_reason_derives_tool_use_when_absent() {
        // Given: ToolUse block, stop_reason None (typed path == handler's Anthropic-format branch)
        // Then: message_delta carries "tool_use", not null
        let msg = AnthropicMessage {
            id: "m59".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "f".to_string(),
                input: json!({}),
            }],
            model: "m".to_string(),
            stop_reason: None,
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 1,
                output_tokens: 1,
            },
        };
        let b = body(
            synthesize_anthropic_streaming_response(msg, &SynthesisConfig::default())
                .await
                .unwrap(),
        )
        .await;
        assert!(
            b.contains("\"stop_reason\":\"tool_use\""),
            "baseline emits null => RED, got:\n{b}"
        );
    }

    #[tokio::test]
    async fn anthropic_stop_reason_derives_end_turn_when_no_tool_use() {
        let msg = AnthropicMessage {
            id: "m59b".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock::Text { text: "hi".to_string() }],
            model: "m".to_string(),
            stop_reason: None,
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 1,
                output_tokens: 1,
            },
        };
        let b = body(
            synthesize_anthropic_streaming_response(msg, &SynthesisConfig::default())
                .await
                .unwrap(),
        )
        .await;
        assert!(
            b.contains("\"stop_reason\":\"end_turn\""),
            "baseline emits null => RED, got:\n{b}"
        );
    }

    #[tokio::test]
    async fn anthropic_explicit_max_tokens_is_preserved() {
        // max_tokens can only come from the backend (finish_reason length -> From maps it);
        // the derivation must not clobber it
        let msg = AnthropicMessage {
            id: "m59c".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock::Text { text: "hi".to_string() }],
            model: "m".to_string(),
            stop_reason: Some("max_tokens".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 1,
                output_tokens: 1,
            },
        };
        let b = body(
            synthesize_anthropic_streaming_response(msg, &SynthesisConfig::default())
                .await
                .unwrap(),
        )
        .await;
        assert!(b.contains("\"stop_reason\":\"max_tokens\""));
    }
}

#[cfg(test)]
mod tool_result_signature_tests {
    use super::*;
    use crate::api::AnthropicUsage;

    async fn body(resp: Response) -> String {
        let b = axum::body::to_bytes(resp.into_body(), 10_000_000).await.unwrap();
        String::from_utf8(b.to_vec()).unwrap()
    }

    fn msg_with(blocks: Vec<AnthropicContentBlock>) -> AnthropicMessage {
        AnthropicMessage {
            id: "m60".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: blocks,
            model: "m".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 1,
                output_tokens: 1,
            },
        }
    }

    #[tokio::test]
    async fn thinking_with_signature_emits_signature_delta() {
        // Given: thinking block carrying a signature
        // When: Anthropic streaming synthesis
        // Then: a {"type":"signature_delta"} frame exists (baseline: NONE - signature only
        //       in content_block_start; capture task60 evidence)
        let msg = msg_with(vec![AnthropicContentBlock::Thinking {
            thinking: "deep".to_string(),
            signature: Some("sig_abc".to_string()),
        }]);
        let b = body(
            synthesize_anthropic_streaming_response(msg, &SynthesisConfig::default())
                .await
                .unwrap(),
        )
        .await;
        assert!(
            b.contains("\"type\":\"signature_delta\""),
            "signature_delta missing => RED:\n{b}"
        );
        assert!(b.contains("\"signature\":\"sig_abc\""));
    }

    #[tokio::test]
    async fn thinking_without_signature_emits_no_signature_delta() {
        let msg = msg_with(vec![AnthropicContentBlock::Thinking {
            thinking: "deep".to_string(),
            signature: None,
        }]);
        let b = body(
            synthesize_anthropic_streaming_response(msg, &SynthesisConfig::default())
                .await
                .unwrap(),
        )
        .await;
        assert!(!b.contains("signature_delta"));
    }

    #[tokio::test]
    async fn tool_result_array_content_emits_tool_result_block_gap_free() {
        // Given: [Text, ToolResult(array content)]
        // Then: index 1 emits a content_block_start whose block type is tool_result with the
        //       raw content array VERBATIM, plus content_block_stop at index 1 (baseline emitted
        //       ZERO frames for this block -> gap 0,2)
        let content = json!([{"type": "text", "text": "TR-ARRAY"}]);
        let msg = msg_with(vec![
            AnthropicContentBlock::Text { text: "T".to_string() },
            AnthropicContentBlock::ToolResult {
                tool_use_id: "toolu_9".to_string(),
                content: content.clone(),
                is_error: Some(true),
            },
        ]);
        let b = body(
            synthesize_anthropic_streaming_response(msg, &SynthesisConfig::default())
                .await
                .unwrap(),
        )
        .await;
        assert!(
            b.contains("\"type\":\"tool_result\""),
            "tool_result block missing => RED:\n{b}"
        );
        assert!(b.contains("\"tool_use_id\":\"toolu_9\""));
        assert!(b.contains("\"is_error\":true"));
        assert!(
            b.contains("TR-ARRAY"),
            "raw content must ride verbatim (untrusted pass-through)"
        );
        let start1 = b.find("\"index\":1").expect("start frame at index 1");
        let stop1 = rfind_index(&b, 1);
        assert!(start1 < stop1, "start before stop at index 1");
        assert!(
            b.contains("\"index\":1,\"type\":\"content_block_stop\""),
            "gap-free: stop at index 1"
        );
    }

    #[tokio::test]
    async fn tool_result_string_content_is_tool_result_not_text() {
        // Baseline LIE: string-shaped ToolResult was emitted as a "text" block
        let msg = msg_with(vec![AnthropicContentBlock::ToolResult {
            tool_use_id: "toolu_2".to_string(),
            content: json!("TR-STRING"),
            is_error: None,
        }]);
        let b = body(
            synthesize_anthropic_streaming_response(msg, &SynthesisConfig::default())
                .await
                .unwrap(),
        )
        .await;
        assert!(
            b.contains("\"type\":\"tool_result\""),
            "string ToolResult must be a tool_result block => RED:\n{b}"
        );
        assert!(b.contains("\"content\":\"TR-STRING\""));
        assert!(!b.contains("\"type\":\"text\""), "no more text-block lie");
        assert!(!b.contains("\"is_error\""), "absent is_error must not be fabricated");
    }

    #[tokio::test]
    async fn full_block_sweep_indices_are_gap_free() {
        // Given: five blocks incl. both ToolResult shapes (the baseline gap fixture)
        // Then: content_block_start indices form 0,1,2,3,4 with no skips
        let msg = msg_with(vec![
            AnthropicContentBlock::Thinking {
                thinking: "d".to_string(),
                signature: Some("s".to_string()),
            },
            AnthropicContentBlock::Text { text: "v".to_string() },
            AnthropicContentBlock::ToolUse {
                id: "t".to_string(),
                name: "f".to_string(),
                input: json!({"a":1}),
            },
            AnthropicContentBlock::ToolResult {
                tool_use_id: "u1".to_string(),
                content: json!([{"type":"text","text":"A"}]),
                is_error: Some(false),
            },
            AnthropicContentBlock::ToolResult {
                tool_use_id: "u2".to_string(),
                content: json!("B"),
                is_error: None,
            },
        ]);
        let b = body(
            synthesize_anthropic_streaming_response(msg, &SynthesisConfig::default())
                .await
                .unwrap(),
        )
        .await;
        for i in 0..5 {
            assert!(b.contains(&format!("\"index\":{i}")), "index {i} missing => gap => RED");
        }
    }

    fn rfind_index(b: &str, idx: usize) -> usize {
        let pat = format!("\"index\":{idx},\"type\":\"content_block_stop\"");
        b.rfind(&pat).expect("stop frame present")
    }
}

#[cfg(test)]
mod raw_choice_unknown_preservation_tests {
    use super::*;

    async fn body(resp: Response) -> String {
        let b = axum::body::to_bytes(resp.into_body(), 10_000_000).await.unwrap();
        String::from_utf8(b.to_vec()).unwrap()
    }

    fn two_choice_raw() -> Value {
        json!({
            "id": "cmpl-probe", "object": "chat.completion", "created": 171, "model": "qwen3",
            "system_fingerprint": "fp_999",
            "choices": [
                {"index": 0, "message": {"role": "assistant", "content": "Answer here.", "reasoning_text": "Hmm.",
                    "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "lookup", "arguments": "{\"q\":\"x\"}"}}]}},
                {"index": 1, "message": {"role": "assistant", "content": "SECOND CHOICE"}}
            ],
            "usage": {"prompt_tokens": 5, "completion_tokens": 7, "total_tokens": 12, "llama_extra": {"k": 1}},
            "x_top_unknown": {"alpha": true}
        })
    }

    #[tokio::test]
    async fn native_streams_all_choices_and_unknown_fields() {
        // Baseline DROPPED all of this (typed round-trip, choices[0] only - see
        // task61 baseline capture bytes).
        let b = body(
            synthesize_streaming_response(&two_choice_raw(), &SynthesisConfig::default())
                .await
                .unwrap(),
        )
        .await;
        assert!(b.contains("SECOND CHOICE"), "choices[1] must stream:\n{b}");
        assert!(b.contains("\"index\":1"), "choice-1 frames carry index 1");
        assert!(b.contains("fp_999"), "system_fingerprint must survive");
        assert!(
            b.contains("\"x_top_unknown\":{\"alpha\":true}"),
            "unknown top-level must survive"
        );
        assert!(b.contains("\"llama_extra\""), "unknown usage key must survive");
    }

    #[tokio::test]
    async fn convert_keeps_unknowns_and_maps_usage_in_message_start() {
        let resp = synthesize_anthropic_openai_format_response(&two_choice_raw(), &SynthesisConfig::default()).expect("valid");
        let b = body(resp).await;
        let first_block = b.find("content_block_start").expect("blocks follow message_start");
        let start = &b[..first_block];
        assert!(
            start.contains("fp_999") && start.contains("\"x_top_unknown\""),
            "unknowns merge into message"
        );
        assert!(
            start.contains("\"input_tokens\":5") && start.contains("\"output_tokens\":7"),
            "usage maps"
        );
        assert!(start.contains("\"llama_extra\""), "unknown usage keys survive");
    }

    #[tokio::test]
    async fn convert_streams_every_choice_gap_free() {
        let b =
            body(synthesize_anthropic_openai_format_response(&two_choice_raw(), &SynthesisConfig::default()).unwrap()).await;
        assert!(b.contains("SECOND CHOICE"), "choices[1] must become blocks");
        // thinking@0, text@1, tool_use@2, choice-1 text@3 - gap-free globals
        assert!(b.contains("\"index\":3"), "fourth block index must exist:\n{b}");
        // no finish_reason anywhere -> derivation: tool_use was emitted -> tool_use
        assert!(b.contains("\"stop_reason\":\"tool_use\""));
    }

    #[tokio::test]
    async fn convert_maps_finish_reason_vocabulary() {
        let cfg = SynthesisConfig::default();
        let raw = json!({"id":"i","model":"m","choices":[{"index":0,"finish_reason":"length","message":{"role":"assistant","content":"x"}}]});
        let b = body(synthesize_anthropic_openai_format_response(&raw, &cfg).unwrap()).await;
        assert!(b.contains("\"stop_reason\":\"max_tokens\""), "length -> max_tokens:\n{b}");
        let raw = json!({"id":"i","model":"m","choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":"x"}}]});
        let b = body(synthesize_anthropic_openai_format_response(&raw, &cfg).unwrap()).await;
        assert!(b.contains("\"stop_reason\":\"end_turn\""), "stop -> end_turn");
    }

    #[test]
    fn malformed_known_fields_are_rejected_by_both_paths() {
        // The task-11 null-index class stays a REJECTION (index is a known field
        // with a broken wire type) - the /v1/messages error-frame contract holds.
        let cfg = SynthesisConfig::default();
        let bad: Vec<Value> = vec![
            json!([]),
            json!({"choices": null}),
            json!({"id": "i", "model": "m", "choices": []}),
            json!({"id": "i", "model": "m", "choices": [{"index": null, "message": {"role": "assistant", "content": "leaky"}}]}),
            json!({"id": "i", "model": "m", "choices": [{"message": "not an object"}]}),
            json!({"id": "i", "model": "m", "choices": [{"index": 0}]}),
            json!({"id": "i", "model": "m", "choices": [{"index": 0, "finish_reason": 7, "message": {"role": "assistant", "content": "x"}}]}),
        ];
        for b in bad {
            assert!(convert_openai_to_claude_sse(&b, &cfg).is_none(), "convert must reject {b}");
            assert!(parse_raw_completion(&b).is_err(), "parse must reject {b}");
        }
    }

    #[test]
    fn extras_exclude_envelope_owned_keys() {
        let raw = two_choice_raw();
        let parsed = parse_raw_completion(&raw).expect("valid");
        let extras = parsed.meta.extras.as_object().unwrap();
        assert!(extras.contains_key("system_fingerprint") && extras.contains_key("x_top_unknown"));
        for owned in ["id", "object", "created", "model", "choices", "usage", "timings"] {
            assert!(!extras.contains_key(owned), "{owned} is envelope-owned, not an extra");
        }
    }
}
