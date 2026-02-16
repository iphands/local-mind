//! Fix for duplicate/malformed filePath in Qwen3-Coder tool calls
//!
//! Handles malformed JSON like:
//! `{"content":"code","filePath":"/path/to/file","filePath"/path/to/file"}`
//!
//! Where:
//! - filePath is duplicated
//! - The second occurrence is malformed JSON (missing colon/value quotes)

use super::{FixAction, ResponseFix, ToolCallAccumulator};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Fix for malformed filePath in Qwen3-Coder tool calls
pub struct ToolcallBadFilepathFix {
    /// If true, remove duplicate keys; if false, fix and keep both
    remove_duplicate: Arc<AtomicBool>,
}

impl ToolcallBadFilepathFix {
    pub fn new(remove_duplicate: bool) -> Self {
        Self {
            remove_duplicate: Arc::new(AtomicBool::new(remove_duplicate)),
        }
    }

    /// Set whether to remove duplicates
    pub fn set_remove_duplicate(&self, value: bool) {
        self.remove_duplicate.store(value, Ordering::SeqCst);
    }

    /// Check if arguments string is malformed
    fn is_malformed(&self, args: &str) -> bool {
        // Only trigger if:
        // 1. "filePath" appears as a JSON key (quoted, followed by colon or in duplicate pattern)
        // 2. AND the JSON is invalid

        // Look for "filePath" as a key pattern (not just anywhere in the string)
        let has_filepath_key = args.contains(r#""filePath":"#)
            || args.contains(r#","filePath""#)
            || args.starts_with(r#"{"filePath""#);

        if !has_filepath_key {
            return false;
        }

        // Check for duplicate "filePath" or malformed "filePath" patterns
        let filepath_count = args.matches(r#""filePath""#).count();

        // If multiple "filePath" keys, definitely malformed
        if filepath_count > 1 {
            return true;
        }

        // Single "filePath" key with invalid JSON
        !self.is_valid_json(args)
    }

    /// Check if a string is valid JSON
    fn is_valid_json(&self, s: &str) -> bool {
        serde_json::from_str::<Value>(s).is_ok()
    }

    /// Create a snippet for logging (static method)
    fn create_snippet_static(text: &str, max_len: usize) -> String {
        if text.len() > max_len {
            format!("{}...", &text[..max_len])
        } else {
            text.to_string()
        }
    }

    /// Attempt to fix malformed arguments string
    fn fix_arguments(&self, args: &str) -> String {
        // First, try to parse as-is
        if let Ok(json) = serde_json::from_str::<Value>(args) {
            // Valid JSON - no fix needed
            return serde_json::to_string(&json).unwrap_or_else(|_| args.to_string());
        }

        // Invalid JSON - try to fix
        self.fix_malformed_json(args)
    }

    /// Fix malformed JSON with duplicate/malformed filePath
    fn fix_malformed_json(&self, args: &str) -> String {
        // Pattern: "filePath":"/path","filePath"/path"
        // The second occurrence is missing the colon or has broken syntax

        // Try to find and fix the pattern
        let fixed = self.try_fix_duplicate_filepath(args);
        if self.is_valid_json(&fixed) {
            return fixed;
        }

        // Try more aggressive fixing
        self.try_aggressive_fix(args)
    }

    /// Try to fix duplicate filePath pattern
    fn try_fix_duplicate_filepath(&self, args: &str) -> String {
        // Look for pattern: "filePath":"...","filePath"...
        let fp_pattern = r#""filePath""#;

        let occurrences: Vec<_> = args.match_indices(fp_pattern).collect();

        if occurrences.len() < 2 {
            // No duplicates, try other fixes
            return args.to_string();
        }

        let remove_dup = self.remove_duplicate.load(Ordering::SeqCst);

        if remove_dup {
            // Remove everything from the second filePath occurrence
            // Find the second occurrence and what follows
            let first_end = occurrences[0].0 + fp_pattern.len();

            // Find the value after first filePath
            let after_first = &args[first_end..];

            // Find where the first filePath value ends
            if let Some(value_start) = after_first.find(':') {
                let after_colon = &after_first[value_start..];

                // Find the value (should be a string)
                if let Some(value_end) = self.find_string_end(after_colon) {
                    let keep_until = first_end + value_start + value_end;

                    // Reconstruct: take content up to end of first filePath value, then close
                    // This truncates everything after the first valid filePath value
                    let result = format!("{}{}", &args[..keep_until], "}");

                    return result;
                }
            }
        }

        args.to_string()
    }

    /// Find the end of a JSON string value starting from position after colon
    fn find_string_end(&self, s: &str) -> Option<usize> {
        let chars: Vec<char> = s.chars().collect();

        // Skip whitespace and colon
        let mut i = 0;
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ':') {
            i += 1;
        }

        // Expect opening quote
        if i >= chars.len() || chars[i] != '"' {
            return None;
        }
        i += 1;

        // Find closing quote (handle escapes)
        while i < chars.len() {
            if chars[i] == '\\' && i + 1 < chars.len() {
                i += 2; // Skip escaped char
                continue;
            }
            if chars[i] == '"' {
                return Some(i + 1);
            }
            i += 1;
        }

        None
    }

    /// More aggressive fix attempt
    fn try_aggressive_fix(&self, args: &str) -> String {
        // Try to extract valid key-value pairs and rebuild
        let mut result = String::from("{");

        // Simple regex-like extraction for "key":"value" patterns
        let mut in_string = false;
        let mut escaped = false;
        let mut current_key: Option<String> = None;
        let mut current_value: Option<String> = None;
        let mut chars = args.chars().peekable();
        let mut first_pair = true;
        let mut seen_keys = std::collections::HashSet::new();

        while let Some(c) = chars.next() {
            if escaped {
                if let Some(ref mut val) = current_value {
                    val.push(c);
                }
                escaped = false;
                continue;
            }

            match c {
                '\\' => {
                    escaped = true;
                    if let Some(ref mut val) = current_value {
                        val.push('\\');
                    }
                }
                '"' => {
                    in_string = !in_string;
                }
                ':' if !in_string => {
                    // Value starts
                    // Skip whitespace
                    while let Some(&next) = chars.peek() {
                        if next.is_whitespace() {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
                ',' if !in_string => {
                    // End of pair
                    if let (Some(key), Some(value)) = (&current_key, &current_value) {
                        if !seen_keys.contains(key) {
                            if !first_pair {
                                result.push(',');
                            }
                            result.push_str(&format!(r#""{}":"{}""#, key, value));
                            seen_keys.insert(key.clone());
                            first_pair = false;
                        }
                    }
                    current_key = None;
                    current_value = None;
                }
                '{' | '}' if !in_string => {
                    // Skip braces
                }
                _ if in_string => {
                    // Accumulate string content
                    if current_key.is_none() {
                        current_key = Some(String::new());
                    }
                    if current_value.is_some() {
                        if let Some(ref mut val) = current_value {
                            val.push(c);
                        }
                    } else if let Some(ref mut key) = current_key {
                        key.push(c);
                    }
                }
                _ => {}
            }
        }

        // Handle last pair
        if let (Some(key), Some(value)) = (&current_key, &current_value) {
            if !seen_keys.contains(key) {
                if !first_pair {
                    result.push(',');
                }
                result.push_str(&format!(r#""{}":"{}""#, key, value));
            }
        }

        result.push('}');

        // Validate the result
        if self.is_valid_json(&result) {
            result
        } else {
            // Last resort: return empty object
            "{}".to_string()
        }
    }
}

impl ResponseFix for ToolcallBadFilepathFix {
    fn name(&self) -> &str {
        "toolcall_bad_filepath"
    }

    fn description(&self) -> &str {
        "Fixes duplicate/malformed filePath in Qwen3-Coder tool calls"
    }

    fn applies(&self, response: &Value) -> bool {
        // Check for tool_calls with potentially malformed arguments
        response
            .get("choices")
            .and_then(|c| c.as_array())
            .map(|choices| {
                choices.iter().any(|choice| {
                    choice
                        .get("message")
                        .and_then(|m| m.get("tool_calls"))
                        .and_then(|tc| tc.as_array())
                        .map(|calls| {
                            calls.iter().any(|call| {
                                call.get("function")
                                    .and_then(|f| f.get("arguments"))
                                    .and_then(|a| a.as_str())
                                    .map(|args| self.is_malformed(args))
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    }

    fn apply(&self, mut response: Value) -> (Value, FixAction) {
        let mut overall_action = FixAction::NotApplicable;

        if let Some(choices) = response.get_mut("choices").and_then(|c| c.as_array_mut()) {
            for choice in choices {
                if let Some(tool_calls) = choice
                    .get_mut("message")
                    .and_then(|m| m.get_mut("tool_calls"))
                    .and_then(|tc| tc.as_array_mut())
                {
                    for call in tool_calls {
                        if let Some(function) = call.get_mut("function") {
                            if let Some(args) = function.get("arguments").and_then(|a| a.as_str()) {
                                if self.is_malformed(args) {
                                    let original = args.to_string();
                                    let fixed = self.fix_arguments(args);
                                    if self.is_valid_json(&fixed) {
                                        function["arguments"] = Value::String(fixed.clone());
                                        overall_action = FixAction::fixed(&original, &fixed);
                                    } else {
                                        overall_action = FixAction::failed(&original, &fixed);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        (response, overall_action)
    }

    fn apply_stream(&self, mut chunk: Value) -> (Value, FixAction) {
        let mut overall_action = FixAction::NotApplicable;

        // Handle streaming chunks with delta.tool_calls
        if let Some(choices) = chunk.get_mut("choices").and_then(|c| c.as_array_mut()) {
            for choice in choices {
                if let Some(tool_calls) = choice
                    .get_mut("delta")
                    .and_then(|d| d.get_mut("tool_calls"))
                    .and_then(|tc| tc.as_array_mut())
                {
                    for call in tool_calls {
                        if let Some(function) = call.get_mut("function") {
                            if let Some(args) = function.get("arguments").and_then(|a| a.as_str()) {
                                if self.is_malformed(args) {
                                    let original = args.to_string();
                                    let fixed = self.fix_arguments(args);
                                    if self.is_valid_json(&fixed) {
                                        function["arguments"] = Value::String(fixed.clone());
                                        overall_action = FixAction::fixed(&original, &fixed);
                                    } else {
                                        overall_action = FixAction::failed(&original, &fixed);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        (chunk, overall_action)
    }

    fn apply_stream_with_accumulation_default(
        &self,
        mut chunk: Value,
        accumulator: &mut ToolCallAccumulator,
    ) -> (Value, FixAction) {
        let mut overall_action = FixAction::NotApplicable;

        if let Some(choices) = chunk.get_mut("choices").and_then(|c| c.as_array_mut()) {
            for choice in choices {
                if let Some(tool_calls) = choice
                    .get_mut("delta")
                    .and_then(|d| d.get_mut("tool_calls"))
                    .and_then(|tc| tc.as_array_mut())
                {
                    for call in tool_calls {
                        if let (Some(index), Some(function)) = (
                            call.get("index").and_then(|i| i.as_u64()).map(|i| i as usize),
                            call.get_mut("function"),
                        ) {
                            if let Some(chunk_args) =
                                function.get("arguments").and_then(|a| a.as_str())
                            {
                                // Accumulate the arguments with eager detection
                                let accumulated = accumulator.accumulate_and_check(index, chunk_args, self.name());

                                // Only try to fix if the accumulated content looks complete
                                // (ends with } for an object) or has the specific duplicate pattern
                                let looks_complete = accumulated.trim_end().ends_with('}');
                                let has_duplicate_filepath =
                                    accumulated.matches(r#""filePath""#).count() > 1;

                                if looks_complete || has_duplicate_filepath {
                                    // Check if accumulated args are malformed
                                    if self.is_malformed(&accumulated) {
                                        // NEW: Log warning BEFORE attempting fix (don't rely only on FixAction)
                                        tracing::warn!(
                                            fix_name = self.name(),
                                            index = index,
                                            accumulated_length = accumulated.len(),
                                            filepath_count = accumulated.matches(r#""filePath""#).count(),
                                            original = Self::create_snippet_static(&accumulated, 200),
                                            "ATTEMPTING FIX: Malformed filePath detected in complete arguments"
                                        );

                                        let original = accumulated.clone();
                                        let fixed = self.fix_arguments(&accumulated);
                                        if self.is_valid_json(&fixed) {
                                            // CRITICAL FIX: Don't send the FULL fixed JSON
                                            // The client (Claude Code) accumulates deltas, so sending the full
                                            // fixed JSON would cause it to append to what it already has, corrupting it.
                                            //
                                            // Instead, calculate the DELTA needed to complete the JSON validly.
                                            // We need to determine what was already sent to the client and send
                                            // only the remaining part to make valid JSON.

                                            // Get what was already sent to client (accumulated minus current chunk)
                                            let current_chunk = chunk_args;
                                            let already_sent = if accumulated.ends_with(current_chunk) {
                                                &accumulated[..accumulated.len() - current_chunk.len()]
                                            } else {
                                                // Fallback: assume all previous content was sent
                                                ""
                                            };

                                            // Calculate the delta: what to add to already_sent to get fixed result
                                            // For the filePath fix, we need to handle the trailing comma issue.
                                            // Chunk 2 likely ended with a comma (expecting another field), but
                                            // chunk 3 was trying to add a malformed duplicate field.
                                            //
                                            // We have three options:
                                            // 1. Send `}` - leaves trailing comma, but some parsers accept it
                                            // 2. Send empty string - leaves incomplete JSON with trailing comma
                                            // 3. Send a dummy field to consume the comma
                                            //
                                            // Option 3 is the safest for compatibility
                                            let valid_completion = if already_sent.trim_end().ends_with(',') {
                                                // Trailing comma exists - send a minimal dummy field to consume it
                                                // This produces valid JSON that all parsers will accept
                                                // The field name "_" is short and indicates it's a placeholder
                                                r#""_":null}"#
                                            } else if already_sent.is_empty() {
                                                // First chunk - send the full fixed content
                                                &fixed
                                            } else {
                                                // Send what's needed to complete it
                                                "}"
                                            };

                                            function["arguments"] = Value::String(valid_completion.to_string());
                                            // Clear accumulator since we've fixed it
                                            accumulator.clear(index);

                                            // NEW: Log success explicitly
                                            tracing::info!(
                                                fix_name = self.name(),
                                                index = index,
                                                already_sent_to_client = Self::create_snippet_static(already_sent, 100),
                                                sending_delta = valid_completion,
                                                original_accumulated = Self::create_snippet_static(&original, 100),
                                                fixed_version = Self::create_snippet_static(&fixed, 100),
                                                "FIX SUCCESSFUL: Sending completion delta to client"
                                            );

                                            overall_action = FixAction::fixed(&original, &fixed);
                                        } else {
                                            // NEW: Log failure explicitly
                                            tracing::error!(
                                                fix_name = self.name(),
                                                index = index,
                                                original = Self::create_snippet_static(&original, 100),
                                                attempted_fix = Self::create_snippet_static(&fixed, 100),
                                                "FIX FAILED: Could not repair malformed filePath"
                                            );

                                            overall_action = FixAction::failed(&original, &fixed);
                                        }
                                        // If still invalid, keep accumulating
                                    } else if self.is_valid_json(&accumulated) {
                                        // Valid JSON - clear accumulator, use as-is
                                        accumulator.clear(index);
                                    }
                                }
                                // Otherwise keep accumulating
                            }
                        }
                    }
                }
            }
        }
        (chunk, overall_action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fix_duplicate_filepath() {
        let fix = ToolcallBadFilepathFix::new(true);

        let malformed = r#"{"content":"code","filePath":"/path/to/file","filePath"/path/to/file"}"#;
        assert!(fix.is_malformed(malformed));

        let fixed = fix.fix_arguments(malformed);
        assert!(fix.is_valid_json(&fixed));
    }

    #[test]
    fn test_valid_json_passthrough() {
        let fix = ToolcallBadFilepathFix::new(true);

        let valid = r#"{"content":"code","filePath":"/path/to/file"}"#;
        assert!(!fix.is_malformed(valid));

        let fixed = fix.fix_arguments(valid);
        assert_eq!(fixed, valid);
    }

    #[test]
    fn test_new_with_remove_duplicate() {
        let fix = ToolcallBadFilepathFix::new(true);
        assert_eq!(fix.name(), "toolcall_bad_filepath");
        assert!(fix.description().contains("filePath"));
    }

    #[test]
    fn test_set_remove_duplicate() {
        let fix = ToolcallBadFilepathFix::new(true);
        fix.set_remove_duplicate(false);
        // Should not panic, setting works
    }

    #[test]
    fn test_is_valid_json() {
        let fix = ToolcallBadFilepathFix::new(true);

        assert!(fix.is_valid_json("{}"));
        assert!(fix.is_valid_json("{\"key\": \"value\"}"));
        assert!(fix.is_valid_json("[]"));
        assert!(!fix.is_valid_json("invalid"));
        assert!(!fix.is_valid_json("{broken"));
    }

    #[test]
    fn test_is_malformed() {
        let fix = ToolcallBadFilepathFix::new(true);

        // Valid JSON with filePath - not malformed
        assert!(!fix.is_malformed(r#"{"filePath": "/path"}"#));

        // Invalid JSON with filePath - malformed
        assert!(fix.is_malformed(r#"{"filePath": "/path" broken"#));

        // Valid JSON without filePath - not malformed (no filePath to check)
        assert!(!fix.is_malformed(r#"{"other": "value"}"#));
    }

    #[test]
    fn test_fix_arguments_empty() {
        let fix = ToolcallBadFilepathFix::new(true);

        let empty = "{}";
        let fixed = fix.fix_arguments(empty);
        assert_eq!(fixed, "{}");
    }

    #[test]
    fn test_fix_arguments_complex_valid() {
        let fix = ToolcallBadFilepathFix::new(true);

        let valid = r#"{"content":"some code","filePath":"/home/user/file.txt"}"#;
        let fixed = fix.fix_arguments(valid);
        // Should return valid JSON (might be reformatted)
        assert!(fix.is_valid_json(&fixed));
    }

    #[test]
    fn test_applies_no_choices() {
        let fix = ToolcallBadFilepathFix::new(true);

        let response = serde_json::json!({"other": "data"});
        assert!(!fix.applies(&response));
    }

    #[test]
    fn test_applies_no_tool_calls() {
        let fix = ToolcallBadFilepathFix::new(true);

        let response = serde_json::json!({
            "choices": [{
                "message": {"content": "Hello"}
            }]
        });
        assert!(!fix.applies(&response));
    }

    #[test]
    fn test_applies_valid_tool_call() {
        let fix = ToolcallBadFilepathFix::new(true);

        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "write",
                            "arguments": "{\"content\":\"code\",\"filePath\":\"/path\"}"
                        }
                    }]
                }
            }]
        });
        // Valid JSON - doesn't apply
        assert!(!fix.applies(&response));
    }

    #[test]
    fn test_applies_malformed_tool_call() {
        let fix = ToolcallBadFilepathFix::new(true);

        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "write",
                            "arguments": "{\"filePath\":\"/path\",\"filePath\"/broken\"}"
                        }
                    }]
                }
            }]
        });
        assert!(fix.applies(&response));
    }

    #[test]
    fn test_apply_no_changes_needed() {
        let fix = ToolcallBadFilepathFix::new(true);

        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "Hello",
                    "tool_calls": null
                }
            }]
        });

        let (result, action) = fix.apply(response.clone());
        assert_eq!(result, response);
        assert!(!action.detected());
    }

    #[test]
    fn test_apply_fixes_malformed() {
        let fix = ToolcallBadFilepathFix::new(true);

        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "write",
                            "arguments": "{\"content\":\"code\",\"filePath\":\"/path\",\"filePath\"/broken\"}"
                        }
                    }]
                }
            }]
        });

        let (result, action) = fix.apply(response);
        let args = result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert!(fix.is_valid_json(args));
        assert!(action.detected());
    }

    #[test]
    fn test_apply_stream_no_delta() {
        let fix = ToolcallBadFilepathFix::new(true);

        let chunk = serde_json::json!({
            "choices": [{
                "message": {"content": "test"}
            }]
        });

        let (result, action) = fix.apply_stream(chunk.clone());
        assert_eq!(result, chunk);
        assert!(!action.detected());
    }

    #[test]
    fn test_apply_stream_with_delta() {
        let fix = ToolcallBadFilepathFix::new(true);

        let chunk = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "function": {
                            "name": "write",
                            "arguments": "{\"filePath\":\"/path\",\"filePath\"/broken\"}"
                        }
                    }]
                }
            }]
        });

        let (result, action) = fix.apply_stream(chunk);
        let args = result["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert!(fix.is_valid_json(args));
        assert!(action.detected());
    }

    #[test]
    fn test_fix_malformed_json_no_filepath() {
        let fix = ToolcallBadFilepathFix::new(true);

        // Malformed JSON but no filePath - should still try to fix
        let malformed = r#"{"key": "value" broken"#;
        // This doesn't contain filePath, so is_malformed returns false
        assert!(!fix.is_malformed(malformed));
    }

    #[test]
    fn test_fix_keep_duplicate_mode() {
        let fix = ToolcallBadFilepathFix::new(false); // Don't remove duplicate

        // This tests the non-removal path
        let malformed = r#"{"filePath":"/path","filePath"/broken"}"#;
        let fixed = fix.fix_arguments(malformed);
        // Should still produce valid JSON (via aggressive fix if needed)
        assert!(
            fix.is_valid_json(&fixed) || fixed == "{}",
            "Fixed output should be valid JSON or empty object"
        );
    }

    #[test]
    fn test_multiple_tool_calls() {
        let fix = ToolcallBadFilepathFix::new(true);

        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [
                        {
                            "function": {
                                "name": "read",
                                "arguments": "{\"filePath\":\"/valid/path\"}"
                            }
                        },
                        {
                            "function": {
                                "name": "write",
                                "arguments": "{\"filePath\":\"/path\",\"filePath\"/broken\"}"
                            }
                        }
                    ]
                }
            }]
        });

        assert!(fix.applies(&response));
        let (result, action) = fix.apply(response);

        // First tool call should be unchanged
        let args1 = result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert!(fix.is_valid_json(args1));

        // Second tool call should be fixed
        let args2 = result["choices"][0]["message"]["tool_calls"][1]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert!(fix.is_valid_json(args2));
        assert!(action.detected());
    }

    #[test]
    fn test_escaped_characters() {
        let fix = ToolcallBadFilepathFix::new(true);

        let valid = r#"{"content":"line1\nline2","filePath":"/path/to/file"}"#;
        assert!(!fix.is_malformed(valid));

        let fixed = fix.fix_arguments(valid);
        assert!(fix.is_valid_json(&fixed));
    }

    #[test]
    fn test_accumulated_streaming_fix() {
        use super::ToolCallAccumulator;

        let fix = ToolcallBadFilepathFix::new(true);
        let mut accumulator = ToolCallAccumulator::new();

        // Simulate streaming chunks where the malformed pattern spans multiple chunks
        // The accumulated arguments will form: {"content":"code","filePath":"/path","filePath":"/path"}
        // which has duplicate filePath keys

        // Chunk 1: {"content":"code",
        let chunk1 = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"content\":\"code\","}}]}}]}"#;
        // Chunk 2: "filePath":"/path",
        let chunk2 = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"filePath\":\"/path\","}}]}}]}"#;
        // Chunk 3: "filePath":"/path"} - duplicate key triggers the fix
        let chunk3 = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"filePath\":\"/path\"}"}}]}}]}"#;

        // Apply fix to each chunk
        let (result1, _) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk1).unwrap(),
            &mut accumulator,
        );
        let (result2, _) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk2).unwrap(),
            &mut accumulator,
        );
        let (result3, action3) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk3).unwrap(),
            &mut accumulator,
        );

        // First two chunks should still have their original content (accumulating)
        let args1 = result1["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert_eq!(args1, r#"{"content":"code","#);

        let args2 = result2["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert_eq!(args2, r#""filePath":"/path","#);

        // The third chunk should have the completion delta (not full JSON)
        let args3 = result3["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();

        // Simulate client-side accumulation (what Claude Code does)
        let mut client_accumulated = String::new();
        client_accumulated.push_str(args1);
        client_accumulated.push_str(args2);
        client_accumulated.push_str(args3);

        // The final accumulated result should be valid JSON
        assert!(
            fix.is_valid_json(&client_accumulated),
            "Expected valid JSON in client accumulation but got: {}",
            client_accumulated
        );

        // Verify action was detected
        assert!(action3.detected());

        // Verify accumulator was cleared after fix
        assert!(accumulator.get(0).is_none());
    }

    #[test]
    fn test_accumulated_streaming_valid_json_clears_accumulator() {
        use super::ToolCallAccumulator;

        let fix = ToolcallBadFilepathFix::new(true);
        let mut accumulator = ToolCallAccumulator::new();

        // Simulate streaming chunks that form valid JSON
        let chunk1 = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"con"}}]}}]}"#;
        let chunk2 = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"tent\":\"code\"}"}}]}}]}"#;

        // Apply fix to first chunk
        let _result1 = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk1).unwrap(),
            &mut accumulator,
        );

        // Accumulator should have content
        assert!(accumulator.get(0).is_some());

        // Apply fix to second chunk - now we have valid JSON
        let _result2 = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk2).unwrap(),
            &mut accumulator,
        );

        // Accumulator should be cleared after valid JSON detected
        assert!(
            accumulator.get(0).is_none(),
            "Accumulator should be cleared after valid JSON"
        );
    }

    #[test]
    fn test_accumulated_streaming_multiple_tool_calls() {
        use super::ToolCallAccumulator;

        let fix = ToolcallBadFilepathFix::new(true);
        let mut accumulator = ToolCallAccumulator::new();

        // Two different tool calls with different indices
        // Tool 0 will complete normally, tool 1 will have malformed filePath
        let chunk1 = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"a\":"}},{"index":1,"function":{"arguments":"{\"filePath\":"}}]}}]}"#;
        let chunk2 = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"b\"}"}},{"index":1,"function":{"arguments":"\"/first/path\","}}]}}]}"#;
        // Tool 1 now gets duplicate filePath with proper quotes (pattern the fix can handle)
        let chunk3 = r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"\"filePath\":\"/second/path\"}"}}]}}]}"#;

        fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk1).unwrap(),
            &mut accumulator,
        );
        fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk2).unwrap(),
            &mut accumulator,
        );

        // Tool 0 should be cleared (valid JSON), tool 1 should still be accumulating
        assert!(accumulator.get(0).is_none());
        assert!(accumulator.get(1).is_some());

        let (result3, action3) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk3).unwrap(),
            &mut accumulator,
        );

        // Tool 1 should be fixed - the accumulated content has duplicate filePath
        // {"filePath":"/first/path","filePath":"/second/path"}
        // The chunk contains a delta, we need to check if action was detected
        assert!(action3.detected(), "Fix should be detected for tool 1");

        // Both should be cleared now
        assert!(accumulator.get(0).is_none());
        assert!(accumulator.get(1).is_none());
    }

    #[test]
    fn test_user_reported_malformed_pattern() {
        // This tests the exact pattern reported by the user:
        // {"content":"...","filePath":"/path","filePath"/path"}
        // Where the second filePath is missing the colon
        let fix = ToolcallBadFilepathFix::new(true);

        let malformed = r#"{"content":"some code","filePath":"/home/user/file.c","filePath"/home/user/file.c"}"#;

        // Verify it's detected as malformed
        assert!(fix.is_malformed(malformed), "Should detect malformed pattern");

        // Verify the fix produces valid JSON
        let fixed = fix.fix_arguments(malformed);
        assert!(
            fix.is_valid_json(&fixed),
            "Fixed output should be valid JSON, got: {}",
            fixed
        );

        // Verify the fixed output contains the expected content
        let parsed: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        assert_eq!(parsed["content"], "some code");
        assert_eq!(parsed["filePath"], "/home/user/file.c");
    }

    #[test]
    fn test_streaming_accumulated_malformed_pattern() {
        use super::ToolCallAccumulator;

        // Test the user's exact pattern accumulated across streaming chunks
        let fix = ToolcallBadFilepathFix::new(true);
        let mut accumulator = ToolCallAccumulator::new();

        // Chunk 1: {"content":"some code",
        let chunk1 = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"content\":\"some code\","}}]}}]}"#;
        // Chunk 2: "filePath":"/home/user/file.c",
        let chunk2 = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"filePath\":\"/home/user/file.c\","}}]}}]}"#;
        // Chunk 3: "filePath"/home/user/file.c"} - malformed, missing colon
        let chunk3 = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"filePath\"/home/user/file.c\"}"}}]}}]}"#;

        let (result1, _) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk1).unwrap(),
            &mut accumulator,
        );
        let (result2, _) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk2).unwrap(),
            &mut accumulator,
        );

        // Before the final chunk, we should be accumulating
        assert!(accumulator.get(0).is_some());

        let (result3, action3) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk3).unwrap(),
            &mut accumulator,
        );

        // Extract deltas from each chunk
        let delta1 = result1["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        let delta2 = result2["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        let delta3 = result3["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();

        // Simulate client-side accumulation
        let mut client_accumulated = String::new();
        client_accumulated.push_str(delta1);
        client_accumulated.push_str(delta2);
        client_accumulated.push_str(delta3);

        // Should be valid JSON after accumulation
        assert!(
            fix.is_valid_json(&client_accumulated),
            "Expected valid JSON after client accumulation, got: {}",
            client_accumulated
        );

        // Verify action was detected
        assert!(action3.detected());

        // Verify content
        let parsed: serde_json::Value = serde_json::from_str(&client_accumulated).unwrap();
        assert_eq!(parsed["content"], "some code");
        assert_eq!(parsed["filePath"], "/home/user/file.c");

        // Accumulator should be cleared
        assert!(accumulator.get(0).is_none());
    }

    // ============================================================
    // PHASE 1: Test-First Fix - Tests from the Plan
    // ============================================================

    #[test]
    fn test_user_exact_error_pattern_non_streaming() {
        let fix = ToolcallBadFilepathFix::new(true);

        // EXACT pattern from user's error
        let malformed = "{\"content\":\"#!/usr/bin/perl\\n# test\\n\",\"filePath\":\"/home/iphands/prog/slop/trash/primes.pl\",\"filePath\"/home/iphands/prog/slop/llama-proxy/trash/primes.pl\"}";

        println!("Testing malformed: {}", malformed);

        // Step 1: Verify it's detected as malformed
        assert!(fix.is_malformed(malformed), "Should detect as malformed");

        // Step 2: Apply the fix
        let fixed = fix.fix_arguments(malformed);
        println!("Fixed result: {}", fixed);

        // Step 3: Verify the fixed version is valid JSON
        assert!(fix.is_valid_json(&fixed), "Fixed version MUST be valid JSON, got: {}", fixed);

        // Step 4: Parse and verify structure
        let parsed: serde_json::Value = serde_json::from_str(&fixed).expect("Should parse as JSON");
        assert!(parsed.get("content").is_some(), "Should have content field");
        assert!(parsed.get("filePath").is_some(), "Should have filePath field");

        // Step 5: Verify we kept the FIRST filePath value
        let filepath = parsed["filePath"].as_str().unwrap();
        assert_eq!(filepath, "/home/iphands/prog/slop/trash/primes.pl",
                   "Should keep first filePath value, got: {}", filepath);
    }

    #[test]
    fn test_user_exact_error_pattern_streaming() {
        use super::ToolCallAccumulator;

        let fix = ToolcallBadFilepathFix::new(true);
        let mut accumulator = ToolCallAccumulator::new();

        // Simulate how llama.cpp would stream this malformed content
        // Chunk 1: Start of JSON with content
        let chunk1 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"content\\\":\\\"#!/usr/bin/perl\\\\n# test\\\\n\\\",\"}}]}}]}";

        // Chunk 2: First filePath (correct)
        let chunk2 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\":\\\"/home/iphands/prog/slop/trash/primes.pl\\\",\"}}]}}]}";

        // Chunk 3: Second filePath (MALFORMED - missing colon)
        let chunk3 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\"/home/iphands/prog/slop/llama-proxy/trash/primes.pl\\\"}\"}}]}}]}";

        // Process each chunk
        let (_result1, _action1) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk1).unwrap(),
            &mut accumulator,
        );

        let (_result2, _action2) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk2).unwrap(),
            &mut accumulator,
        );

        let (result3, action3) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk3).unwrap(),
            &mut accumulator,
        );

        // Verify the fix was detected and applied
        assert!(action3.detected(), "Should detect malformed content on chunk 3");

        // Extract the deltas from each chunk
        let delta1 = _result1["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        let delta2 = _result2["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        let delta3 = result3["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();

        println!("Delta 1: {}", delta1);
        println!("Delta 2: {}", delta2);
        println!("Delta 3 (fix applied): {}", delta3);

        // Simulate client-side accumulation (what Claude Code does)
        let mut client_accumulated = String::new();
        client_accumulated.push_str(delta1);
        client_accumulated.push_str(delta2);
        client_accumulated.push_str(delta3);

        println!("Final client accumulated: {}", client_accumulated);

        // Verify the client's accumulated content is valid JSON
        assert!(fix.is_valid_json(&client_accumulated),
                "Final client accumulated arguments MUST be valid JSON, got: {}", client_accumulated);

        // Parse and verify structure
        let parsed: serde_json::Value = serde_json::from_str(&client_accumulated).unwrap();
        assert!(parsed.get("content").is_some());
        assert!(parsed.get("filePath").is_some());
        assert_eq!(parsed["filePath"], "/home/iphands/prog/slop/trash/primes.pl");
    }

    #[test]
    fn test_user_pattern_in_full_response() {
        let fix = ToolcallBadFilepathFix::new(true);

        // Full non-streaming response with user's exact error
        let response = serde_json::json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "qwen3-coder",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "write",
                            "arguments": "{\"content\":\"#!/usr/bin/perl\\n# test\\n\",\"filePath\":\"/home/iphands/prog/slop/trash/primes.pl\",\"filePath\"/home/iphands/prog/slop/llama-proxy/trash/primes.pl\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150
            }
        });

        println!("Testing full response with malformed tool call");

        // Verify the fix applies
        assert!(fix.applies(&response), "Fix should apply to this response");

        // Apply the fix
        let (fixed_response, action) = fix.apply(response);

        // Verify fix was applied
        assert!(action.detected(), "Should detect and fix the malformed content");

        // Extract the fixed arguments
        let fixed_args = fixed_response["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();

        println!("Fixed arguments: {}", fixed_args);

        // CRITICAL: Verify the fixed arguments are valid JSON
        let parsed_result = serde_json::from_str::<serde_json::Value>(fixed_args);
        assert!(parsed_result.is_ok(),
                "Fixed arguments MUST be valid JSON! Parse error: {:?}\nArguments: {}",
                parsed_result.err(),
                fixed_args);

        // Verify structure
        let parsed = parsed_result.unwrap();
        assert!(parsed.get("content").is_some());
        assert!(parsed.get("filePath").is_some());
        assert_eq!(parsed["filePath"], "/home/iphands/prog/slop/trash/primes.pl");
    }

    #[test]
    fn test_simpler_malformed_patterns() {
        let fix = ToolcallBadFilepathFix::new(true);

        // Test various malformed patterns
        let test_cases = vec![
            // Original simple case
            (r#"{"filePath":"/path1","filePath"/path2"}"#, "simple missing colon"),
            // With content field
            (r#"{"content":"code","filePath":"/path1","filePath"/path2"}"#, "with content"),
            // Duplicate with colon (both valid keys)
            (r#"{"filePath":"/path1","filePath":"/path2"}"#, "duplicate valid keys"),
        ];

        for (malformed, description) in test_cases {
            println!("\nTesting case: {}", description);
            println!("Input: {}", malformed);

            assert!(fix.is_malformed(malformed),
                    "Case '{}' should be detected as malformed", description);

            let fixed = fix.fix_arguments(malformed);
            println!("Fixed: {}", fixed);

            assert!(fix.is_valid_json(&fixed),
                    "Case '{}' should produce valid JSON after fix, got: {}",
                    description, fixed);
        }
    }

    #[test]
    fn test_client_side_accumulation_behavior() {
        // This test simulates what Claude Code sees on its side
        // Claude Code accumulates the delta.tool_calls[].function.arguments strings
        use super::ToolCallAccumulator;

        let fix = ToolcallBadFilepathFix::new(true);
        let mut accumulator = ToolCallAccumulator::new();

        // Chunk 1: {"content":"...","
        let chunk1 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"content\\\":\\\"#!/usr/bin/perl\\\\n# test\\\\n\\\",\"}}]}}]}";
        // Chunk 2: "filePath":"/path1",
        let chunk2 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\":\\\"/home/iphands/prog/slop/trash/primes.pl\\\",\"}}]}}]}";
        // Chunk 3: "filePath"/path2"} (malformed!)
        let chunk3 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\"/home/iphands/prog/slop/llama-proxy/trash/primes.pl\\\"}\"}}]}}]}";

        // Simulate what CLAUDE CODE accumulates
        let mut client_accumulated = String::new();

        // Process chunk 1
        let (result1, _action1) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk1).unwrap(),
            &mut accumulator,
        );
        let delta1 = result1["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        println!("Chunk 1 delta sent to client: {}", delta1);
        client_accumulated.push_str(delta1);
        println!("Client accumulated after chunk 1: {}", client_accumulated);

        // Process chunk 2
        let (result2, _action2) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk2).unwrap(),
            &mut accumulator,
        );
        let delta2 = result2["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        println!("Chunk 2 delta sent to client: {}", delta2);
        client_accumulated.push_str(delta2);
        println!("Client accumulated after chunk 2: {}", client_accumulated);

        // Process chunk 3 - THIS IS WHERE THE FIX TRIGGERS
        let (result3, action3) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk3).unwrap(),
            &mut accumulator,
        );
        let delta3 = result3["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        println!("Chunk 3 delta sent to client: {}", delta3);
        println!("Fix detected: {}", action3.detected());
        client_accumulated.push_str(delta3);
        println!("Client accumulated after chunk 3: {}", client_accumulated);

        // CRITICAL TEST: What does Claude Code see?
        println!("\nFinal client-side accumulated content: {}", client_accumulated);
        println!("Is it valid JSON? {}", serde_json::from_str::<serde_json::Value>(&client_accumulated).is_ok());

        // THIS IS THE BUG: Claude Code will have corrupted accumulated content
        // because the proxy sent the FULL fixed JSON in chunk 3,
        // which Claude Code appended to what it already had from chunks 1-2
        assert!(
            serde_json::from_str::<serde_json::Value>(&client_accumulated).is_ok(),
            "CLIENT-SIDE ACCUMULATION FAILED! Claude Code sees: {}",
            client_accumulated
        );
    }
}
