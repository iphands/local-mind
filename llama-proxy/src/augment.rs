//! Augment backend integration
//!
//! Enriches incoming user requests by calling a fast LLM backend
//! to generate additional context before forwarding to the main backend.

use crate::api::{Message, MessageContent};
use crate::config::AugmentBackendConfig;

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
            .timeout(std::time::Duration::from_secs(60))
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
        // Load backend prompt (empty string on failure)
        let backend_prompt = std::fs::read_to_string(&self.prompt_file).unwrap_or_else(|e| {
            tracing::warn!(error = %e, file = %self.prompt_file, "Failed to load backend prompt, using empty string");
            String::new()
        });

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
            return Err(format!("Augment backend returned {}: {}", status, body).into());
        }

        let body: serde_json::Value = response.json().await?;

        // Extract text from response (supports OpenAI and Anthropic format)
        extract_response_text(&body)
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

    Err(format!("Could not extract text from augment backend response: {}", body).into())
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
///     `text`; if no part has `type: "text"`, a new
///     `{"type":"text","text":<block>}` part is pushed at the end; a text part
///     whose `text` is absent or `null` drops the block (the typed
///     implementation dropped it there too - preserved until task 45).
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
            if let Some(part) = parts
                .iter_mut()
                .rev()
                .find(|p| p.get("type").and_then(serde_json::Value::as_str) == Some("text"))
            {
                if let Some(serde_json::Value::String(text)) = part.get_mut("text") {
                    text.push_str(block);
                }
                // text part with absent/null `text`: the typed implementation
                // dropped the block here; preserved (task 45 territory).
            } else {
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
    fn value_injection_text_part_with_null_text_drops_block_as_typed_did() {
        let input = serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": [{"type": "text", "text": null}]}]
        });

        let output = inject(input.clone()).unwrap();

        assert_eq!(
            serde_json::to_vec(&output["messages"][0]).unwrap(),
            serde_json::to_vec(&input["messages"][0]).unwrap(),
            "typed semantics dropped the block on text:null parts; pinned until task 45"
        );
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
}
