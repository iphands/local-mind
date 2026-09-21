//! OpenAI-compatible API type definitions

use serde::{Deserialize, Deserializer, Serialize};

/// Deserialize `arguments` as either a JSON string or a JSON object.
/// The OpenAI spec requires a string, but some backends (e.g. llama.cpp) may
/// return a raw object. We normalize to a JSON string either way.
/// A JSON `null` normalizes to `"{}"` (strict clients reject `"null"` arguments).
fn deserialize_arguments<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Null => Ok("{}".to_string()),
        other => serde_json::to_string(&other).map_err(serde::de::Error::custom),
    }
}

fn default_empty_args() -> String {
    "{}".to_string()
}

/// Chat completion request
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    // Opencode request extensions (pass-through to backend)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<u64>,

    // Catch-all for unlisted params (top_k, min_p, seed, repetition_penalty, mirostat, etc.)
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

/// Chat message
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<MessageContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// Message content - can be string or array of content parts
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl Default for MessageContent {
    fn default() -> Self {
        MessageContent::Text(String::new())
    }
}

/// Content part for multimodal messages
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContentPart {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub image_url: Option<ImageUrl>,
}

/// Image URL content
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageUrl {
    pub url: String,
    #[serde(default)]
    pub detail: Option<String>,
}

/// Tool definition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDef,
}

/// Function definition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FunctionDef {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

/// Tool choice option
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ToolChoice {
    String(String),
    Object(ToolChoiceObject),
}

/// Tool choice object
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolChoiceObject {
    #[serde(rename = "type")]
    pub choice_type: String,
    pub function: Option<FunctionRef>,
}

/// Function reference
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FunctionRef {
    pub name: String,
}

/// Chat completion response
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatCompletionResponse {
    #[serde(default)]
    pub id: String,
    #[serde(default = "default_chat_completion_object")]
    pub object: String,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(default)]
    pub timings: Option<Timings>,
}

fn default_chat_completion_object() -> String {
    "chat.completion".to_string()
}

/// Response choice
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Choice {
    pub index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<ResponseMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<Delta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Response message
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResponseMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,

    // Opencode/Anthropic/Copilot reasoning extensions (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_opaque: Option<String>,
}

/// Tool call in response
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    pub function: FunctionCall,
}

/// Function call
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FunctionCall {
    pub name: String,
    #[serde(default = "default_empty_args", deserialize_with = "deserialize_arguments")]
    pub arguments: String,
}

/// Streaming delta
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Delta {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,

    // Reasoning extensions for streaming
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_opaque: Option<String>,
}

/// Token usage
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,

    // Extended usage details (Opencode/Copilot)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens_details: Option<CompletionTokensDetails>,
}

/// Extended completion token details
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompletionTokensDetails {
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    #[serde(default)]
    pub accepted_prediction_tokens: Option<u64>,
    #[serde(default)]
    pub rejected_prediction_tokens: Option<u64>,
}

/// Timing information from llama.cpp
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Timings {
    #[serde(default)]
    pub prompt_n: Option<u64>,
    #[serde(default)]
    pub prompt_ms: Option<f64>,
    #[serde(default)]
    pub prompt_per_token_ms: Option<f64>,
    #[serde(default)]
    pub prompt_per_second: Option<f64>,
    #[serde(default)]
    pub predicted_n: Option<u64>,
    #[serde(default)]
    pub predicted_ms: Option<f64>,
    #[serde(default)]
    pub predicted_per_token_ms: Option<f64>,
    #[serde(default)]
    pub predicted_per_second: Option<f64>,
    #[serde(default)]
    pub cache_n: Option<u64>,
}

/// Streaming chunk
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StreamChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<StreamChoice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// Streaming choice
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StreamChoice {
    pub index: u32,
    #[serde(default)]
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

// ============================================================================
// Anthropic Messages API Types
// ============================================================================

/// Anthropic Messages API request format
/// Used for both requests to and responses from Anthropic-compatible endpoints
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicMessageRequest {
    #[serde(default)]
    pub model: Option<String>,
    pub messages: Vec<AnthropicMessageContent>,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub max_tokens: u32,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub tools: Option<Vec<AnthropicTool>>,
    #[serde(default)]
    pub tool_choice: Option<serde_json::Value>,

    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

/// Anthropic message content (user/assistant/system messages)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicMessageContent {
    pub role: String,
    pub content: Vec<AnthropicContentBlock>,
}

/// Anthropic tool definition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicTool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

/// Anthropic Messages API response format
/// Used by llama.cpp when endpoint is /v1/messages
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String, // "message"
    pub role: String,
    #[serde(default)]
    pub content: Vec<AnthropicContentBlock>,
    pub model: String,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub stop_sequence: Option<String>,
    #[serde(default)]
    pub usage: AnthropicUsage,
}

/// Anthropic content block (text, thinking, tool_use, tool_result, or an
/// opaque `Other` block whose unknown `type` is preserved verbatim)
#[derive(Debug, Clone)]
pub enum AnthropicContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        signature: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
        is_error: Option<bool>,
    },
    /// An unknown `type` — or a known `type` whose field shape does not
    /// match — captured as the WHOLE raw object. Re-serialization emits it
    /// verbatim (forward-compat pass-through: the proxy never rejects or
    /// rewrites a block shape it does not understand).
    Other(serde_json::Value),
}

/// Wire helper for content-buffered deserialization: exactly the four known
/// internally-tagged block shapes. Private on purpose — unknown shapes never
/// need a variant here, they land in `AnthropicContentBlock::Other`.
#[derive(Deserialize)]
#[serde(tag = "type")]
enum AnthropicContentBlockWire {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
        #[serde(default)]
        is_error: Option<bool>,
    },
}

/// Wire helper for serialization: borrowed mirror of the four known shapes so
/// known variants emit byte-identical tagged objects to the old derived form
/// (explicit `"signature":null` / `"is_error":null` included), while `Other`
/// bypasses tagging entirely.
#[derive(Serialize)]
#[serde(tag = "type")]
enum AnthropicContentBlockWireRef<'a> {
    #[serde(rename = "text")]
    Text { text: &'a str },
    #[serde(rename = "thinking")]
    Thinking { thinking: &'a str, signature: Option<&'a str> },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: &'a str,
        name: &'a str,
        input: &'a serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: &'a str,
        content: &'a serde_json::Value,
        is_error: Option<bool>,
    },
}

impl<'de> Deserialize<'de> for AnthropicContentBlock {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Content-buffered: capture the raw object first, then try the known
        // tagged shapes. Anything that does not fit — unknown `type`, or a
        // known `type` with a missing/mismatched field — is preserved whole
        // as Other instead of failing the enclosing message parse.
        let raw = serde_json::Value::deserialize(deserializer)?;
        Ok(match serde_json::from_value::<AnthropicContentBlockWire>(raw.clone()) {
            Ok(AnthropicContentBlockWire::Text { text }) => Self::Text { text },
            Ok(AnthropicContentBlockWire::Thinking { thinking, signature }) => Self::Thinking { thinking, signature },
            Ok(AnthropicContentBlockWire::ToolUse { id, name, input }) => Self::ToolUse { id, name, input },
            Ok(AnthropicContentBlockWire::ToolResult {
                tool_use_id,
                content,
                is_error,
            }) => Self::ToolResult {
                tool_use_id,
                content,
                is_error,
            },
            Err(_) => Self::Other(raw),
        })
    }
}

impl Serialize for AnthropicContentBlock {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Text { text } => AnthropicContentBlockWireRef::Text { text }.serialize(serializer),
            Self::Thinking { thinking, signature } => AnthropicContentBlockWireRef::Thinking {
                thinking: thinking.as_str(),
                signature: signature.as_deref(),
            }
            .serialize(serializer),
            Self::ToolUse { id, name, input } => AnthropicContentBlockWireRef::ToolUse {
                id: id.as_str(),
                name: name.as_str(),
                input,
            }
            .serialize(serializer),
            Self::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => AnthropicContentBlockWireRef::ToolResult {
                tool_use_id: tool_use_id.as_str(),
                content,
                is_error: *is_error,
            }
            .serialize(serializer),
            // Unknown blocks: emit the original object verbatim — the `type`
            // value and every field survive exactly as they arrived.
            Self::Other(value) => value.serialize(serializer),
        }
    }
}

/// Anthropic usage (different field names than OpenAI)
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AnthropicUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

/// Convert Anthropic Message to OpenAI ChatCompletionResponse
/// This allows us to reuse synthesis logic for Anthropic format
impl From<AnthropicMessage> for ChatCompletionResponse {
    fn from(msg: AnthropicMessage) -> Self {
        // Convert content blocks to text and tool_calls
        let mut content_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut reasoning_parts = Vec::new();
        let mut reasoning_opaque: Option<String> = None;

        for block in &msg.content {
            match block {
                AnthropicContentBlock::Text { text } => {
                    content_parts.push(text.clone());
                }
                AnthropicContentBlock::Thinking { thinking, signature } => {
                    // A-M6: reasoning is not content - it lands in the
                    // reasoning fields of the OpenAI message instead.
                    reasoning_parts.push(thinking.clone());
                    if reasoning_opaque.is_none() {
                        reasoning_opaque = signature.clone();
                    }
                }
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    // Convert Anthropic tool_use to OpenAI tool_calls format
                    tool_calls.push(ToolCall {
                        id: Some(id.clone()),
                        call_type: Some("function".to_string()),
                        index: Some(tool_calls.len() as u32),
                        function: FunctionCall {
                            name: name.clone(),
                            arguments: serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string()),
                        },
                    });
                }
                AnthropicContentBlock::ToolResult { content, .. } => {
                    // A-M7: the String shape keeps the verbatim text; the
                    // array-of-parts shape joins its text parts (non-text
                    // parts skipped); any other shape contributes nothing.
                    match content {
                        serde_json::Value::String(s) => content_parts.push(s.clone()),
                        serde_json::Value::Array(parts) => {
                            let texts: Vec<&str> = parts
                                .iter()
                                .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some("text"))
                                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                                .collect();
                            if !texts.is_empty() {
                                content_parts.push(texts.join("\n"));
                            }
                        }
                        _ => {}
                    }
                }
                AnthropicContentBlock::Other(_) => {
                    // Opaque block: the OpenAI message has no carrier for it,
                    // so it contributes nothing to this conversion. Verbatim
                    // fidelity is preserved on the Anthropic-side round-trip;
                    // the enclosing parse no longer fails on it.
                }
            }
        }

        let content = if content_parts.is_empty() {
            None
        } else {
            Some(content_parts.join("\n"))
        };
        let reasoning_text = if reasoning_parts.is_empty() {
            None
        } else {
            Some(reasoning_parts.join("\n"))
        };

        let tool_calls_opt = if tool_calls.is_empty() { None } else { Some(tool_calls) };

        // Map Anthropic stop_reason to OpenAI finish_reason
        // "end_turn" -> "stop", "max_tokens" -> "length", "tool_use" -> "tool_calls", etc.
        let finish_reason = msg.stop_reason.as_ref().map(|reason| {
            match reason.as_str() {
                "end_turn" => "stop".to_string(),
                "max_tokens" => "length".to_string(),
                "stop_sequence" => "stop".to_string(),
                "tool_use" => "tool_calls".to_string(),
                other => other.to_string(), // Pass through unknown reasons
            }
        });

        ChatCompletionResponse {
            id: msg.id,
            object: "chat.completion".to_string(),
            created: 0, // Anthropic format doesn't include timestamp
            model: msg.model,
            choices: vec![Choice {
                index: 0,
                message: Some(ResponseMessage {
                    role: msg.role,
                    content,
                    tool_calls: tool_calls_opt,
                    reasoning_text,
                    reasoning_opaque,
                }),
                delta: None,
                finish_reason,
            }],
            usage: Some(Usage {
                prompt_tokens: msg.usage.input_tokens,
                completion_tokens: msg.usage.output_tokens,
                total_tokens: msg.usage.input_tokens + msg.usage.output_tokens,
                completion_tokens_details: None,
            }),
            timings: None, // Anthropic format doesn't include timings
        }
    }
}

/// Convert OpenAI ChatCompletionResponse to Anthropic Message format
/// This is needed when the backend (e.g., llama.cpp) returns OpenAI format
/// but the client expects Anthropic format (e.g., Claude CLI)
impl From<ChatCompletionResponse> for AnthropicMessage {
    fn from(resp: ChatCompletionResponse) -> Self {
        // Extract content from the first choice and convert to content blocks
        let content: Vec<AnthropicContentBlock> = resp
            .choices
            .first()
            .and_then(|c| c.message.as_ref())
            .map(|m| {
                let mut blocks = Vec::new();

                // Add reasoning as thinking block if present
                if let Some(reasoning) = &m.reasoning_text {
                    blocks.push(AnthropicContentBlock::Thinking {
                        thinking: reasoning.clone(),
                        signature: None,
                    });
                }

                // Add text content
                if let Some(text) = &m.content {
                    if !text.is_empty() {
                        blocks.push(AnthropicContentBlock::Text { text: text.clone() });
                    }
                }

                // Convert OpenAI tool_calls to Anthropic tool_use blocks
                if let Some(tool_calls) = &m.tool_calls {
                    for tool_call in tool_calls {
                        // Parse arguments as JSON value
                        let input = serde_json::from_str(&tool_call.function.arguments)
                            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                        blocks.push(AnthropicContentBlock::ToolUse {
                            id: tool_call
                                .id
                                .clone()
                                .unwrap_or_else(|| format!("toolu_{}", uuid::Uuid::new_v4().to_string().replace('-', ""))),
                            name: tool_call.function.name.clone(),
                            input,
                        });
                    }
                }

                // If no content at all, add empty text block
                if blocks.is_empty() {
                    blocks.push(AnthropicContentBlock::Text { text: String::new() });
                }

                blocks
            })
            .unwrap_or_else(|| vec![AnthropicContentBlock::Text { text: String::new() }]);

        // Convert usage from OpenAI format to Anthropic format
        let usage = resp
            .usage
            .as_ref()
            .map(|u| AnthropicUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            })
            .unwrap_or(AnthropicUsage {
                input_tokens: 0,
                output_tokens: 0,
            });

        // Map OpenAI finish_reason to Anthropic stop_reason
        let stop_reason = resp
            .choices
            .first()
            .and_then(|c| c.finish_reason.as_ref())
            .map(|r| match r.as_str() {
                "stop" => "end_turn".to_string(),
                "length" => "max_tokens".to_string(),
                "tool_calls" => "tool_use".to_string(),
                other => other.to_string(),
            });

        AnthropicMessage {
            id: resp.id,
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content,
            model: resp.model,
            stop_reason,
            stop_sequence: None,
            usage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_completion_request_serialize() {
        let request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: Some(MessageContent::Text("Hello".to_string())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: Some(0.7),
            top_p: None,
            max_tokens: Some(100),
            stream: Some(false),
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
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("gpt-4"));
        assert!(json.contains("Hello"));
    }

    #[test]
    fn test_chat_completion_request_deserialize() {
        let json = r#"{
            "model": "test-model",
            "messages": [{"role": "user", "content": "Hi"}]
        }"#;

        let request: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.model, "test-model");
        assert_eq!(request.messages.len(), 1);
        assert!(request.temperature.is_none());
    }

    #[test]
    fn test_message_content_default() {
        let content = MessageContent::default();
        assert!(matches!(content, MessageContent::Text(ref s) if s.is_empty()));
    }

    #[test]
    fn test_message_content_text() {
        let json = r#"{"role": "user", "content": "Hello"}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert!(matches!(msg.content, Some(MessageContent::Text(ref s)) if s == "Hello"));
    }

    #[test]
    fn test_message_content_parts() {
        let json = r#"{
            "role": "user",
            "content": [{"type": "text", "text": "Hello"}]
        }"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert!(matches!(msg.content, Some(MessageContent::Parts(_))));
    }

    #[test]
    fn test_content_part() {
        let part = ContentPart {
            content_type: "text".to_string(),
            text: Some("Hello".to_string()),
            image_url: None,
        };

        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("text"));
        assert!(json.contains("Hello"));
    }

    #[test]
    fn test_content_part_image() {
        let json = r#"{
            "type": "image_url",
            "image_url": {"url": "http://example.com/image.png", "detail": "high"}
        }"#;

        let part: ContentPart = serde_json::from_str(json).unwrap();
        assert_eq!(part.content_type, "image_url");
        assert!(part.image_url.is_some());
        assert_eq!(part.image_url.unwrap().detail, Some("high".to_string()));
    }

    #[test]
    fn test_tool_definition() {
        let tool = Tool {
            tool_type: "function".to_string(),
            function: FunctionDef {
                name: "get_weather".to_string(),
                description: Some("Get weather info".to_string()),
                parameters: serde_json::json!({"type": "object"}),
            },
        };

        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("get_weather"));
    }

    #[test]
    fn test_tool_choice_string() {
        let choice = ToolChoice::String("auto".to_string());
        let json = serde_json::to_string(&choice).unwrap();
        assert_eq!(json, "\"auto\"");
    }

    #[test]
    fn test_tool_choice_object() {
        let choice = ToolChoice::Object(ToolChoiceObject {
            choice_type: "function".to_string(),
            function: Some(FunctionRef {
                name: "get_weather".to_string(),
            }),
        });

        let json = serde_json::to_string(&choice).unwrap();
        assert!(json.contains("get_weather"));
    }

    #[test]
    fn test_chat_completion_response() {
        let json = r#"{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        }"#;

        let response: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.id, "chatcmpl-123");
        assert_eq!(response.model, "gpt-4");
        assert_eq!(response.choices.len(), 1);
        assert!(response.usage.is_some());
    }

    #[test]
    fn test_response_with_tool_calls() {
        let json = r#"{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call-123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\": \"Paris\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        }"#;

        let response: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        let tool_calls = response.choices[0].message.as_ref().unwrap().tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "get_weather");
    }

    #[test]
    fn test_timings() {
        let json = r#"{
            "prompt_n": 100,
            "prompt_ms": 50.5,
            "prompt_per_second": 1980.2,
            "predicted_n": 50,
            "predicted_ms": 100.0,
            "predicted_per_second": 500.0,
            "cache_n": 10
        }"#;

        let timings: Timings = serde_json::from_str(json).unwrap();
        assert_eq!(timings.prompt_n, Some(100));
        assert_eq!(timings.prompt_ms, Some(50.5));
        assert_eq!(timings.cache_n, Some(10));
    }

    #[test]
    fn test_stream_chunk() {
        let json = r#"{
            "id": "chatcmpl-123",
            "object": "chat.completion.chunk",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "delta": {"content": "Hello"},
                "finish_reason": null
            }]
        }"#;

        let chunk: StreamChunk = serde_json::from_str(json).unwrap();
        assert_eq!(chunk.object, "chat.completion.chunk");
        assert_eq!(chunk.choices[0].delta.content, Some("Hello".to_string()));
    }

    #[test]
    fn test_stream_chunk_with_usage() {
        let json = r#"{
            "id": "chatcmpl-123",
            "object": "chat.completion.chunk",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        }"#;

        let chunk: StreamChunk = serde_json::from_str(json).unwrap();
        assert!(chunk.usage.is_some());
        assert_eq!(chunk.usage.unwrap().total_tokens, 15);
    }

    #[test]
    fn test_usage() {
        let usage = Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            completion_tokens_details: None,
        };

        let json = serde_json::to_string(&usage).unwrap();
        let parsed: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.prompt_tokens, 100);
        assert_eq!(parsed.completion_tokens, 50);
        assert_eq!(parsed.total_tokens, 150);
    }

    #[test]
    fn test_delta() {
        let delta = Delta {
            role: Some("assistant".to_string()),
            content: Some("Hello".to_string()),
            tool_calls: None,
            reasoning_text: None,
            reasoning_opaque: None,
        };

        let json = serde_json::to_string(&delta).unwrap();
        assert!(json.contains("assistant"));
        assert!(json.contains("Hello"));
    }

    #[test]
    fn test_function_call() {
        let call = FunctionCall {
            name: "get_weather".to_string(),
            arguments: r#"{"location": "Paris"}"#.to_string(),
        };

        let json = serde_json::to_string(&call).unwrap();
        assert!(json.contains("get_weather"));
        assert!(json.contains("Paris"));
    }

    #[test]
    fn test_tool_call() {
        let tool_call = ToolCall {
            id: Some("call-123".to_string()),
            call_type: Some("function".to_string()),
            index: Some(0),
            function: FunctionCall {
                name: "test".to_string(),
                arguments: "{}".to_string(),
            },
        };

        let json = serde_json::to_string(&tool_call).unwrap();
        let parsed: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, Some("call-123".to_string()));
        assert_eq!(parsed.index, Some(0));
    }

    #[test]
    fn test_response_message_with_reasoning() {
        let json = r#"{
            "role": "assistant",
            "content": "Answer",
            "reasoning_text": "Thinking steps",
            "reasoning_opaque": "state_blob"
        }"#;
        let msg: ResponseMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.reasoning_text, Some("Thinking steps".to_string()));
        assert_eq!(msg.reasoning_opaque, Some("state_blob".to_string()));
    }

    #[test]
    fn test_response_message_without_reasoning() {
        let json = r#"{
            "role": "assistant",
            "content": "Answer"
        }"#;
        let msg: ResponseMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.reasoning_text, None);
        assert_eq!(msg.reasoning_opaque, None);
    }

    #[test]
    fn test_usage_with_extended_details() {
        let json = r#"{
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150,
            "completion_tokens_details": {
                "reasoning_tokens": 20
            }
        }"#;
        let usage: Usage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.completion_tokens_details.unwrap().reasoning_tokens, Some(20));
    }

    #[test]
    fn test_request_with_reasoning_effort() {
        let json = r#"{
            "model": "test",
            "messages": [{"role": "user", "content": "Test"}],
            "reasoning_effort": "high"
        }"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.reasoning_effort, Some("high".to_string()));
    }

    #[test]
    fn test_delta_with_reasoning() {
        let json = r#"{
            "role": "assistant",
            "content": "Text",
            "reasoning_text": "Thinking"
        }"#;
        let delta: Delta = serde_json::from_str(json).unwrap();
        assert_eq!(delta.reasoning_text, Some("Thinking".to_string()));
    }

    // ============================================================================
    // Anthropic Messages API Tests
    // ============================================================================

    #[test]
    fn test_parse_anthropic_message() {
        let json = serde_json::json!({
            "id": "msg-123",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello!"}
            ],
            "model": "test-model",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        });

        let msg: AnthropicMessage = serde_json::from_value(json).unwrap();
        assert_eq!(msg.id, "msg-123");
        assert_eq!(msg.message_type, "message");
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.usage.input_tokens, 10);
        assert_eq!(msg.usage.output_tokens, 5);
        assert_eq!(msg.content.len(), 1);
    }

    #[test]
    fn test_parse_anthropic_message_with_thinking() {
        let json = serde_json::json!({
            "id": "msg-456",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "Let me think..."},
                {"type": "text", "text": "Answer"}
            ],
            "model": "test-model",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 20,
                "output_tokens": 10
            }
        });

        let msg: AnthropicMessage = serde_json::from_value(json).unwrap();
        assert_eq!(msg.content.len(), 2);
        match &msg.content[0] {
            AnthropicContentBlock::Thinking { thinking, .. } => {
                assert_eq!(thinking, "Let me think...");
            }
            _ => panic!("Expected thinking block"),
        }
        match &msg.content[1] {
            AnthropicContentBlock::Text { text } => {
                assert_eq!(text, "Answer");
            }
            _ => panic!("Expected text block"),
        }
    }

    #[test]
    fn test_anthropic_to_openai_conversion() {
        let anthropic_msg = AnthropicMessage {
            id: "msg-123".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock::Text {
                text: "Hello!".to_string(),
            }],
            model: "test-model".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 5,
            },
        };

        let openai_response: ChatCompletionResponse = anthropic_msg.into();
        assert_eq!(openai_response.id, "msg-123");
        assert_eq!(openai_response.model, "test-model");
        assert_eq!(openai_response.object, "chat.completion");
        assert_eq!(openai_response.choices.len(), 1);
        assert_eq!(
            openai_response.choices[0].message.as_ref().unwrap().content,
            Some("Hello!".to_string())
        );
        assert_eq!(openai_response.choices[0].finish_reason, Some("stop".to_string()));

        let usage = openai_response.usage.as_ref().unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn test_anthropic_to_openai_thinking_converts_to_reasoning_text() {
        // Task 40 rule 1 (A-M6): a thinking block converts to reasoning_text,
        // NOT merged into content.
        // RED baseline (raw, probe at 92ab176): content=Some("THINK\nANS")
        // reasoning_text=None reasoning_opaque=None (thinking landed in content).
        let anthropic_msg = AnthropicMessage {
            id: "msg-456".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![
                AnthropicContentBlock::Thinking {
                    thinking: "Let me think...".to_string(),
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
                input_tokens: 20,
                output_tokens: 10,
            },
        };

        let openai_response: ChatCompletionResponse = anthropic_msg.into();
        let message = openai_response.choices[0].message.as_ref().unwrap();
        assert_eq!(message.content, Some("Answer".to_string()));
        assert_eq!(message.reasoning_text, Some("Let me think...".to_string()));
        assert_eq!(message.reasoning_opaque, None);
    }

    #[test]
    fn test_anthropic_stop_reason_mapping() {
        let test_cases = vec![("end_turn", "stop"), ("max_tokens", "length"), ("stop_sequence", "stop")];

        for (anthropic_reason, expected_openai) in test_cases {
            let anthropic_msg = AnthropicMessage {
                id: "msg-test".to_string(),
                message_type: "message".to_string(),
                role: "assistant".to_string(),
                content: vec![AnthropicContentBlock::Text {
                    text: "Test".to_string(),
                }],
                model: "test-model".to_string(),
                stop_reason: Some(anthropic_reason.to_string()),
                stop_sequence: None,
                usage: AnthropicUsage {
                    input_tokens: 5,
                    output_tokens: 5,
                },
            };

            let openai_response: ChatCompletionResponse = anthropic_msg.into();
            assert_eq!(
                openai_response.choices[0].finish_reason,
                Some(expected_openai.to_string()),
                "Failed for anthropic reason: {}",
                anthropic_reason
            );
        }
    }

    #[test]
    fn test_real_anthropic_response_from_llama_server() {
        // This is an actual response format from llama.cpp server on /v1/messages endpoint
        let json = serde_json::json!({
            "id": "chatcmpl-5678",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello! How can I help you today?"}
            ],
            "model": "ERNIE-4.5",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 158,
                "output_tokens": 265
            }
        });

        // Parse as Anthropic format
        let anthropic_msg: AnthropicMessage = serde_json::from_value(json).unwrap();
        assert_eq!(anthropic_msg.usage.input_tokens, 158);
        assert_eq!(anthropic_msg.usage.output_tokens, 265);

        // Convert to OpenAI format (this is what the proxy does)
        let openai_response: ChatCompletionResponse = anthropic_msg.into();

        // Verify conversion preserves critical fields
        assert_eq!(openai_response.usage.as_ref().unwrap().prompt_tokens, 158);
        assert_eq!(openai_response.usage.as_ref().unwrap().completion_tokens, 265);
        assert_eq!(openai_response.usage.as_ref().unwrap().total_tokens, 423);
        assert_eq!(openai_response.choices[0].finish_reason, Some("stop".to_string()));
        assert_eq!(
            openai_response.choices[0].message.as_ref().unwrap().content,
            Some("Hello! How can I help you today?".to_string())
        );
    }

    #[test]
    fn test_openai_to_anthropic_conversion() {
        // Test OpenAI → Anthropic conversion (for backends that return OpenAI format)
        let openai_response = ChatCompletionResponse {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion".to_string(),
            created: 1234567890,
            model: "test-model".to_string(),
            choices: vec![Choice {
                index: 0,
                message: Some(ResponseMessage {
                    role: "assistant".to_string(),
                    content: Some("Hello from OpenAI!".to_string()),
                    tool_calls: None,
                    reasoning_text: None,
                    reasoning_opaque: None,
                }),
                delta: None,
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
                completion_tokens_details: None,
            }),
            timings: None,
        };

        let anthropic: AnthropicMessage = openai_response.into();
        assert_eq!(anthropic.id, "chatcmpl-123");
        assert_eq!(anthropic.message_type, "message");
        assert_eq!(anthropic.role, "assistant");
        assert_eq!(anthropic.model, "test-model");
        assert_eq!(anthropic.stop_reason, Some("end_turn".to_string())); // stop -> end_turn
        assert_eq!(anthropic.usage.input_tokens, 10); // was prompt_tokens
        assert_eq!(anthropic.usage.output_tokens, 5); // was completion_tokens

        // Check content block
        assert_eq!(anthropic.content.len(), 1);
        match &anthropic.content[0] {
            AnthropicContentBlock::Text { text } => {
                assert_eq!(text, "Hello from OpenAI!");
            }
            _ => panic!("Expected text block"),
        }
    }

    #[test]
    fn test_openai_to_anthropic_with_reasoning() {
        // Test OpenAI → Anthropic conversion with reasoning_text
        let openai_response = ChatCompletionResponse {
            id: "chatcmpl-456".to_string(),
            object: "chat.completion".to_string(),
            created: 1234567890,
            model: "test-model".to_string(),
            choices: vec![Choice {
                index: 0,
                message: Some(ResponseMessage {
                    role: "assistant".to_string(),
                    content: Some("The answer is 42.".to_string()),
                    tool_calls: None,
                    reasoning_text: Some("Let me think about this...".to_string()),
                    reasoning_opaque: None,
                }),
                delta: None,
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(Usage {
                prompt_tokens: 20,
                completion_tokens: 10,
                total_tokens: 30,
                completion_tokens_details: None,
            }),
            timings: None,
        };

        let anthropic: AnthropicMessage = openai_response.into();

        // Should have two content blocks: thinking + text
        assert_eq!(anthropic.content.len(), 2);

        match &anthropic.content[0] {
            AnthropicContentBlock::Thinking { thinking, .. } => {
                assert_eq!(thinking, "Let me think about this...");
            }
            _ => panic!("Expected thinking block first"),
        }

        match &anthropic.content[1] {
            AnthropicContentBlock::Text { text } => {
                assert_eq!(text, "The answer is 42.");
            }
            _ => panic!("Expected text block second"),
        }
    }

    #[test]
    fn test_openai_to_anthropic_finish_reason_mapping() {
        let test_cases = vec![("stop", "end_turn"), ("length", "max_tokens"), ("tool_calls", "tool_use")];

        for (openai_reason, expected_anthropic) in test_cases {
            let openai_response = ChatCompletionResponse {
                id: "chatcmpl-test".to_string(),
                object: "chat.completion".to_string(),
                created: 0,
                model: "test".to_string(),
                choices: vec![Choice {
                    index: 0,
                    message: Some(ResponseMessage {
                        role: "assistant".to_string(),
                        content: Some("Test".to_string()),
                        tool_calls: None,
                        reasoning_text: None,
                        reasoning_opaque: None,
                    }),
                    delta: None,
                    finish_reason: Some(openai_reason.to_string()),
                }],
                usage: None,
                timings: None,
            };

            let anthropic: AnthropicMessage = openai_response.into();
            assert_eq!(
                anthropic.stop_reason,
                Some(expected_anthropic.to_string()),
                "Failed for OpenAI reason: {}",
                openai_reason
            );
        }
    }

    #[test]
    fn test_openai_to_anthropic_from_llama_cpp() {
        // This is what llama.cpp actually returns (OpenAI format)
        let json = serde_json::json!({
            "id": "chatcmpl-abc123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "qwen3-coder",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Here's the code you requested."
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 150,
                "completion_tokens": 75,
                "total_tokens": 225
            }
        });

        // Parse as OpenAI format
        let openai_response: ChatCompletionResponse = serde_json::from_value(json).unwrap();

        // Convert to Anthropic format
        let anthropic: AnthropicMessage = openai_response.into();

        // Verify conversion
        assert_eq!(anthropic.id, "chatcmpl-abc123");
        assert_eq!(anthropic.message_type, "message");
        assert_eq!(anthropic.model, "qwen3-coder");
        assert_eq!(anthropic.stop_reason, Some("end_turn".to_string()));
        assert_eq!(anthropic.usage.input_tokens, 150);
        assert_eq!(anthropic.usage.output_tokens, 75);

        // Content should be preserved
        match &anthropic.content[0] {
            AnthropicContentBlock::Text { text } => {
                assert_eq!(text, "Here's the code you requested.");
            }
            _ => panic!("Expected text block"),
        }
    }

    #[test]
    fn test_parse_anthropic_message_with_tool_use() {
        // Test parsing Anthropic response with tool_use content block
        let json = serde_json::json!({
            "id": "msg-tool-123",
            "type": "message",
            "role": "assistant",
            "content": [
                {
                    "type": "tool_use",
                    "id": "toolu_abc123",
                    "name": "get_weather",
                    "input": {
                        "location": "Paris",
                        "unit": "celsius"
                    }
                }
            ],
            "model": "test-model",
            "stop_reason": "tool_use",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50
            }
        });

        let msg: AnthropicMessage = serde_json::from_value(json).unwrap();
        assert_eq!(msg.id, "msg-tool-123");
        assert_eq!(msg.stop_reason, Some("tool_use".to_string()));
        assert_eq!(msg.content.len(), 1);

        match &msg.content[0] {
            AnthropicContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_abc123");
                assert_eq!(name, "get_weather");
                assert_eq!(input["location"], "Paris");
                assert_eq!(input["unit"], "celsius");
            }
            _ => panic!("Expected tool_use block"),
        }
    }

    #[test]
    fn test_anthropic_tool_use_to_openai_conversion() {
        // Test conversion of Anthropic tool_use to OpenAI tool_calls
        let anthropic_msg = AnthropicMessage {
            id: "msg-tool-456".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock::ToolUse {
                id: "toolu_xyz789".to_string(),
                name: "calculate".to_string(),
                input: serde_json::json!({"x": 5, "y": 3, "operation": "add"}),
            }],
            model: "test-model".to_string(),
            stop_reason: Some("tool_use".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 50,
                output_tokens: 25,
            },
        };

        let openai_response: ChatCompletionResponse = anthropic_msg.into();
        assert_eq!(openai_response.id, "msg-tool-456");
        assert_eq!(openai_response.choices[0].finish_reason, Some("tool_calls".to_string()));

        let tool_calls = openai_response.choices[0]
            .message
            .as_ref()
            .unwrap()
            .tool_calls
            .as_ref()
            .unwrap();

        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, Some("toolu_xyz789".to_string()));
        assert_eq!(tool_calls[0].function.name, "calculate");

        let args: serde_json::Value = serde_json::from_str(&tool_calls[0].function.arguments).unwrap();
        assert_eq!(args["x"], 5);
        assert_eq!(args["y"], 3);
        assert_eq!(args["operation"], "add");
    }

    #[test]
    fn test_openai_tool_calls_to_anthropic_conversion() {
        // Test conversion of OpenAI tool_calls to Anthropic tool_use
        let openai_response = ChatCompletionResponse {
            id: "chatcmpl-tool-789".to_string(),
            object: "chat.completion".to_string(),
            created: 1234567890,
            model: "test-model".to_string(),
            choices: vec![Choice {
                index: 0,
                message: Some(ResponseMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: Some("call_abc123".to_string()),
                        call_type: Some("function".to_string()),
                        index: Some(0),
                        function: FunctionCall {
                            name: "search".to_string(),
                            arguments: r#"{"query":"weather","limit":5}"#.to_string(),
                        },
                    }]),
                    reasoning_text: None,
                    reasoning_opaque: None,
                }),
                delta: None,
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: Some(Usage {
                prompt_tokens: 75,
                completion_tokens: 35,
                total_tokens: 110,
                completion_tokens_details: None,
            }),
            timings: None,
        };

        let anthropic: AnthropicMessage = openai_response.into();
        assert_eq!(anthropic.id, "chatcmpl-tool-789");
        assert_eq!(anthropic.stop_reason, Some("tool_use".to_string()));
        assert_eq!(anthropic.content.len(), 1);

        match &anthropic.content[0] {
            AnthropicContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_abc123");
                assert_eq!(name, "search");
                assert_eq!(input["query"], "weather");
                assert_eq!(input["limit"], 5);
            }
            _ => panic!("Expected tool_use block"),
        }
    }

    #[test]
    fn test_anthropic_mixed_content_with_tool_use() {
        // Test message with both text and tool_use blocks
        let json = serde_json::json!({
            "id": "msg-mixed-123",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Let me check the weather for you."},
                {
                    "type": "tool_use",
                    "id": "toolu_weather_1",
                    "name": "get_weather",
                    "input": {"location": "London"}
                }
            ],
            "model": "test-model",
            "stop_reason": "tool_use",
            "usage": {
                "input_tokens": 80,
                "output_tokens": 40
            }
        });

        let msg: AnthropicMessage = serde_json::from_value(json).unwrap();
        assert_eq!(msg.content.len(), 2);

        match &msg.content[0] {
            AnthropicContentBlock::Text { text } => {
                assert_eq!(text, "Let me check the weather for you.");
            }
            _ => panic!("Expected text block first"),
        }

        match &msg.content[1] {
            AnthropicContentBlock::ToolUse { id, name, .. } => {
                assert_eq!(id, "toolu_weather_1");
                assert_eq!(name, "get_weather");
            }
            _ => panic!("Expected tool_use block second"),
        }

        // Convert to OpenAI and verify both content and tool_calls are present
        let openai_response: ChatCompletionResponse = msg.into();
        let message = openai_response.choices[0].message.as_ref().unwrap();

        assert_eq!(message.content, Some("Let me check the weather for you.".to_string()));
        assert!(message.tool_calls.is_some());
        assert_eq!(message.tool_calls.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_parse_anthropic_message_with_tool_result() {
        // Test parsing tool_result content block
        let json = serde_json::json!({
            "id": "msg-result-123",
            "type": "message",
            "role": "user",
            "content": [
                {
                    "type": "tool_result",
                    "tool_use_id": "toolu_abc123",
                    "content": "The weather in Paris is 22°C and sunny."
                }
            ],
            "model": "test-model",
            "usage": {
                "input_tokens": 50,
                "output_tokens": 0
            }
        });

        let msg: AnthropicMessage = serde_json::from_value(json).unwrap();
        assert_eq!(msg.content.len(), 1);

        match &msg.content[0] {
            AnthropicContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "toolu_abc123");
                assert_eq!(content.as_str(), Some("The weather in Paris is 22°C and sunny."));
                assert_eq!(*is_error, None);
            }
            _ => panic!("Expected tool_result block"),
        }
    }

    #[test]
    fn test_request_round_trip_emits_no_null_keys() {
        // Given: a minimal request with only model + messages (one message with
        // content, one assistant message without), parsed then re-serialized.
        let json = r#"{"model":"m","messages":[{"role":"user","content":"Hi"},{"role":"assistant"}]}"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        let out = serde_json::to_string(&req).unwrap();

        // Then: every Option-bearing field of ChatCompletionRequest and Message is
        // omitted when None — no `"key":null` may survive the round trip.
        let mut offenders: Vec<&str> = Vec::new();
        for key in [
            "temperature",
            "top_p",
            "max_tokens",
            "stream",
            "tools",
            "tool_choice",
            "stop",
            "frequency_penalty",
            "presence_penalty",
            "user",
            "reasoning_effort",
            "verbosity",
            "thinking_budget",
            "name",
            "tool_calls",
            "tool_call_id",
        ] {
            if out.contains(&format!("\"{key}\":null")) {
                offenders.push(key);
            }
        }
        // Message.content: None (assistant message) must not serialize as null.
        // Count null occurrences: the user message legitimately carries "content":"Hi",
        // so a null content can only come from the None assistant message.
        if out.contains("\"content\":null") {
            offenders.push("content");
        }
        assert!(
            offenders.is_empty(),
            "round-trip emitted null keys for Option fields: {offenders:?}\nserialized: {out}"
        );
        // Positive pins: mandatory data survives.
        assert!(out.contains("\"model\":\"m\""));
        assert!(out.contains("\"content\":\"Hi\""));
    }

    #[test]
    fn test_message_content_string_and_array_round_trip_unchanged() {
        // String content round-trips unchanged.
        let json = r#"{"model":"m","messages":[{"role":"user","content":"Hi"}]}"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        let out = serde_json::to_string(&req).unwrap();
        assert!(out.contains(r#""content":"Hi""#), "{out}");

        // Array (content parts) content round-trips unchanged in shape:
        // still an array with the part's type/text intact. (ContentPart's own
        // Option fields are out of this task's scope, so no byte-exact pin here.)
        let json = r#"{"model":"m","messages":[{"role":"user","content":[{"type":"text","text":"Hi"}]}]}"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        let out = serde_json::to_string(&req).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let content = &parsed["messages"][0]["content"];
        assert!(content.is_array(), "content must stay an array: {out}");
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Hi");
    }

    #[test]
    fn test_function_call_null_arguments_deserializes_to_empty_object() {
        let call: FunctionCall = serde_json::from_str(r#"{"name":"f","arguments":null}"#).unwrap();
        assert_eq!(call.arguments, "{}");
    }

    #[test]
    fn test_function_call_null_arguments_round_trips_as_empty_object() {
        let call: FunctionCall = serde_json::from_str(r#"{"name":"f","arguments":null}"#).unwrap();
        let out = serde_json::to_string(&call).unwrap();
        assert!(out.contains(r#""arguments":"{}""#), "{out}");
        assert!(!out.contains(r#""arguments":null"#), "{out}");
        assert!(!out.contains(r#""arguments":"null""#), "{out}");
    }

    #[test]
    fn test_function_call_absent_arguments_defaults_to_empty_object() {
        let call: FunctionCall = serde_json::from_str(r#"{"name":"f"}"#).unwrap();
        assert_eq!(call.arguments, "{}");
    }

    #[test]
    fn test_function_call_arguments_string_variant_round_trips_unchanged() {
        let json = r#"{"name":"f","arguments":"{\"a\":1}"}"#;
        let call: FunctionCall = serde_json::from_str(json).unwrap();
        assert_eq!(call.arguments, r#"{"a":1}"#);
        let out = serde_json::to_string(&call).unwrap();
        assert_eq!(out, json);
    }

    #[test]
    fn test_function_call_arguments_object_variant_normalizes_to_string() {
        let call: FunctionCall = serde_json::from_str(r#"{"name":"f","arguments":{}}"#).unwrap();
        assert_eq!(call.arguments, "{}");
    }

    #[test]
    fn test_tool_call_all_none_serializes_no_null_keys() {
        let call = ToolCall {
            id: None,
            call_type: None,
            index: None,
            function: FunctionCall {
                name: "f".to_string(),
                arguments: "{}".to_string(),
            },
        };
        let out = serde_json::to_string(&call).unwrap();
        for key in ["id", "type", "index"] {
            assert!(
                !out.contains(&format!("\"{key}\":null")),
                "ToolCall serialization leaked null key {key}: {out}"
            );
        }
    }

    #[test]
    fn test_delta_tool_calls_serialization_leaks_no_null_keys() {
        // Synthesis embeds the Vec<ToolCall> verbatim into delta.tool_calls
        // (src/proxy/synthesis.rs). This serialization-level fixture models
        // that payload: null arguments and absent id/type/index.
        let json = r#"{"tool_calls":[{"function":{"name":"f","arguments":null}}]}"#;
        let delta: Delta = serde_json::from_str(json).unwrap();
        let out = serde_json::to_string(&delta).unwrap();
        assert!(!out.contains(r#""arguments":null"#), "{out}");
        assert!(!out.contains(r#""arguments":"null""#), "{out}");
        for key in ["id", "type", "index"] {
            assert!(
                !out.contains(&format!("\"{key}\":null")),
                "delta.tool_calls serialization leaked null key {key}: {out}"
            );
        }
        assert!(out.contains(r#""arguments":"{}""#), "{out}");
    }

    // ============================================================================
    // Task 37: response leniency (report A M3/M4) - minimal {} parses,
    // Choice null-key hygiene on serialization
    // ============================================================================

    #[test]
    fn test_chat_completion_response_minimal_empty_object_parses() {
        // Given: a bare {} body (A-M4: every currently-required field must be defaulted)
        // When: parsed as ChatCompletionResponse
        // Then: it parses with zeroed strings/numbers, empty choices, and the
        //       "chat.completion" object default
        let resp: ChatCompletionResponse = serde_json::from_str("{}").unwrap();
        assert_eq!(resp.id, "");
        assert_eq!(resp.created, 0);
        assert_eq!(resp.model, "");
        assert!(resp.choices.is_empty());
        assert_eq!(resp.object, "chat.completion");
        assert!(resp.usage.is_none());
        assert!(resp.timings.is_none());

        // And re-serializing carries the defaulted object tag and no null leak
        let out = serde_json::to_string(&resp).unwrap();
        assert!(out.contains(r#""object":"chat.completion""#), "{out}");
    }

    #[test]
    fn test_chat_completion_response_object_default_when_fields_absent() {
        // Given: a response missing every field except object (object is the
        // ONLY field provided, so every other default must be exercised)
        // Then: all-required-fields-default path parses; id/created/model/choices defaulted
        let resp: ChatCompletionResponse = serde_json::from_str(r#"{"object":"chat.completion"}"#).unwrap();
        assert_eq!(resp.id, "");
        assert_eq!(resp.created, 0);
        assert_eq!(resp.model, "");
        assert!(resp.choices.is_empty());
        assert_eq!(resp.object, "chat.completion");
    }

    #[test]
    fn test_choice_absent_fields_serialize_no_null_keys() {
        // Given: a choice carrying neither message, delta, nor finish_reason
        // When: the whole response is serialized
        // Then: NO message/delta/finish_reason key appears at all (A-M3:
        // skip_serializing_if on all three Options, not "key":null)
        let json = r#"{"id":"x","object":"chat.completion","created":0,"model":"m","choices":[{"index":0}]}"#;
        let resp: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        let choice = &resp.choices[0];
        assert!(choice.message.is_none());
        assert!(choice.delta.is_none());
        assert!(choice.finish_reason.is_none());

        let out = serde_json::to_string(&resp).unwrap();
        for key in ["message", "delta", "finish_reason"] {
            assert!(
                !out.contains(&format!("\"{key}\"")),
                "Choice serialization leaked key {key}: {out}"
            );
        }
    }

    #[test]
    fn test_choice_explicit_nulls_stay_none_and_stay_unserialized() {
        // Given: explicit JSON nulls for message/delta/finish_reason
        // Then (deser): default + Option must agree - fields land as None
        // Then (ser): the defaulted None must NOT resurrect as a serialized null
        let json = r#"{"index":0,"message":null,"delta":null,"finish_reason":null}"#;
        let choice: Choice = serde_json::from_str(json).unwrap();
        assert!(choice.message.is_none());
        assert!(choice.delta.is_none());
        assert!(choice.finish_reason.is_none());

        let out = serde_json::to_string(&choice).unwrap();
        for key in ["message", "delta", "finish_reason"] {
            assert!(
                !out.contains(&format!("\"{key}\"")),
                "null-defaulted Choice key {key} resurrected in serialization: {out}"
            );
        }
        assert!(out.contains(r#""index":0"#), "{out}");
    }

    #[test]
    fn test_realistic_response_round_trips_losslessly() {
        // Given: a realistic llama.cpp-style response (message + finish_reason +
        // usage + timings, CJK model name and content)
        // When: parse -> serialize -> parse again
        // Then: semantic equality - no field lost, CJK survives
        let raw = serde_json::json!({
            "id": "chatcmpl-abc123",
            "object": "chat.completion",
            "created": 1712345678,
            "model": "qwen3-coder-30b-中文",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "你好, world",
                    "reasoning_text": "thinking…"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15,
                "completion_tokens_details": { "reasoning_tokens": 2 }
            },
            "timings": { "predicted_per_second": 33.13 }
        });

        let first: ChatCompletionResponse = serde_json::from_value(raw.clone()).unwrap();
        let wire = serde_json::to_string(&first).unwrap();
        let second: ChatCompletionResponse = serde_json::from_str(&wire).unwrap();

        assert_eq!(second.model, "qwen3-coder-30b-中文");
        assert_eq!(
            second.choices[0].message.as_ref().unwrap().content,
            Some("你好, world".to_string())
        );
        assert_eq!(second.choices[0].finish_reason, Some("stop".to_string()));
        assert_eq!(second.usage.as_ref().unwrap().completion_tokens, 5);
        assert_eq!(
            second
                .usage
                .as_ref()
                .unwrap()
                .completion_tokens_details
                .as_ref()
                .unwrap()
                .reasoning_tokens,
            Some(2)
        );
        assert_eq!(second.timings.as_ref().unwrap().predicted_per_second, Some(33.13));

        // Semantic equality: re-serialized bytes are value-identical
        assert_eq!(serde_json::to_value(&first).unwrap(), serde_json::to_value(&second).unwrap());
        // And the original keys/values survive as far as the struct models them
        let back = serde_json::to_value(&second).unwrap();
        assert_eq!(back["model"], "qwen3-coder-30b-中文");
        assert_eq!(back["created"], 1712345678);
    }

    #[test]
    fn test_choices_array_null_element_still_rejected() {
        // Pinned shipped behavior: field-level #[serde(default)] defaults MISSING
        // fields, it does not accept a `null` ELEMENT inside Vec<Choice>.
        // {"choices":[null]} errors ("invalid type: null") - a null element is a
        // malformed payload, not a missing field; no leniency is shipped here.
        let err = serde_json::from_str::<ChatCompletionResponse>(r#"{"choices":[null]}"#)
            .expect_err("null element inside choices must not silently become a default Choice");
        let msg = err.to_string();
        assert!(msg.contains("invalid type"), "{msg}");
    }

    // ============================================================================
    // Task 38: Anthropic leniency (report A M5) - absent usage/content parse,
    // tokens default to 0
    // ============================================================================

    #[test]
    fn test_anthropic_message_without_usage_parses() {
        // Given: a real-shaped Anthropic message with NO usage block (A-M5:
        // required usage aborted the whole parse when absent)
        // Then: it parses and both tokens read as 0 (pinned 0-vs-None decision:
        // spec defaults the TOKENS themselves -> 0, never None)
        let json = serde_json::json!({
            "id": "msg-789",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Done."}],
            "model": "test-model",
            "stop_reason": "end_turn"
        });
        let msg: AnthropicMessage = serde_json::from_value(json).unwrap();
        assert_eq!(msg.id, "msg-789");
        assert_eq!(msg.usage.input_tokens, 0);
        assert_eq!(msg.usage.output_tokens, 0);
    }

    #[test]
    fn test_anthropic_message_without_content_parses() {
        // Given: a message with no content array
        // Then: content defaults to empty vec, and the From-conversion path
        // treats it as no-content (message.content None), not a panic
        let json = serde_json::json!({
            "id": "msg-no-content",
            "type": "message",
            "role": "assistant",
            "model": "test-model",
            "usage": {"input_tokens": 3, "output_tokens": 1}
        });
        let msg: AnthropicMessage = serde_json::from_value(json).unwrap();
        assert!(msg.content.is_empty());

        let resp: ChatCompletionResponse = msg.into();
        assert_eq!(
            resp.choices[0].message.as_ref().unwrap().content,
            None,
            "empty content vec must convert to None content, not Some(\"\")"
        );
    }

    #[test]
    fn test_anthropic_usage_empty_object_tokens_default_zero() {
        // Given: usage present but both token fields absent
        // Then: each token field individually defaults to 0
        let json = serde_json::json!({
            "id": "msg-tokens",
            "type": "message",
            "role": "assistant",
            "content": [],
            "model": "test-model",
            "usage": {}
        });
        let msg: AnthropicMessage = serde_json::from_value(json).unwrap();
        assert_eq!(msg.usage.input_tokens, 0);
        assert_eq!(msg.usage.output_tokens, 0);
    }

    #[test]
    fn test_absent_usage_converts_to_zeroed_openai_usage() {
        // Consumer seam (pinned, NOT changed): the From impl reads msg.usage
        // fields directly, so absent usage flows through as Some(Usage{0,0,0})
        // handler.rs sums input+output -> 0; synthesis emits zeroed usage JSON.
        let json = serde_json::json!({
            "id": "msg-zero",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "hi"}],
            "model": "m"
        });
        let msg: AnthropicMessage = serde_json::from_value(json).unwrap();
        let resp: ChatCompletionResponse = msg.into();
        let usage = resp.usage.unwrap();
        assert_eq!((usage.prompt_tokens, usage.completion_tokens, usage.total_tokens), (0, 0, 0));
    }

    // ============================================================================
    // Task 40: thinking/ToolResult conversion fidelity (report A M6/M7)
    // ============================================================================

    fn assistant_msg(content: Vec<AnthropicContentBlock>) -> AnthropicMessage {
        AnthropicMessage {
            id: "msg-t40".to_string(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            content,
            model: "m".to_string(),
            stop_reason: None,
            stop_sequence: None,
            usage: AnthropicUsage::default(),
        }
    }

    #[test]
    fn test_thinking_signature_maps_to_reasoning_opaque_when_present() {
        // Task 40 rule 2 (A-M6): the ResponseMessage reasoning fields were
        // hardcoded None (openai.rs:476-477 at 92ab176). thinking.signature is
        // the opaque proof-of-integrity state and maps to reasoning_opaque;
        // a thinking block WITHOUT a signature leaves reasoning_opaque None.
        // RED baseline (raw): both fields always None (probe confirmed).
        let msg = assistant_msg(vec![
            AnthropicContentBlock::Thinking {
                thinking: "step one".to_string(),
                signature: Some("SIG_ABC".to_string()),
            },
            AnthropicContentBlock::Thinking {
                thinking: "step two".to_string(),
                signature: None,
            },
        ]);
        let resp: ChatCompletionResponse = msg.into();
        let message = resp.choices[0].message.as_ref().unwrap();
        assert_eq!(message.content, None, "thinking must not leak into content");
        assert_eq!(
            message.reasoning_text,
            Some("step one\nstep two".to_string()),
            "multiple thinking blocks join with newline, matching the content joiner"
        );
        assert_eq!(message.reasoning_opaque, Some("SIG_ABC".to_string()));

        // Adversarial: thinking WITHOUT the signature field at all -> None.
        let json = serde_json::json!({
            "id": "msg-nosig", "type": "message", "role": "assistant", "model": "m",
            "content": [{"type": "thinking", "thinking": "bare"}]
        });
        let msg: AnthropicMessage = serde_json::from_value(json).unwrap();
        let resp: ChatCompletionResponse = msg.into();
        let message = resp.choices[0].message.as_ref().unwrap();
        assert_eq!(message.reasoning_text, Some("bare".to_string()));
        assert_eq!(message.reasoning_opaque, None);
    }

    #[test]
    fn test_tool_result_array_parts_join_text_fields_non_text_skipped() {
        // Task 40 rule 3 (A-M7): tool_result content as array-of-parts was
        // DROPPED by the conversion (content.as_str() only). Text parts now
        // join with "\n"; non-text parts are skipped. The plain-String shape
        // keeps working (both shapes probed at baseline: both deserialize).
        // RED baseline (raw): array shape -> content None (dropped).
        let array_content = serde_json::json!([
            {"type": "text", "text": "a"},
            {"type": "image", "x": 1},
            {"type": "text", "text": "b"}
        ]);
        let msg = assistant_msg(vec![AnthropicContentBlock::ToolResult {
            tool_use_id: "toolu_1".to_string(),
            content: array_content,
            is_error: None,
        }]);
        let resp: ChatCompletionResponse = msg.into();
        assert_eq!(resp.choices[0].message.as_ref().unwrap().content, Some("a\nb".to_string()));

        // Plain String shape unchanged.
        let msg = assistant_msg(vec![AnthropicContentBlock::ToolResult {
            tool_use_id: "toolu_2".to_string(),
            content: serde_json::json!("plain"),
            is_error: None,
        }]);
        let resp: ChatCompletionResponse = msg.into();
        assert_eq!(resp.choices[0].message.as_ref().unwrap().content, Some("plain".to_string()));

        // Array with only non-text parts contributes nothing (no empty line).
        let msg = assistant_msg(vec![AnthropicContentBlock::ToolResult {
            tool_use_id: "toolu_3".to_string(),
            content: serde_json::json!([{"type": "image"}]),
            is_error: None,
        }]);
        let resp: ChatCompletionResponse = msg.into();
        assert_eq!(resp.choices[0].message.as_ref().unwrap().content, None);
    }

    // ============================================================================
    // Task 41 (A-L4, openai.rs half): chunks without delta parse
    // RED baseline (raw probe @92ab176):
    //   Err("missing field `delta` at line 1 column 112")
    // ============================================================================

    #[test]
    fn test_stream_choice_without_delta_parses_with_empty_default() {
        let chunk: StreamChunk = serde_json::from_str(
            r#"{"id":"x","object":"chat.completion.chunk","created":0,"model":"m","choices":[{"index":0,"finish_reason":"stop"}]}"#,
        )
        .unwrap();
        let delta = &chunk.choices[0].delta;
        assert!(delta.content.is_none());
        assert!(delta.role.is_none());
        assert!(delta.tool_calls.is_none());
        assert!(delta.reasoning_text.is_none());
        assert!(delta.reasoning_opaque.is_none());
        assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // ============================================================================
    // Task 39: unknown Anthropic content blocks survive as Other
    // RED baseline (raw, probe at 92ab176, re-confirmed at fcec6c8): request AND
    // response parses failed with Err("unknown variant `advisor_tool_result`,
    // expected one of `text`, `thinking`, `tool_use`, `tool_result`")
    // ============================================================================

    #[test]
    fn test_unknown_content_block_parses_as_other_and_round_trips() {
        let block = serde_json::json!({"type":"advisor_tool_result","tool_use_id":"x","content":"hi"});
        let raw = serde_json::json!({
            "model": "t",
            "max_tokens": 100,
            "messages": [{"role":"user","content":[{"type":"text","text":"hi"}, block.clone()]}]
        });

        let req: AnthropicMessageRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.messages[0].content.len(), 2);
        match &req.messages[0].content[1] {
            AnthropicContentBlock::Other(v) => assert_eq!(v, &block),
            other => panic!("expected Other, got {other:?}"),
        }

        // Forwarding reality (pinned in evidence): handler forwards /v1/messages
        // bodies as bytes; this typed parse feeds the augment-injection
        // re-serialization. Structural fidelity: the unknown object re-emits
        // exactly as it arrived.
        let out = serde_json::to_value(&req).unwrap();
        assert_eq!(out["messages"][0]["content"][1], block);
        assert_eq!(out["messages"][0]["content"][0]["type"], "text");
    }

    #[test]
    fn test_other_block_preserves_nested_arrays_unicode_and_all_keys() {
        // Adversarial: nested arrays, unicode, many keys. Pinned STRUCTURAL
        // equality + every original key present (NOT byte order: serde_json
        // Map here is BTreeMap - preserve_order feature is not enabled).
        let block = serde_json::json!({
            "type": "weird_block",
            "nested": [1, {"中": "文"}, [true, null]],
            "emoji": "🤖",
            "z_key": 1,
            "a_key": 2
        });
        let b: AnthropicContentBlock = serde_json::from_value(block.clone()).unwrap();
        assert!(matches!(&b, AnthropicContentBlock::Other(v) if v == &block));
        let out = serde_json::to_value(&b).unwrap();
        assert_eq!(out, block);
        for key in ["type", "nested", "emoji", "z_key", "a_key"] {
            assert!(out.get(key).is_some(), "original key {key} lost: {out}");
        }
    }

    #[test]
    fn test_known_tag_with_shape_mismatch_falls_to_other() {
        // A known type whose required field is missing is a shape mismatch:
        // the whole raw object is preserved instead of failing the parse.
        let raw = serde_json::json!({"type":"text","unexpected":true});
        let b: AnthropicContentBlock = serde_json::from_value(raw.clone()).unwrap();
        assert!(matches!(&b, AnthropicContentBlock::Other(v) if v == &raw));
    }

    #[test]
    fn test_known_blocks_still_serialize_byte_identical_to_derive_form() {
        // Serialize parity pin: known variants emit the same tagged objects as
        // the old #[derive(Serialize)] internally-tagged enum - including the
        // explicit nulls the derive emitted for absent Option fields.
        let t: AnthropicContentBlock = serde_json::from_str(r#"{"type":"text","text":"hi"}"#).unwrap();
        assert!(matches!(&t, AnthropicContentBlock::Text { text } if text == "hi"));
        assert_eq!(serde_json::to_string(&t).unwrap(), r#"{"type":"text","text":"hi"}"#);

        let th: AnthropicContentBlock = serde_json::from_str(r#"{"type":"thinking","thinking":"x"}"#).unwrap();
        assert_eq!(
            serde_json::to_string(&th).unwrap(),
            r#"{"type":"thinking","thinking":"x","signature":null}"#
        );

        let tr: AnthropicContentBlock =
            serde_json::from_str(r#"{"type":"tool_result","tool_use_id":"t","content":"c"}"#).unwrap();
        assert_eq!(
            serde_json::to_string(&tr).unwrap(),
            r#"{"type":"tool_result","tool_use_id":"t","content":"c","is_error":null}"#
        );
    }

    #[test]
    fn test_anthropic_message_with_unknown_block_parses_and_converts_without_panic() {
        let msg: AnthropicMessage = serde_json::from_value(serde_json::json!({
            "id": "msg-unknown",
            "type": "message",
            "role": "assistant",
            "model": "m",
            "content": [
                {"type": "text", "text": "hi"},
                {"type": "advisor_tool_result", "tool_use_id": "x", "content": "hi"}
            ]
        }))
        .unwrap();
        assert!(matches!(&msg.content[1], AnthropicContentBlock::Other(_)));

        // Response conversion: no OpenAI carrier for an opaque block - it is
        // skipped; the known text block still converts and the parse survives.
        let resp: ChatCompletionResponse = msg.into();
        assert_eq!(resp.choices[0].message.as_ref().unwrap().content, Some("hi".to_string()));
    }
}
