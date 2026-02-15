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
}

/// Token usage
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
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
