//! Fix for null or missing index fields in tool calls
//!
//! llama.cpp sometimes sends tool calls with index=null or a missing index
//! field. This causes validation errors in clients expecting numeric indices.
//!
//! ## Index-repair semantics
//!
//! Repair is **positional** and **minimal**:
//!
//! - An entry whose `index` is missing, `null`, or not a number is assigned
//!   the index of its 0-based position within the `tool_calls` array.
//! - An entry with a numeric `index` keeps it untouched, even when that number
//!   disagrees with the entry's position. This fix never renumbers valid data,
//!   so a repaired array can still contain gaps or duplicate indices if the
//!   model emitted them: repair fixes "absent/invalid", not "inconsistent".
//! - Non-object array entries are skipped — there is nothing to insert into.
//!
//! The fix walks both response shapes, `message.tool_calls` (complete
//! responses) and `delta.tool_calls` (chunk shapes), and the registry
//! registers it FIRST because the other fixes may assume indices exist.
//!
//! ## Enabled state
//!
//! Owned exclusively by the registry ([`crate::fixes::FixRegistry::set_enabled`]
//! and the `fixes:` config, where a disabled module is not even constructed).
//! The struct carries no flag of its own [B-L3]: two sources of truth for one
//! switch is how a "disabled" fix keeps running.

use crate::fixes::{FixAction, FixLogLevel, ResponseFix};
use serde_json::Value;

/// Fixes null/missing/non-numeric `index` fields by assigning positional
/// indices (see the module docs for the exact repair semantics).
#[derive(Default)]
pub struct ToolCallNullIndexFix;

impl ToolCallNullIndexFix {
    pub fn new() -> Self {
        Self
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
    fn fix_tool_calls_in_choices(choices: &mut [Value]) -> bool {
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

    /// Assign positional indices to entries whose `index` needs repair.
    /// Entries that are not objects are skipped, valid numeric indices pass
    /// through untouched (module docs pin the semantics).
    fn assign_sequential_indices(tool_calls: &mut [Value]) -> bool {
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
        // In-place mutation through the borrow: no clone of the choices array,
        // no reassign of `response["choices"]` [B-L2].
        let fixed_any = match response.get_mut("choices").and_then(|c| c.as_array_mut()) {
            Some(choices) => Self::fix_tool_calls_in_choices(choices),
            None => return (response, FixAction::NotApplicable),
        };

        if fixed_any {
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
    use std::sync::Arc;

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
    fn test_needs_index_fix_wrong_type() {
        for bad in [json!("0"), json!(true), json!([0]), json!({"i": 0})] {
            let tool_call = json!({"id": "call-1", "index": bad});
            assert!(
                ToolCallNullIndexFix::needs_index_fix(&tool_call),
                "non-number index must need repair: {tool_call}"
            );
        }
    }

    #[test]
    fn test_fix_message_tool_calls() {
        let fix = ToolCallNullIndexFix::new();
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
        let fix = ToolCallNullIndexFix::new();
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
        let fix = ToolCallNullIndexFix::new();
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
        let fix = ToolCallNullIndexFix::new();

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
        let fix = ToolCallNullIndexFix::new();
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
        let fix = ToolCallNullIndexFix::new();
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
    fn test_enabled_state_is_registry_owned() {
        // [B-L3] The fix struct carries no enabled flag: the registry's map is
        // the single switch, driven by FixRegistry::set_enabled / config.
        let mut registry = crate::fixes::FixRegistry::new();
        registry.register(Arc::new(ToolCallNullIndexFix::new()));

        let response = json!({
            "choices": [{
                "message": { "tool_calls": [{"id": "call-1", "index": null}] }
            }]
        });

        let repaired = registry.apply_fixes(response.clone());
        assert_eq!(repaired["choices"][0]["message"]["tool_calls"][0]["index"], 0);

        registry.set_enabled("toolcall_null_index_fix", false);
        let untouched = registry.apply_fixes(response.clone());
        assert_eq!(
            serde_json::to_string(&untouched).unwrap(),
            serde_json::to_string(&response).unwrap(),
            "a registry-disabled fix must not touch the response"
        );
    }

    #[test]
    fn test_repairs_wrong_typed_index_positionally() {
        let fix = ToolCallNullIndexFix::new();
        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [
                        {"id": "call-1", "index": "0"},
                        {"id": "call-2", "index": true}
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
    fn test_valid_numeric_index_kept_when_position_disagrees() {
        // Documented semantics: repair never renumbers valid data, even when
        // the surviving number and the assigned position collide or gap.
        let fix = ToolCallNullIndexFix::new();
        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [
                        {"id": "call-1", "index": 5},
                        {"id": "call-2", "index": null}
                    ]
                }
            }]
        });

        let (fixed, action) = fix.apply(response);

        assert!(matches!(action, FixAction::Fixed { .. }));
        assert_eq!(
            fixed["choices"][0]["message"]["tool_calls"][0]["index"], 5,
            "valid index passes through"
        );
        assert_eq!(
            fixed["choices"][0]["message"]["tool_calls"][1]["index"], 1,
            "repair is positional, not sequential-after-valid"
        );
    }

    #[test]
    fn test_non_object_tool_call_entry_skipped_others_repaired() {
        let fix = ToolCallNullIndexFix::new();
        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [
                        {"id": "call-1"},
                        42,
                        {"id": "call-3", "index": null}
                    ]
                }
            }]
        });

        let (fixed, action) = fix.apply(response);

        assert!(matches!(action, FixAction::Fixed { .. }));
        assert_eq!(fixed["choices"][0]["message"]["tool_calls"][0]["index"], 0);
        assert_eq!(
            fixed["choices"][0]["message"]["tool_calls"][1],
            json!(42),
            "non-object entry must survive as sent"
        );
        assert_eq!(fixed["choices"][0]["message"]["tool_calls"][2]["index"], 2);
    }

    #[test]
    fn test_non_array_tool_calls_untouched() {
        let fix = ToolCallNullIndexFix::new();
        let response = json!({
            "choices": [{ "message": { "tool_calls": "not-an-array" } }]
        });

        let (fixed, action) = fix.apply(response.clone());
        assert!(matches!(action, FixAction::NotApplicable));
        assert_eq!(
            serde_json::to_string(&fixed).unwrap(),
            serde_json::to_string(&response).unwrap()
        );
    }

    #[test]
    fn test_missing_choices_untouched() {
        let fix = ToolCallNullIndexFix::new();
        let response = json!({ "id": "chatcmpl-1" });

        let (fixed, action) = fix.apply(response.clone());
        assert!(matches!(action, FixAction::NotApplicable));
        assert_eq!(
            serde_json::to_string(&fixed).unwrap(),
            serde_json::to_string(&response).unwrap()
        );
    }

    #[test]
    fn test_log_level_is_debug() {
        use crate::fixes::{FixLogLevel, ResponseFix};
        let fix = ToolCallNullIndexFix::new();

        // Verify this fix uses DEBUG log level (not INFO)
        assert_eq!(fix.log_level(), FixLogLevel::Debug);
    }
}
