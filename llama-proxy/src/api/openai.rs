//! OpenAI-compatible API type definitions

use serde::{Deserialize, Serialize};

/// Chat completion request
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub tools: Option<Vec<Tool>>,
    #[serde(default)]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default)]
    pub stop: Option<Vec<String>>,
    #[serde(default)]
    pub frequency_penalty: Option<f32>,
    #[serde(default)]
    pub presence_penalty: Option<f32>,
    #[serde(default)]
    pub user: Option<String>,

    // Opencode request extensions (pass-through to backend)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<u64>,
}

/// Chat message
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub role: String,
    #[serde(default)]
    pub content: Option<MessageContent>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default)]
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
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(default)]
    pub timings: Option<Timings>,
}

/// Response choice
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Choice {
    pub index: u32,
    pub message: Option<ResponseMessage>,
    pub delta: Option<Delta>,
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
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "type", default)]
    pub call_type: Option<String>,
    #[serde(default)]
    pub index: Option<u32>,
    pub function: FunctionCall,
}

/// Function call
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Streaming delta
#[derive(Debug, Clone, Deserialize, Serialize)]
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
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
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
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

// ============================================================================
// Anthropic Messages API Types
// ============================================================================

/// Anthropic Messages API response format
/// Used by llama.cpp when endpoint is /v1/messages
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String, // "message"
    pub role: String,
    pub content: Vec<AnthropicContentBlock>,
    pub model: String,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub stop_sequence: Option<String>,
    pub usage: AnthropicUsage,
}

/// Anthropic content block (text, thinking, tool_use, or tool_result)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum AnthropicContentBlock {
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

/// Anthropic usage (different field names than OpenAI)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Convert Anthropic Message to OpenAI ChatCompletionResponse
/// This allows us to reuse synthesis logic for Anthropic format
impl From<AnthropicMessage> for ChatCompletionResponse {
    fn from(msg: AnthropicMessage) -> Self {
        // Convert content blocks to text and tool_calls
        let mut content_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for block in &msg.content {
            match block {
                AnthropicContentBlock::Text { text } => {
                    content_parts.push(text.clone());
                }
                AnthropicContentBlock::Thinking { thinking, .. } => {
                    content_parts.push(thinking.clone());
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
                    // Include tool results as text for now
                    if let Some(text) = content.as_str() {
                        content_parts.push(text.to_string());
                    }
                }
            }
        }

        let content = if content_parts.is_empty() {
            None
        } else {
            Some(content_parts.join("\n"))
        };

        let tool_calls_opt = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

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
                    reasoning_text: None,
                    reasoning_opaque: None,
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
            .get(0)
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
                            id: tool_call.id.clone().unwrap_or_else(|| {
                                format!("toolu_{}", uuid::Uuid::new_v4().to_string().replace('-', ""))
                            }),
                            name: tool_call.function.name.clone(),
                            input,
                        });
                    }
                }

                // If no content at all, add empty text block
                if blocks.is_empty() {
                    blocks.push(AnthropicContentBlock::Text {
                        text: String::new(),
                    });
                }

                blocks
            })
            .unwrap_or_else(|| {
                vec![AnthropicContentBlock::Text {
                    text: String::new(),
                }]
            });

        // Convert usage from OpenAI format to Anthropic format
        let usage = resp.usage.as_ref()
            .map(|u| AnthropicUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            })
            .unwrap_or(AnthropicUsage {
                input_tokens: 0,
                output_tokens: 0,
            });

        // Map OpenAI finish_reason to Anthropic stop_reason
        let stop_reason = resp.choices.get(0).and_then(|c| c.finish_reason.as_ref()).map(|r| {
            match r.as_str() {
                "stop" => "end_turn".to_string(),
                "length" => "max_tokens".to_string(),
                "tool_calls" => "tool_use".to_string(),
                other => other.to_string(),
            }
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
        let tool_calls = response.choices[0]
            .message
            .as_ref()
            .unwrap()
            .tool_calls
            .as_ref()
            .unwrap();
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
    fn test_anthropic_to_openai_with_thinking() {
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
        // Content should concatenate thinking and text with newline
        assert_eq!(
            openai_response.choices[0].message.as_ref().unwrap().content,
            Some("Let me think...\nAnswer".to_string())
        );
    }

    #[test]
    fn test_anthropic_stop_reason_mapping() {
        let test_cases = vec![
            ("end_turn", "stop"),
            ("max_tokens", "length"),
            ("stop_sequence", "stop"),
        ];

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
        let test_cases = vec![
            ("stop", "end_turn"),
            ("length", "max_tokens"),
            ("tool_calls", "tool_use"),
        ];

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
            AnthropicContentBlock::ToolResult { tool_use_id, content, is_error } => {
                assert_eq!(tool_use_id, "toolu_abc123");
                assert_eq!(content.as_str(), Some("The weather in Paris is 22°C and sunny."));
                assert_eq!(*is_error, None);
            }
            _ => panic!("Expected tool_result block"),
        }
    }
}
