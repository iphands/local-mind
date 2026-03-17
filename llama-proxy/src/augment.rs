//! Augment backend integration
//!
//! Enriches incoming user requests by calling a fast LLM backend
//! to generate additional context before forwarding to the main backend.

use crate::api::{ContentPart, Message, MessageContent};
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
        let backend_prompt = std::fs::read_to_string(&self.prompt_file)
            .unwrap_or_else(|e| {
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

        let response = self.http_client
            .post(&url)
            .json(&request)
            .send()
            .await?;

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
            if let Some(content) = first
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
            {
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
                if text.is_empty() { None } else { Some(text) }
            }
            None => None,
        })
        .collect()
}

/// Inject augmentation text into the last user message of an OpenAI ChatCompletionRequest.
///
/// The injected suffix is: "\n\n{request_prompt}\n\n{augmentation}"
pub fn inject_augmentation(
    mut request: crate::api::ChatCompletionRequest,
    request_prompt: &str,
    augmentation: &str,
) -> Result<crate::api::ChatCompletionRequest, Box<dyn std::error::Error + Send + Sync>> {
    let suffix = format!("\n\n{}\n\n{}", request_prompt, augmentation);

    // Find last user message index
    let last_user_idx = request.messages.iter().rposition(|m| m.role == "user");

    if let Some(idx) = last_user_idx {
        match &request.messages[idx].content {
            Some(MessageContent::Text(existing)) => {
                let new_content = format!("{}{}", existing, suffix);
                request.messages[idx].content = Some(MessageContent::Text(new_content));
            }
            Some(MessageContent::Parts(parts)) => {
                let mut parts = parts.clone();
                // Append to last text part, or add a new one
                if let Some(last_text) = parts.iter_mut().rev().find(|p| p.content_type == "text") {
                    if let Some(ref mut text) = last_text.text {
                        *text = format!("{}{}", text, suffix);
                    }
                } else {
                    parts.push(ContentPart {
                        content_type: "text".to_string(),
                        text: Some(suffix),
                        image_url: None,
                    });
                }
                request.messages[idx].content = Some(MessageContent::Parts(parts));
            }
            None => {
                request.messages[idx].content = Some(MessageContent::Text(suffix));
            }
        }
    } else if !request.messages.is_empty() {
        // Fallback: append to first message
        let suffix_owned = suffix;
        match &request.messages[0].content {
            Some(MessageContent::Text(existing)) => {
                let new_content = format!("{}{}", existing, suffix_owned);
                request.messages[0].content = Some(MessageContent::Text(new_content));
            }
            _ => {
                tracing::warn!("Could not find a user message to inject augmentation into");
            }
        }
    }

    Ok(request)
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
}
