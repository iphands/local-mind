//! Fix for null or missing index fields in tool calls
//!
//! llama.cpp sometimes sends tool calls with index=null or missing index field.
//! This causes validation errors in clients expecting numeric indices.
//!
//! This fix assigns sequential indices (0, 1, 2, ...) to tool calls that lack them.

use crate::fixes::{FixAction, FixLogLevel, ResponseFix};
use serde_json::Value;

pub struct ToolCallNullIndexFix {
    enabled: bool,
}

impl ToolCallNullIndexFix {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Check if a tool call has null or missing index
    fn needs_index_fix(tool_call: &Value) -> bool {
        match tool_call.get("index") {
            None => true,                    // Missing index field
            Some(Value::Null) => true,       // Explicit null
            Some(Value::Number(_)) => false, // Has valid number
            _ => true,                       // Invalid type
        }
    }

    /// Fix tool calls in a choices array (works for both message and delta)
    fn fix_tool_calls_in_choices(choices: &mut Vec<Value>) -> bool {
        let mut fixed_any = false;

        for choice in choices.iter_mut() {
            // Try message.tool_calls (non-streaming complete response)
            if let Some(message) = choice.get_mut("message") {
                if let Some(tool_calls) = message.get_mut("tool_calls").and_then(|tc| tc.as_array_mut()) {
                    fixed_any |= Self::assign_sequential_indices(tool_calls);
                }
            }

            // Try delta.tool_calls (streaming chunks)
            if let Some(delta) = choice.get_mut("delta") {
                if let Some(tool_calls) = delta.get_mut("tool_calls").and_then(|tc| tc.as_array_mut()) {
                    fixed_any |= Self::assign_sequential_indices(tool_calls);
                }
            }
        }

        fixed_any
    }

    /// Assign sequential indices to tool calls
    fn assign_sequential_indices(tool_calls: &mut Vec<Value>) -> bool {
        let mut fixed_any = false;

        for (idx, tool_call) in tool_calls.iter_mut().enumerate() {
            if Self::needs_index_fix(tool_call) {
                if let Some(obj) = tool_call.as_object_mut() {
                    obj.insert("index".to_string(), Value::Number((idx as u32).into()));
                    fixed_any = true;
                }
            }
        }

        fixed_any
    }
}

impl ResponseFix for ToolCallNullIndexFix {
    fn name(&self) -> &str {
        "toolcall_null_index_fix"
    }

    fn description(&self) -> &str {
        "Fixes null or missing index fields in tool calls by assigning sequential indices"
    }

    fn log_level(&self) -> FixLogLevel {
        // Demote to DEBUG - this fix applies to nearly every request
        // Use RUST_LOG=debug to see these messages
        FixLogLevel::Debug
    }

    fn applies(&self, response: &Value) -> bool {
        if !self.enabled {
            return false;
        }

        // Check if response has tool calls with null/missing indices
        if let Some(choices) = response.get("choices").and_then(|c| c.as_array()) {
            for choice in choices {
                // Check message.tool_calls
                if let Some(tool_calls) = choice
                    .get("message")
                    .and_then(|m| m.get("tool_calls"))
                    .and_then(|tc| tc.as_array())
                {
                    if tool_calls.iter().any(Self::needs_index_fix) {
                        return true;
                    }
                }

                // Check delta.tool_calls
                if let Some(tool_calls) = choice
                    .get("delta")
                    .and_then(|d| d.get("tool_calls"))
                    .and_then(|tc| tc.as_array())
                {
                    if tool_calls.iter().any(Self::needs_index_fix) {
                        return true;
                    }
                }
            }
        }

        false
    }

    fn apply(&self, mut response: Value) -> (Value, FixAction) {
        if !self.enabled {
            return (response, FixAction::NotApplicable);
        }

        let mut choices = match response.get_mut("choices").and_then(|c| c.as_array_mut()) {
            Some(c) => c.clone(),
            None => return (response, FixAction::NotApplicable),
        };

        let fixed_any = Self::fix_tool_calls_in_choices(&mut choices);

        if fixed_any {
            response["choices"] = Value::Array(choices);
            (
                response,
                FixAction::Fixed {
                    original_snippet: "tool_calls with null/missing indices".to_string(),
                    fixed_snippet: "tool_calls with sequential indices (0, 1, 2, ...)".to_string(),
                },
            )
        } else {
            (response, FixAction::NotApplicable)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_needs_index_fix_null() {
        let tool_call = json!({"id": "call-1", "index": null});
        assert!(ToolCallNullIndexFix::needs_index_fix(&tool_call));
    }

    #[test]
    fn test_needs_index_fix_missing() {
        let tool_call = json!({"id": "call-1"});
        assert!(ToolCallNullIndexFix::needs_index_fix(&tool_call));
    }

    #[test]
    fn test_needs_index_fix_valid() {
        let tool_call = json!({"id": "call-1", "index": 0});
        assert!(!ToolCallNullIndexFix::needs_index_fix(&tool_call));
    }

    #[test]
    fn test_fix_message_tool_calls() {
        let fix = ToolCallNullIndexFix::new(true);
        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [
                        {"id": "call-1", "index": null, "function": {"name": "test"}},
                        {"id": "call-2", "function": {"name": "test2"}}
                    ]
                }
            }]
        });

        let (fixed, action) = fix.apply(response);

        assert!(matches!(action, FixAction::Fixed { .. }));
        assert_eq!(fixed["choices"][0]["message"]["tool_calls"][0]["index"], 0);
        assert_eq!(fixed["choices"][0]["message"]["tool_calls"][1]["index"], 1);
    }

    #[test]
    fn test_fix_delta_tool_calls() {
        let fix = ToolCallNullIndexFix::new(true);
        let response = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [
                        {"id": "call-1", "index": null, "function": {"name": "test"}}
                    ]
                }
            }]
        });

        let (fixed, action) = fix.apply(response);

        assert!(matches!(action, FixAction::Fixed { .. }));
        assert_eq!(fixed["choices"][0]["delta"]["tool_calls"][0]["index"], 0);
    }

    #[test]
    fn test_no_fix_needed() {
        let fix = ToolCallNullIndexFix::new(true);
        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [
                        {"id": "call-1", "index": 0, "function": {"name": "test"}}
                    ]
                }
            }]
        });

        let (_, action) = fix.apply(response);
        assert!(matches!(action, FixAction::NotApplicable));
    }

    #[test]
    fn test_applies_detection() {
        let fix = ToolCallNullIndexFix::new(true);

        let response_needs_fix = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{"id": "call-1", "index": null}]
                }
            }]
        });
        assert!(fix.applies(&response_needs_fix));

        let response_ok = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{"id": "call-1", "index": 0}]
                }
            }]
        });
        assert!(!fix.applies(&response_ok));
    }

    #[test]
    fn test_multiple_tool_calls_sequential_indices() {
        let fix = ToolCallNullIndexFix::new(true);
        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [
                        {"id": "call-1", "index": null},
                        {"id": "call-2", "index": null},
                        {"id": "call-3", "index": null}
                    ]
                }
            }]
        });

        let (fixed, action) = fix.apply(response);

        assert!(matches!(action, FixAction::Fixed { .. }));
        assert_eq!(fixed["choices"][0]["message"]["tool_calls"][0]["index"], 0);
        assert_eq!(fixed["choices"][0]["message"]["tool_calls"][1]["index"], 1);
        assert_eq!(fixed["choices"][0]["message"]["tool_calls"][2]["index"], 2);
    }

    #[test]
    fn test_mixed_indices() {
        let fix = ToolCallNullIndexFix::new(true);
        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [
                        {"id": "call-1", "index": 0},
                        {"id": "call-2", "index": null},
                        {"id": "call-3"}
                    ]
                }
            }]
        });

        let (fixed, action) = fix.apply(response);

        assert!(matches!(action, FixAction::Fixed { .. }));
        // First already has correct index
        assert_eq!(fixed["choices"][0]["message"]["tool_calls"][0]["index"], 0);
        // Second gets index 1 (fixes null)
        assert_eq!(fixed["choices"][0]["message"]["tool_calls"][1]["index"], 1);
        // Third gets index 2 (fixes missing)
        assert_eq!(fixed["choices"][0]["message"]["tool_calls"][2]["index"], 2);
    }

    #[test]
    fn test_disabled() {
        let fix = ToolCallNullIndexFix::new(false);
        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{"id": "call-1", "index": null}]
                }
            }]
        });

        assert!(!fix.applies(&response));
        let (_, action) = fix.apply(response);
        assert!(matches!(action, FixAction::NotApplicable));
    }

    #[test]
    fn test_log_level_is_debug() {
        use crate::fixes::{FixLogLevel, ResponseFix};
        let fix = ToolCallNullIndexFix::new(true);

        // Verify this fix uses DEBUG log level (not INFO)
        assert_eq!(fix.log_level(), FixLogLevel::Debug);
    }
}
