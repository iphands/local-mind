//! Augment backend integration
//!
//! Enriches incoming user requests by calling a fast LLM backend
//! to generate additional context before forwarding to the main backend.

use crate::api::{Message, MessageContent};
use crate::config::AugmentBackendConfig;
use crate::prompt_cache::{self, Refresh};
use std::path::Path;

/// Augment backend client
pub struct AugmentBackend {
    pub url: String,
    pub model: String,
    pub prompt_file: String,
    pub request_prompt_file: String,
    pub http_client: reqwest::Client,
}

impl AugmentBackend {
    /// Create from config
    pub fn from_config(config: &AugmentBackendConfig) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()?;

        Ok(Self {
            url: config.url.trim_end_matches('/').to_string(),
            model: config.model.clone(),
            prompt_file: config.prompt_file.clone(),
            request_prompt_file: config.request_prompt_file.clone(),
            http_client,
        })
    }

    /// Load and return the request prompt file contents
    pub fn load_request_prompt(&self) -> Result<String, std::io::Error> {
        std::fs::read_to_string(&self.request_prompt_file)
    }

    /// Get augmentation text for the given user content.
    /// Loads backend_prompt.md, combines with user content, calls augment backend,
    /// and returns the extracted text from the response.
    pub async fn get_augmentation(&self, user_content: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Load backend prompt through the shared mtime-checked cache
        // (empty string on failure)
        let backend_prompt = self.load_backend_prompt().await;

        // Combine backend_prompt + user_content
        let combined = format!("{}\n\n{}", backend_prompt, user_content);

        // Build a simple non-tool-calling chat completion request
        let request = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": combined
                }
            ],
            "stream": false
        });

        let url = format!("{}/v1/chat/completions", self.url);

        tracing::debug!(url = %url, model = %self.model, "Sending request to augment backend");

        let response = self.http_client.post(&url).json(&request).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::debug!(status = %status, body = %body, "Augment backend returned non-success response");
            return Err(format!("Augment backend returned {}: {}", status, clip_error_body(&body)).into());
        }

        let body: serde_json::Value = response.json().await?;

        // Extract text from response (supports OpenAI and Anthropic format)
        extract_response_text(&body)
    }

    /// Backend prompt via the shared mtime-checked cache; `tokio::fs` reads so
    /// the async request path never blocks the runtime on disk.
    async fn load_backend_prompt(&self) -> String {
        let path = Path::new(&self.prompt_file);
        let cache = prompt_cache::shared(path).await;
        match cache.refresh(path).await {
            Refresh::Reloaded { text, .. } => text,
            Refresh::Empty => String::new(),
            Refresh::Unchanged => cache.text().await,
            Refresh::Failed(e) => {
                tracing::warn!(error = %e, file = %self.prompt_file, "Failed to load backend prompt, using empty string");
                String::new()
            }
        }
    }
}

/// First 300 CHARS (not bytes - CJK-safe) of an augment backend error body,
/// with the repo's `...` truncation marker. Errors from this module are
/// embedded into client-facing 502 envelopes at handler.rs, so an unbounded
/// backend body must never ride along. Full body goes to `tracing::debug!`.
fn clip_error_body(body: &str) -> String {
    const MAX_CHARS: usize = 300;
    let mut chars = body.chars();
    let kept: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{kept}...")
    } else {
        kept
    }
}

/// Extract text content from an OpenAI or Anthropic API response body
fn extract_response_text(body: &serde_json::Value) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Try OpenAI format: choices[0].message.content
    if let Some(choices) = body.get("choices").and_then(|c| c.as_array()) {
        if let Some(first) = choices.first() {
            if let Some(content) = first.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_str()) {
                return Ok(content.to_string());
            }
        }
    }

    // Try Anthropic format: content[].text
    if let Some(content) = body.get("content").and_then(|c| c.as_array()) {
        let text: String = content
            .iter()
            .filter_map(|block| {
                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                    block.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        if !text.is_empty() {
            return Ok(text);
        }
    }

    tracing::debug!(body = %body, "Could not extract text from augment backend response");
    Err(format!(
        "Could not extract text from augment backend response: {}",
        clip_error_body(&body.to_string())
    )
    .into())
}

/// Extract user content text directly from a raw JSON request body.
///
/// Works for both OpenAI (`messages[].content`) and Anthropic (`messages[].content[].text`)
/// formats without requiring full struct deserialization — avoids silent failures when
/// the request has fields that don't match the strict struct types.
pub fn extract_user_content_from_json(req_json: &serde_json::Value) -> String {
    let messages = match req_json.get("messages").and_then(|m| m.as_array()) {
        Some(m) => m,
        None => return String::new(),
    };

    let mut parts: Vec<String> = Vec::new();

    for msg in messages {
        if msg.get("role").and_then(|r| r.as_str()) != Some("user") {
            continue;
        }

        match msg.get("content") {
            // OpenAI string content: {"role":"user","content":"hello"}
            Some(serde_json::Value::String(s)) => {
                parts.push(s.clone());
            }
            // Array content (both OpenAI parts and Anthropic blocks)
            Some(serde_json::Value::Array(blocks)) => {
                for block in blocks {
                    // OpenAI content part: {"type":"text","text":"..."}
                    // Anthropic text block: {"type":"text","text":"..."}
                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                            parts.push(t.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    parts.join("\n")
}

/// Extract text content from OpenAI messages where role == "user"
pub fn extract_user_content(messages: &[Message]) -> Vec<String> {
    messages
        .iter()
        .filter(|m| m.role == "user")
        .filter_map(|m| match &m.content {
            Some(MessageContent::Text(text)) => Some(text.clone()),
            Some(MessageContent::Parts(parts)) => {
                let text: String = parts
                    .iter()
                    .filter_map(|part| {
                        if part.content_type == "text" {
                            part.text.clone()
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
            None => None,
        })
        .collect()
}

/// Inject an augmentation block into a raw JSON chat-completion request with
/// `serde_json::Value` surgery - no typed `ChatCompletionRequest` round-trip.
///
/// Unknown fields at every level (top-level, message-level, content-part-level)
/// survive BY CONSTRUCTION: the request stays a `Value` throughout and only the
/// keys the injection needs are touched, so byte-passthrough of everything else
/// is not a feature to implement but the shape of the operation.
///
/// Semantics (mapped from the typed implementation this replaces, observable
/// behavior preserved):
/// - The block is `"\n\n{request_prompt}\n\n{augmentation}"`, appended to the
///   LAST `role == "user"` message. That target is the shipped contract (this
///   function's doc, the README, and the handler's augmented-request log all
///   describe enriching the user message), so it is kept.
///   - string `content`: block concatenated onto the end.
///   - array `content`: block appended to the last `{"type":"text"}` part's
///     `text`; a new `{"type":"text","text":<block>}` part is pushed at the
///     end when no part has `type: "text"` OR when the last text part's
///     `text` is absent, `null`, or not a string (F-M12, task 45: the block
///     is never dropped and such a part is never mutated).
///   - absent or `null` `content`: becomes the block string.
///   - any other `content` type: `Err`. The typed deserializer gate rejected
///     such requests wholesale (handler forwarded the original bytes, no
///     injection), so an error keeps that net effect for callers.
/// - `"stop": "<string>"` is normalized to `["<string>"]` BEFORE injection, on
///   every success path regardless of which injection path runs. The typed
///   model is `Option<Vec<String>>`, so a string `stop` previously failed the
///   whole typed gate and augmentation was silently skipped for the entire
///   request. Non-string `stop` values (array, number, absent) are untouched.
/// - Repeat injection is NOT idempotent: each call appends another block.
///   There is no begin/end marker and no replace semantics today; pinned as-is.
/// - No user message and non-empty `messages`: a NEW
///   `{"role":"system","content":<block>}` message is pushed at the tail;
///   `messages[0]` is never written to (the typed implementation appended the
///   block onto `messages[0]` instead, destroying the user's own system prompt
///   when it sat there). An empty `messages: []` stays a no-op - no system
///   message is invented for a conversation with no turns.
/// - `messages` missing, non-array, or a non-object body: `Err` - the typed
///   gate rejected those too, and callers treat `Err` as "forward the
///   original bytes unchanged".
pub fn inject_augmentation_value(
    mut request: serde_json::Value,
    request_prompt: &str,
    augmentation: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    if !request.is_object() {
        return Err("augment injection: request body is not a JSON object".into());
    }

    // `"stop": "<string>"` -> `["<string>"]`, BEFORE any injection.
    let normalized_stop = match request.get("stop") {
        Some(serde_json::Value::String(s)) => Some(serde_json::json!([s.clone()])),
        _ => None,
    };
    if let (Some(body), Some(stop)) = (request.as_object_mut(), normalized_stop) {
        body.insert("stop".to_string(), stop);
    }

    let block = format!("\n\n{}\n\n{}", request_prompt, augmentation);

    let messages = request
        .get_mut("messages")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or("augment injection: request has no \"messages\" array")?;

    if let Some(idx) = messages
        .iter()
        .rposition(|m| m.get("role").and_then(serde_json::Value::as_str) == Some("user"))
    {
        append_block_to_message(&mut messages[idx], &block)?;
    } else if !messages.is_empty() {
        // No user message: the typed implementation appended the block onto
        // messages[0], destroying the user's own system prompt when that first
        // message held it. The block now always lands in a NEW system message
        // pushed at the tail; messages[0] is never written to here and the
        // block is never dropped, whatever messages[0] holds.
        tracing::warn!("No user message to inject augmentation into; augmentation pushed as a new system message");
        messages.push(serde_json::json!({ "role": "system", "content": block }));
    }

    Ok(request)
}

/// Append the augmentation block to one message's `content`, following the
/// string/array/absent rules documented on [`inject_augmentation_value`].
fn append_block_to_message(msg: &mut serde_json::Value, block: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if matches!(msg.get("content"), None | Some(serde_json::Value::Null)) {
        // Absent or null content becomes the block itself (typed `None` arm).
        // A `role` match above already proves `msg` is an object, so the
        // object-index assignment below cannot panic.
        msg["content"] = serde_json::Value::String(block.to_string());
        return Ok(());
    }
    match msg.get_mut("content").ok_or("augment injection: message has no content")? {
        serde_json::Value::String(existing) => existing.push_str(block),
        serde_json::Value::Array(parts) => {
            let appended = if let Some(part) = parts
                .iter_mut()
                .rev()
                .find(|p| p.get("type").and_then(serde_json::Value::as_str) == Some("text"))
            {
                match part.get_mut("text") {
                    Some(serde_json::Value::String(text)) => {
                        text.push_str(block);
                        true
                    }
                    // F-M12: a text-typed part whose `text` is absent, null,
                    // or a non-string is never mutated here; the block rides
                    // in the fresh part appended below instead.
                    _ => false,
                }
            } else {
                false
            };
            if !appended {
                parts.push(serde_json::json!({ "type": "text", "text": block }));
            }
        }
        other => return Err(format!("augment injection: unsupported message content type: {other}").into()),
    }
    Ok(())
}

/// Inject augmentation text into the last user message of an OpenAI ChatCompletionRequest.
///
/// The injected suffix is: "\n\n{request_prompt}\n\n{augmentation}"
///
/// Compatibility shim: serializes the typed request, runs
/// [`inject_augmentation_value`] on the `serde_json::Value`, and deserializes
/// the result. The typed round-trip through this shim still erases message- and
/// content-part-level unknown fields (`Message` has no flatten catcher);
/// callers holding raw request JSON should call [`inject_augmentation_value`]
/// directly to keep them.
pub fn inject_augmentation(
    request: crate::api::ChatCompletionRequest,
    request_prompt: &str,
    augmentation: &str,
) -> Result<crate::api::ChatCompletionRequest, Box<dyn std::error::Error + Send + Sync>> {
    let value = serde_json::to_value(&request)?;
    let value = inject_augmentation_value(value, request_prompt, augmentation)?;
    Ok(serde_json::from_value(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ChatCompletionRequest, Message, MessageContent};

    fn make_request(messages: Vec<Message>) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "test".to_string(),
            messages,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            user: None,
            reasoning_effort: None,
            verbosity: None,
            thinking_budget: None,
            extra: Default::default(),
        }
    }

    fn user_msg(content: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: Some(MessageContent::Text(content.to_string())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn assistant_msg(content: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text(content.to_string())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn test_extract_user_content_simple() {
        let msgs = vec![user_msg("Hello"), assistant_msg("Hi"), user_msg("World")];
        let result = extract_user_content(&msgs);
        assert_eq!(result, vec!["Hello", "World"]);
    }

    #[test]
    fn test_extract_user_content_empty() {
        let msgs: Vec<Message> = vec![];
        let result = extract_user_content(&msgs);
        assert!(result.is_empty());
    }

    #[test]
    fn test_inject_augmentation_last_user() {
        let req = make_request(vec![user_msg("Hello"), assistant_msg("Hi"), user_msg("World")]);
        let result = inject_augmentation(req, "req_prompt", "aug_text").unwrap();

        let last = result.messages.last().unwrap();
        if let Some(MessageContent::Text(t)) = &last.content {
            assert!(t.contains("World"));
            assert!(t.contains("req_prompt"));
            assert!(t.contains("aug_text"));
        } else {
            panic!("Expected text content");
        }
    }

    #[test]
    fn test_inject_augmentation_only_user() {
        let req = make_request(vec![user_msg("Hello, Claude")]);
        let result = inject_augmentation(req, "my_prompt", "extra_info").unwrap();

        if let Some(MessageContent::Text(t)) = &result.messages[0].content {
            assert!(t.starts_with("Hello, Claude"));
            assert!(t.contains("my_prompt"));
            assert!(t.contains("extra_info"));
        } else {
            panic!("Expected text content");
        }
    }

    #[test]
    fn test_extract_response_text_openai() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello guy!!!"
                }
            }]
        });
        let result = extract_response_text(&body).unwrap();
        assert_eq!(result, "Hello guy!!!");
    }

    #[test]
    fn test_extract_response_text_anthropic() {
        let body = serde_json::json!({
            "id": "msg_01XFDUDYJgAACzvnptvVoYEL",
            "type": "message",
            "role": "assistant",
            "content": [
                { "type": "text", "text": "Hello guy!!!" }
            ]
        });
        let result = extract_response_text(&body).unwrap();
        assert_eq!(result, "Hello guy!!!");
    }

    #[test]
    fn test_extract_response_text_unknown() {
        let body = serde_json::json!({ "foo": "bar" });
        assert!(extract_response_text(&body).is_err());
    }

    fn inject(json: serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        inject_augmentation_value(json, "REQ_PROMPT", "AUG_TEXT")
    }

    fn fixture_unknown_fields() -> serde_json::Value {
        serde_json::json!({
            "model": "test-model",
            "x_custom": {"deep": {"nested": [1, 2, 3], "uni": "日本語"}},
            "messages": [
                {"role": "system", "content": "SYS PROMPT", "x_sys_meta": {"k": [1, 2]}},
                {
                    "role": "user",
                    "content": [{"type": "text", "text": "hello", "x_part_meta": {"p": true}}],
                    "x_msg_level": "keep-me"
                }
            ],
            "stop": ["END"],
            "top_k": 40
        })
    }

    #[test]
    fn value_injection_keeps_unknown_fields_when_typed_roundtrip_loses_them() {
        let input = fixture_unknown_fields();

        let baseline_loss = serde_json::to_value(
            inject_augmentation(
                serde_json::from_value::<ChatCompletionRequest>(input.clone()).unwrap(),
                "REQ_PROMPT",
                "AUG_TEXT",
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            baseline_loss["messages"][1].get("x_msg_level").is_none()
                && baseline_loss["messages"][0].get("x_sys_meta").is_none()
                && baseline_loss["messages"][1]["content"][0].get("x_part_meta").is_none(),
            "the typed round-trip must be shown to erase message/part-level unknowns"
        );

        let output = inject(input.clone()).unwrap();

        let bytes = |v: &serde_json::Value| serde_json::to_vec(v).expect("serialize");
        assert_eq!(bytes(&input["x_custom"]), bytes(&output["x_custom"]), "unknown top-level key");
        assert_eq!(
            bytes(&input["messages"][0]["x_sys_meta"]),
            bytes(&output["messages"][0]["x_sys_meta"]),
            "unknown system-message key"
        );
        assert_eq!(
            bytes(&input["messages"][1]["x_msg_level"]),
            bytes(&output["messages"][1]["x_msg_level"]),
            "unknown user-message key"
        );
        assert_eq!(
            bytes(&input["messages"][1]["content"][0]["x_part_meta"]),
            bytes(&output["messages"][1]["content"][0]["x_part_meta"]),
            "unknown content-part key"
        );

        let mut expected = input;
        expected["messages"][1]["content"][0]["text"] = serde_json::json!("hello\n\nREQ_PROMPT\n\nAUG_TEXT");
        assert_eq!(output, expected, "output must equal the input plus ONLY the injected block");
    }

    #[test]
    fn value_injection_normalizes_string_stop_to_array_before_injection() {
        let output = inject(serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": "hi"}],
            "stop": "END"
        }))
        .unwrap();

        assert_eq!(output["stop"], serde_json::json!(["END"]));
        assert_eq!(output["messages"][0]["content"], "hi\n\nREQ_PROMPT\n\nAUG_TEXT");
    }

    #[test]
    fn value_injection_normalizes_stop_even_without_user_message_and_cjk_stays_byte_safe() {
        let output = inject(serde_json::json!({
            "model": "m",
            "messages": [{"role": "assistant", "content": "prior"}],
            "stop": "終り"
        }))
        .unwrap();

        assert_eq!(
            serde_json::to_vec(&output["stop"]).unwrap(),
            serde_json::to_vec(&serde_json::json!(["終り"])).unwrap()
        );
    }

    #[test]
    fn value_injection_leaves_non_string_stop_untouched() {
        let array = inject(serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": "hi"}],
            "stop": ["a", "b"]
        }))
        .unwrap();
        assert_eq!(array["stop"], serde_json::json!(["a", "b"]));

        let absent = inject(serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .unwrap();
        assert!(absent.get("stop").is_none(), "no stop key must be invented");

        let scalar = inject(serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": "hi"}],
            "stop": 5
        }))
        .unwrap();
        assert_eq!(scalar["stop"], serde_json::json!(5));
    }

    #[test]
    fn value_injection_string_content_concat_is_exact() {
        let output = inject(serde_json::json!({
            "model": "m",
            "messages": [
                {"role": "user", "content": "first"},
                {"role": "assistant", "content": "reply"},
                {"role": "user", "content": "World"}
            ]
        }))
        .unwrap();

        assert_eq!(output["messages"][2]["content"], "World\n\nREQ_PROMPT\n\nAUG_TEXT");
        assert_eq!(
            output["messages"][0]["content"], "first",
            "only the LAST user message is touched"
        );
        assert_eq!(output["messages"][1]["content"], "reply");
    }

    #[test]
    fn value_injection_array_content_appends_to_last_text_part_and_keeps_part_unknowns() {
        let input = serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "A"},
                {"type": "image_url", "image_url": {"url": "u"}, "x_meta": 1},
                {"type": "text", "text": "B"}
            ]}]
        });

        let output = inject(input.clone()).unwrap();

        let parts = output["messages"][0]["content"].as_array().unwrap();
        assert_eq!(parts[0]["text"], "A");
        assert_eq!(
            serde_json::to_vec(&parts[1]).unwrap(),
            serde_json::to_vec(&input["messages"][0]["content"][1]).unwrap(),
            "untouched parts stay byte-identical including unknown keys"
        );
        assert_eq!(parts[2]["text"], "B\n\nREQ_PROMPT\n\nAUG_TEXT");
    }

    #[test]
    fn value_injection_array_without_text_parts_pushes_new_text_part() {
        let output = inject(serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": [{"type": "image_url", "image_url": {"url": "u"}}]}]
        }))
        .unwrap();
        let parts = output["messages"][0]["content"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(
            parts[1],
            serde_json::json!({"type": "text", "text": "\n\nREQ_PROMPT\n\nAUG_TEXT"})
        );

        let empty = inject(serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": []}]
        }))
        .unwrap();
        assert_eq!(empty["messages"][0]["content"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn value_injection_text_part_with_null_text_appends_new_part_since_task45() {
        let input = serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": [{"type": "text", "text": null}]}]
        });

        let output = inject(input).unwrap();

        let parts = output["messages"][0]["content"].as_array().unwrap();
        assert_eq!(parts.len(), 2, "task 45 (F-M12): the block is appended, never dropped");
        assert_eq!(parts[1]["text"], "\n\nREQ_PROMPT\n\nAUG_TEXT");
    }

    #[test]
    fn value_injection_absent_or_null_content_becomes_block_string() {
        let absent = inject(serde_json::json!({"model": "m", "messages": [{"role": "user"}]})).unwrap();
        assert_eq!(absent["messages"][0]["content"], "\n\nREQ_PROMPT\n\nAUG_TEXT");

        let null = inject(serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": null}]
        }))
        .unwrap();
        assert_eq!(null["messages"][0]["content"], "\n\nREQ_PROMPT\n\nAUG_TEXT");
    }

    #[test]
    fn repeat_value_injection_appends_second_block_no_replace_semantics_today() {
        let once = inject(serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": "X"}]
        }))
        .unwrap();
        let twice = inject(once).unwrap();

        assert_eq!(
            twice["messages"][0]["content"], "X\n\nREQ_PROMPT\n\nAUG_TEXT\n\nREQ_PROMPT\n\nAUG_TEXT",
            "current semantics have no marker/replace: repeat injection duplicates the block"
        );
    }

    #[test]
    fn value_injection_json_braces_in_augment_text_do_not_corrupt_structure() {
        let evil = "{\"evil\":\"}{\",\"nested\":{\"a\":[1,2]}}";
        let output = inject_augmentation_value(
            serde_json::json!({
                "model": "m",
                "x_custom": {"keep": [1, {"y": null}]},
                "messages": [{"role": "user", "content": "base"}]
            }),
            "{{{{",
            evil,
        )
        .unwrap();

        assert_eq!(output["messages"][0]["content"], format!("base\n\n{}\n\n{}", "{{{{", evil));
        assert_eq!(output["x_custom"], serde_json::json!({"keep": [1, {"y": null}]}));
        let reparsed: serde_json::Value = serde_json::from_slice(&serde_json::to_vec(&output).unwrap()).unwrap();
        assert_eq!(reparsed["messages"][0]["content"], output["messages"][0]["content"]);
    }

    #[test]
    fn value_injection_cjk_prompt_augment_and_content_are_byte_safe() {
        let output = inject_augmentation_value(
            serde_json::json!({
                "model": "m",
                "messages": [{"role": "user", "content": "こんにちは"}]
            }),
            "日本語プロンプト",
            "拡張コンテキスト🚀",
        )
        .unwrap();

        assert_eq!(
            serde_json::to_vec(&output["messages"][0]["content"]).unwrap(),
            serde_json::to_vec(&serde_json::json!("こんにちは\n\n日本語プロンプト\n\n拡張コンテキスト🚀")).unwrap()
        );
    }

    #[test]
    fn value_injection_rejects_bodies_the_typed_gate_rejected() {
        assert!(inject(serde_json::json!("not an object")).is_err());
        assert!(inject(serde_json::json!([1, 2, 3])).is_err());
        assert!(inject(serde_json::json!({"model": "m", "messages": "nope"})).is_err());
        assert!(inject(serde_json::json!({"model": "m"})).is_err(), "missing messages key");
        assert!(
            inject(serde_json::json!({"model": "m", "messages": [{"role": "user", "content": 5}]})).is_err(),
            "number content was rejected by the typed gate"
        );
        assert!(
            inject(serde_json::json!({"model": "m", "messages": [{"role": "user", "content": {"k": 1}}]})).is_err(),
            "object content was rejected by the typed gate"
        );
        assert!(
            inject(serde_json::json!({"model": "m", "messages": ["str", {"role": "user", "content": {"n": 1}}]})).is_err(),
            "last-user path still rejects object content the typed gate rejected"
        );
    }

    #[test]
    fn value_injection_empty_messages_stays_noop() {
        let input = serde_json::json!({"model": "m", "messages": []});
        let output = inject(input.clone()).unwrap();
        assert_eq!(output, input);
    }

    #[test]
    fn no_user_message_pushes_system_message_and_never_touches_messages_zero() {
        let input = serde_json::json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "IMPORTANT USER PROMPT"},
                {"role": "assistant", "content": "prior turn"}
            ]
        });

        let output = inject(input.clone()).unwrap();

        assert_eq!(
            serde_json::to_vec(&output["messages"][0]).unwrap(),
            serde_json::to_vec(&input["messages"][0]).unwrap(),
            "the user's system prompt at messages[0] must survive byte-identical"
        );
        let msgs = output["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3, "augmentation goes into a NEW pushed system message");
        assert_eq!(
            msgs[2],
            serde_json::json!({"role": "system", "content": "\n\nREQ_PROMPT\n\nAUG_TEXT"}),
            "pushed at the tail with the same block string the append paths use"
        );
    }

    #[test]
    fn no_user_with_non_string_first_content_pushes_system_instead_of_dropping_block() {
        let output = inject(serde_json::json!({
            "model": "m",
            "messages": [{"role": "assistant", "content": [{"type": "text", "text": "fine"}]}]
        }))
        .unwrap();

        assert_eq!(output["messages"][0]["content"][0]["text"], "fine", "first message untouched");
        let msgs = output["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(
            msgs[1],
            serde_json::json!({"role": "system", "content": "\n\nREQ_PROMPT\n\nAUG_TEXT"})
        );
    }

    #[test]
    fn user_message_at_index_zero_remains_the_injection_target_contract() {
        let output = inject(serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": "IMPORTANT USER PROMPT"}]
        }))
        .unwrap();

        assert_eq!(
            output["messages"][0]["content"], "IMPORTANT USER PROMPT\n\nREQ_PROMPT\n\nAUG_TEXT",
            "last-user injection is the shipped contract (baseline-D parity): a user \
             message at index 0 is the TARGET, not a clobber victim"
        );
        assert_eq!(output["messages"].as_array().unwrap().len(), 1, "no system message pushed");
    }

    #[test]
    fn no_user_and_empty_messages_stays_noop_without_inventing_system_message() {
        let input = serde_json::json!({"model": "m", "messages": []});
        assert_eq!(inject(input.clone()).unwrap(), input);
    }

    #[test]
    fn fallback_no_longer_rejects_non_object_first_message_it_pushes_instead() {
        let output = inject(serde_json::json!({"model": "m", "messages": [[42]]})).unwrap();
        let msgs = output["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2, "fallback never reads messages[0] any more");
        assert_eq!(msgs[1]["role"], "system");
    }

    // --- task 45 / F-M12: Parts else-arm must APPEND, never drop or mutate ---

    fn parts_of(output: &serde_json::Value) -> Vec<serde_json::Value> {
        output["messages"][0]["content"].as_array().expect("array content").clone()
    }

    #[test]
    fn task45_parts_arm_text_part_with_null_text_appends_new_part_instead_of_dropping() {
        let input = serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": [{"type": "text", "text": null}]}]
        });
        let output = inject(input).unwrap();
        let parts = parts_of(&output);

        assert_eq!(
            parts.len(),
            2,
            "F-M12: block must be APPENDED as a new text part, not dropped"
        );
        assert_eq!(
            parts[0],
            serde_json::json!({"type": "text", "text": null}),
            "the existing part must NOT be mutated (no text: null -> text: Some(block))"
        );
        assert_eq!(
            parts[1],
            serde_json::json!({"type": "text", "text": "\n\nREQ_PROMPT\n\nAUG_TEXT"}),
            "the appended part carries the full block"
        );
    }

    #[test]
    fn task45_parts_arm_text_part_with_absent_text_appends_new_part() {
        let output = inject(serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": [{"type": "text"}]}]
        }))
        .unwrap();
        let parts = parts_of(&output);

        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], serde_json::json!({"type": "text"}), "absent text stays absent");
        assert_eq!(parts[1]["text"], "\n\nREQ_PROMPT\n\nAUG_TEXT");
    }

    #[test]
    fn task45_parts_arm_text_part_with_non_string_text_appends_without_mutating_it() {
        let output = inject(serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": [{"type": "text", "text": 5}]}]
        }))
        .unwrap();
        let parts = parts_of(&output);

        assert_eq!(parts.len(), 2);
        assert_eq!(
            parts[0],
            serde_json::json!({"type": "text", "text": 5}),
            "non-string text untouched"
        );
        assert_eq!(parts[1]["text"], "\n\nREQ_PROMPT\n\nAUG_TEXT");
    }

    #[test]
    fn task45_parts_arm_unusable_last_text_part_appends_after_earlier_usable_one() {
        // Reverse search targets the LAST text-typed part ([2], text: null).
        // It is unusable -> a new part lands at the END of the array; the
        // earlier usable text part ([0]) is NOT the fallback target.
        let output = inject(serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "A"},
                {"type": "image_url", "image_url": {"url": "u"}},
                {"type": "text", "text": null}
            ]}]
        }))
        .unwrap();
        let parts = parts_of(&output);

        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0]["text"], "A", "earlier text part stays untouched");
        assert_eq!(parts[2], serde_json::json!({"type": "text", "text": null}));
        assert_eq!(parts[3]["text"], "\n\nREQ_PROMPT\n\nAUG_TEXT", "new part lands last");
    }

    #[test]
    fn task45_injection_never_loses_the_block_for_any_array_content() {
        // Harm class pin (F-M12): every array-content shape must end up with
        // the block SOMEWHERE in the content - the pre-45 code silently
        // dropped it (handler logged success, user got an unenriched request).
        for content in [
            serde_json::json!([{"type": "text", "text": null}]),
            serde_json::json!([{"type": "text"}]),
            serde_json::json!([{"type": "text", "text": 5}]),
            serde_json::json!([{"type": "text", "text": "A"}, {"type": "image_url", "image_url": {"url": "u"}}, {"type": "text"}]),
            serde_json::json!([{"type": "image_url", "image_url": {"url": "u"}}]),
            serde_json::json!([]),
        ] {
            let output = inject(serde_json::json!({
                "model": "m",
                "messages": [{"role": "user", "content": content}]
            }))
            .expect("injection ok");
            let joined = output["messages"][0]["content"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("|");
            assert!(
                joined.contains("REQ_PROMPT\n\nAUG_TEXT"),
                "augmentation block lost for content: {content}"
            );
        }
    }

    // --- task 47 / F-M14: async fs + mtime-checked prompt cache ---

    fn mtime(seconds: u64) -> std::time::SystemTime {
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds)
    }

    fn write_with_mtime(path: &std::path::Path, content: &str, modified: std::time::SystemTime) {
        std::fs::write(path, content).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();
    }

    async fn spawn_recording_augment_backend() -> (String, tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use tokio::sync::mpsc;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else { return };
                let tx = tx.clone();
                tokio::spawn(async move {
                    let mut sock = sock;
                    let mut head: Vec<u8> = Vec::new();
                    let mut byte = [0u8; 1];
                    loop {
                        match sock.read(&mut byte).await {
                            Ok(0) => return,
                            Ok(_) => {
                                head.push(byte[0]);
                                if head.windows(4).any(|w| w == b"\r\n\r\n") {
                                    break;
                                }
                            }
                            Err(_) => return,
                        }
                    }
                    let content_length = String::from_utf8_lossy(&head)
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
                        .and_then(|l| l.split(':').nth(1))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    let mut body = vec![0u8; content_length];
                    if content_length > 0 && sock.read_exact(&mut body).await.is_err() {
                        return;
                    }
                    if let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&body) {
                        let _ = tx.send(parsed);
                    }
                    let resp_body = r#"{"choices":[{"message":{"role":"assistant","content":"AUGMENTED"}}]}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        resp_body.len(),
                        resp_body
                    );
                    let _ = sock.write_all(response.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        (format!("http://{addr}"), rx)
    }

    async fn first_message_content(rx: &mut tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>) -> String {
        let request = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("augment backend received a request within 5s")
            .expect("recording channel open");
        request["messages"][0]["content"]
            .as_str()
            .expect("string content")
            .to_string()
    }

    fn backend_pointing_at(url: String, prompt_file: std::path::PathBuf) -> AugmentBackend {
        AugmentBackend {
            url,
            model: "fast".to_string(),
            prompt_file: prompt_file.to_string_lossy().into_owned(),
            request_prompt_file: String::new(),
            http_client: reqwest::Client::new(),
        }
    }

    #[tokio::test]
    async fn task47_prompt_file_change_bumped_by_mtime_is_served_without_restart() {
        let (url, mut rx) = spawn_recording_augment_backend().await;
        let dir = tempfile::tempdir().unwrap();
        let prompt = dir.path().join("backend_prompt_v71a.md");
        write_with_mtime(&prompt, "PROMPT-V1", mtime(1_000));
        let backend = backend_pointing_at(url, prompt.clone());

        backend.get_augmentation("USER-TEXT").await.unwrap();
        assert_eq!(first_message_content(&mut rx).await, "PROMPT-V1\n\nUSER-TEXT");

        write_with_mtime(&prompt, "PROMPT-V2", mtime(2_000));
        backend.get_augmentation("USER-TEXT").await.unwrap();
        assert_eq!(
            first_message_content(&mut rx).await,
            "PROMPT-V2\n\nUSER-TEXT",
            "an explicit mtime bump must be visible without restarting the process"
        );

        backend.get_augmentation("USER-TEXT").await.unwrap();
        assert_eq!(
            first_message_content(&mut rx).await,
            "PROMPT-V2\n\nUSER-TEXT",
            "unchanged mtime serves the cached text"
        );
    }

    #[tokio::test]
    async fn task47_content_change_without_mtime_move_serves_cached_stale_by_design() {
        let (url, mut rx) = spawn_recording_augment_backend().await;
        let dir = tempfile::tempdir().unwrap();
        let prompt = dir.path().join("backend_prompt_v71b.md");
        write_with_mtime(&prompt, "PROMPT-V1", mtime(3_000));
        let backend = backend_pointing_at(url, prompt.clone());

        backend.get_augmentation("USER-TEXT").await.unwrap();
        assert_eq!(first_message_content(&mut rx).await, "PROMPT-V1\n\nUSER-TEXT");

        std::fs::write(&prompt, "PROMPT-V2").unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&prompt).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(mtime(3_000))).unwrap();

        backend.get_augmentation("USER-TEXT").await.unwrap();
        assert_eq!(
            first_message_content(&mut rx).await,
            "PROMPT-V1\n\nUSER-TEXT",
            "cache key is path+mtime: content rewritten under a restored mtime is a \
             documented stale read (the uncached baseline re-read and served V2 here)"
        );
    }

    #[tokio::test]
    async fn task47_missing_prompt_file_keeps_the_empty_prompt_fallback_message_for_message() {
        let (url, mut rx) = spawn_recording_augment_backend().await;
        let missing = std::env::temp_dir().join("task47-no-such-augment-prompt-9f3a.md");
        let backend = backend_pointing_at(url, missing);

        let result = backend.get_augmentation("HI").await;

        assert!(result.is_ok(), "missing prompt file must not fail the request: {result:?}");
        assert_eq!(result.unwrap(), "AUGMENTED");
        assert_eq!(
            first_message_content(&mut rx).await,
            "\n\nHI",
            "the fallback sends an empty backend prompt joined to the user content by \
             two newlines, exactly as the baseline's empty-string fallback did"
        );
    }

    // --- task 48 / F-L7, F-L8: bounded error detail + configurable timeout ---

    #[derive(Clone)]
    struct CaptureWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureWriter {
        type Writer = Self;
        fn make_writer(&self) -> Self::Writer {
            self.clone()
        }
    }

    async fn spawn_status_augment_backend(status: u16, body: &'static str) -> String {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let response = format!(
                        "HTTP/1.1 {status} ERR\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(response.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        format!("http://{addr}")
    }

    fn backend_at(url: String) -> AugmentBackend {
        AugmentBackend {
            url,
            model: "fast".to_string(),
            prompt_file: String::new(),
            request_prompt_file: String::new(),
            http_client: reqwest::Client::new(),
        }
    }

    #[tokio::test]
    async fn task48_error_carries_status_and_first_300_chars_with_truncation_marker() {
        let body: &'static str = Box::leak(format!("{}{}", "X".repeat(300), "TAILMARK".repeat(50)).into_boxed_str());
        let backend = backend_at(spawn_status_augment_backend(500, body).await);

        let err = backend.get_augmentation("q").await.expect_err("500 must surface an error");

        let text = err.to_string();
        assert!(text.contains("500"), "status must be carried: {text}");
        assert!(text.contains(&"X".repeat(300)), "first 300 chars must be carried");
        assert!(text.contains("..."), "truncation marker must follow the clip");
        assert!(
            !text.contains("TAILMARK"),
            "char 301+ must NOT leak into the error (it reaches client 502 envelopes via handler.rs): {text}"
        );
    }

    #[tokio::test]
    async fn task48_cjk_error_clip_counts_chars_not_bytes() {
        let cjk_body: &'static str = Box::leak(format!("{}{}", "中".repeat(300), "禁".repeat(10)).into_boxed_str());
        let backend = backend_at(spawn_status_augment_backend(400, cjk_body).await);

        let err = backend.get_augmentation("q").await.expect_err("400 must surface an error");
        let text = err.to_string();

        assert!(
            text.contains(&"中".repeat(300)),
            "300 CJK chars (900 UTF-8 bytes) must ALL survive the clip - byte-based \
             clipping at 300 would keep only 100"
        );
        assert!(!text.contains("禁"), "char 301+ must not leak: {text}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn task48_full_error_body_reaches_debug_level_log() {
        let body: &'static str = Box::leak(format!("{}{}", "D".repeat(300), "FULLBODYMARKER-π-🚀").into_boxed_str());
        let backend = backend_at(spawn_status_augment_backend(503, body).await);

        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(CaptureWriter(buf.clone()))
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let err = backend.get_augmentation("q").await.expect_err("503 must surface an error");

        let captured = String::from_utf8_lossy(&buf.lock().unwrap().clone()).to_string();
        assert!(
            captured.contains(body),
            "the FULL body (clip tail included) must be visible at debug!; err was: {err}"
        );
    }

    #[tokio::test]
    async fn task48_from_config_honors_timeout_secs_against_stalled_backend() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            // The socket must stay OPEN and silent: dropping it would hand reqwest a
            // connection-closed error instantly and the timing assert would prove nothing.
            let _held = sock;
            std::future::pending::<()>().await;
        });

        let config = crate::config::AugmentBackendConfig {
            url: format!("http://{addr}"),
            model: "fast".to_string(),
            prompt_file: String::new(),
            request_prompt_file: String::new(),
            timeout_secs: 1,
            ..Default::default()
        };
        let backend = AugmentBackend::from_config(&config).unwrap();

        let started = std::time::Instant::now();
        let result = backend.get_augmentation("hello").await;
        let elapsed = started.elapsed();

        assert!(result.is_err(), "a stalled backend must surface an error, got {result:?}");
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "timeout_secs: 1 must abort in ~1s, not the old hard-coded 60s (took {elapsed:?})"
        );
    }
}
