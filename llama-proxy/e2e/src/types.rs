//! Shared types for the e2e test framework

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// A mock response the backend will serve for the next request to /v1/chat/completions
#[derive(Debug, Clone)]
pub struct MockResponse {
    pub status: u16,
    pub body: String,
    pub content_type: String,
}

impl MockResponse {
    /// Create a standard JSON chat completion response
    pub fn json(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            content_type: "application/json".to_string(),
        }
    }

    /// Create an error response
    pub fn error(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
            content_type: "application/json".to_string(),
        }
    }
}

/// Shared state for the mock backend server
#[derive(Debug, Default)]
pub struct BackendState {
    /// Queue of responses to serve - tests push responses, backend pops and serves them
    pub response_queue: VecDeque<MockResponse>,
    /// All requests received by the backend (for inspection)
    pub received_requests: Vec<ReceivedRequest>,
}

/// A request received by the mock backend
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ReceivedRequest {
    pub method: String,
    pub path: String,
    pub body: serde_json::Value,
}

pub type SharedBackendState = Arc<Mutex<BackendState>>;

/// A parsed SSE event from the proxy streaming response
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub data: String,
    pub is_done: bool,
}

impl SseEvent {
    pub fn parse_json(&self) -> anyhow::Result<serde_json::Value> {
        serde_json::from_str(&self.data).map_err(|e| anyhow::anyhow!("SSE JSON parse error: {}: {}", e, self.data))
    }
}

/// Result of a non-streaming proxy request
#[derive(Debug)]
pub struct ProxyResponse {
    pub status: u16,
    pub body: serde_json::Value,
}

impl ProxyResponse {
    /// Get a nested field using dot notation (e.g. "choices.0.message.content")
    pub fn get(&self, path: &str) -> Option<&serde_json::Value> {
        let mut current = &self.body;
        for part in path.split('.') {
            current = if let Ok(idx) = part.parse::<usize>() {
                current.as_array()?.get(idx)?
            } else {
                current.as_object()?.get(part)?
            };
        }
        Some(current)
    }

    pub fn get_str(&self, path: &str) -> Option<&str> {
        self.get(path)?.as_str()
    }

    pub fn tool_call_args(&self, choice: usize, tc: usize) -> Option<&str> {
        self.get(&format!("choices.{choice}.message.tool_calls.{tc}.function.arguments"))?.as_str()
    }

    #[allow(dead_code)]
    pub fn tool_call_args_json(&self, choice: usize, tc: usize) -> anyhow::Result<serde_json::Value> {
        let args = self.tool_call_args(choice, tc)
            .ok_or_else(|| anyhow::anyhow!("No tool call args at choices[{choice}].tool_calls[{tc}]"))?;
        serde_json::from_str(args).map_err(|e| anyhow::anyhow!("Tool call args not valid JSON: {}: {}", e, args))
    }
}

/// Result of a streaming proxy request - all parsed SSE events
#[derive(Debug)]
pub struct StreamingResponse {
    pub events: Vec<SseEvent>,
}

impl StreamingResponse {
    /// Check that the stream ends with [DONE]
    pub fn has_done_marker(&self) -> bool {
        self.events.last().map(|e| e.is_done).unwrap_or(false)
    }

    /// Get all data events (excluding [DONE])
    pub fn data_events(&self) -> Vec<&SseEvent> {
        self.events.iter().filter(|e| !e.is_done).collect()
    }

    /// Accumulate all text content deltas
    pub fn accumulated_content(&self) -> String {
        let mut result = String::new();
        for event in self.data_events() {
            if let Ok(json) = event.parse_json() {
                if let Some(content) = json
                    .pointer("/choices/0/delta/content")
                    .and_then(|v| v.as_str())
                {
                    result.push_str(content);
                }
            }
        }
        result
    }

    /// Accumulate tool call arguments for a specific tool call index
    pub fn accumulated_tool_args(&self, tool_call_idx: usize) -> String {
        let mut result = String::new();
        for event in self.data_events() {
            if let Ok(json) = event.parse_json() {
                if let Some(tool_calls) = json.pointer("/choices/0/delta/tool_calls").and_then(|v| v.as_array()) {
                    for tc in tool_calls {
                        let idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        if idx == tool_call_idx {
                            if let Some(args) = tc.pointer("/function/arguments").and_then(|v| v.as_str()) {
                                result.push_str(args);
                            }
                        }
                    }
                }
            }
        }
        result
    }

    /// Get the finish_reason from the final data chunk
    pub fn finish_reason(&self) -> Option<String> {
        for event in self.data_events().iter().rev() {
            if let Ok(json) = event.parse_json() {
                if let Some(reason) = json
                    .pointer("/choices/0/finish_reason")
                    .and_then(|v| v.as_str())
                {
                    if reason != "null" {
                        return Some(reason.to_string());
                    }
                }
            }
        }
        None
    }
}

/// Result of a single test case
#[derive(Debug)]
#[allow(dead_code)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
}
