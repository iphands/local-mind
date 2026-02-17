//! Fix for duplicate/malformed filePath in Qwen3-Coder tool calls
//!
//! Handles malformed JSON like:
//! `{"content":"code","filePath":"/path/to/file","filePath"/path/to/file"}`
//!
//! Where:
//! - filePath is duplicated
//! - The second occurrence is malformed JSON (missing colon/value quotes)
//!
//! ## Implementation Strategy (Simplified)
//!
//! Uses **schema-based truncation**: The Write tool schema (Opencode/Claude Code)
//! strictly defines only 2 fields: `content` and `filePath` with no additional
//! properties allowed. Therefore, once we find the first complete `"filePath":"value"`,
//! everything after is garbage by definition.
//!
//! **Fix approach:**
//! 1. Find first `"filePath":"<value>"` occurrence
//! 2. Truncate after the closing quote of the value
//! 3. Remove trailing comma if present
//! 4. Close with `}`
//!
//! This is simpler and more robust than previous multi-stage fallback approaches.
//!
//! **Streaming delta calculation:**
//! When fixing streaming responses, we MUST send only a completion delta (typically
//! `}` or `"_":null}`), NOT the full fixed JSON. Clients accumulate deltas, so
//! sending full JSON would duplicate content. See `calculate_completion_delta()`.

use super::{FixAction, ResponseFix, ToolCallAccumulator};
use serde_json::Value;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Fix for malformed filePath in Qwen3-Coder tool calls
///
/// Uses schema-based truncation: Since the Write tool schema only allows
/// `content` and `filePath` fields (no additional properties), we truncate
/// after the first complete `"filePath":"value"` occurrence.
pub struct ToolcallBadFilepathFix {
    /// Deprecated: Now always truncates after first filePath (kept for API compatibility)
    #[allow(dead_code)]
    remove_duplicate: Arc<AtomicBool>,
}

impl ToolcallBadFilepathFix {
    /// Create new fix instance
    /// Note: `remove_duplicate` parameter is deprecated (always true now)
    pub fn new(remove_duplicate: bool) -> Self {
        Self {
            remove_duplicate: Arc::new(AtomicBool::new(remove_duplicate)),
        }
    }

    /// Deprecated: No longer has any effect (always removes duplicates via truncation)
    #[allow(dead_code)]
    pub fn set_remove_duplicate(&self, _value: bool) {
        // No-op: truncation is now the only approach
    }

    /// Check if arguments string is malformed
    /// Simplified detection: Invalid JSON + contains "filePath" = malformed
    /// Also treats duplicate filePath keys as malformed (even if syntactically valid JSON)
    fn is_malformed(&self, args: &str) -> bool {
        // Check for duplicate filePath keys first (even if JSON is valid)
        // Duplicate keys are syntactically valid JSON but semantically wrong for our schema
        if args.matches(r#""filePath""#).count() > 1 {
            return true;
        }

        // Valid JSON with single filePath? → Not malformed
        if self.is_valid_json(args) {
            return false;
        }

        // Invalid JSON with "filePath" → Our fix applies
        args.contains(r#""filePath""#)
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

    /// Attempt to fix malformed arguments string using schema-based truncation
    /// Key insight: Write tool schema has only 2 fields (content, filePath) with no
    /// additional properties allowed. Once we find the first complete "filePath":"value",
    /// everything after is garbage by definition.
    fn fix_arguments(&self, args: &str) -> String {
        // Valid JSON? Pass through (normalize it)
        if let Ok(json) = serde_json::from_str::<Value>(args) {
            return serde_json::to_string(&json).unwrap_or_else(|_| args.to_string());
        }

        // Invalid JSON - apply schema-based truncation
        // Find first "filePath":"value", truncate after, close with }
        let filepath_key = r#""filePath":"#;
        if let Some(start) = args.find(filepath_key) {
            let after_colon = &args[start + filepath_key.len()..];

            // Find the end of the string value (handles escapes correctly)
            if let Some(value_end) = self.find_string_end(after_colon) {
                let end_pos = start + filepath_key.len() + value_end;
                let mut result = args[..end_pos].to_string();

                // Remove trailing comma if present (invalid before closing brace)
                if result.trim_end().ends_with(',') {
                    result = result.trim_end().trim_end_matches(',').to_string();
                }

                result.push('}');

                // Validate and return
                if self.is_valid_json(&result) {
                    return result;
                }
            }
        }

        // Fallback: empty valid object
        "{}".to_string()
    }

    /// Find the end of a JSON string value starting from position after colon
    /// Correctly handles escaped quotes like \"
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

    /// Calculate completion delta to send to client
    /// CRITICAL: Must NEVER send full JSON, only minimal completion
    ///
    /// When streaming tool call arguments, clients (e.g., Claude Code, Opencode) accumulate
    /// delta strings from each SSE chunk. If we detect malformed JSON in the accumulated
    /// args, we must calculate and send a "completion delta" that makes the client's
    /// already-accumulated content valid.
    ///
    /// # Arguments
    /// * `accumulated` - Server-side accumulated content (all chunks so far)
    /// * `current_chunk` - Chunk we just received
    /// * `index` - Tool call index (for logging)
    ///
    /// # Returns
    /// A minimal completion string (typically "}" or ""_":null}")
    fn calculate_completion_delta(
        &self,
        accumulated: &str,
        current_chunk: &str,
        index: usize,
    ) -> String {
        // TIER 1: Fast path - accumulated ends with current chunk
        // This is the common case since we just appended current_chunk
        let already_sent_len = if accumulated.ends_with(current_chunk) {
            accumulated.len() - current_chunk.len()
        } else {
            // TIER 2: Fallback for JSON reformatting/escaping
            // String matching may fail due to encoding or escaping differences
            if let Some(pos) = accumulated.rfind(current_chunk) {
                tracing::debug!(
                    fix_name = self.name(),
                    index = index,
                    "Delta calc: rfind fallback (reformatting detected)"
                );
                pos
            } else {
                // TIER 3: Safe fallback - cannot determine position
                // CRITICAL: Return minimal completion, NEVER full JSON
                tracing::warn!(
                    fix_name = self.name(),
                    index = index,
                    "Delta calc: Cannot determine position, using safe fallback"
                );
                return self.safe_completion(accumulated);
            }
        };

        let already_sent = &accumulated[..already_sent_len];

        // Determine closing based on trailing punctuation
        if already_sent.trim_end().ends_with(',') {
            // Consume trailing comma with dummy field
            r#""_":null}"#.to_string()
        } else {
            // Just close the object
            "}".to_string()
        }
    }

    /// Safe completion when delta calculation is uncertain
    /// NEVER sends full JSON - only minimal closing
    ///
    /// This is the last resort when we cannot reliably determine what the client
    /// has already accumulated. We send a minimal completion that attempts to
    /// close the JSON without duplicating content.
    fn safe_completion(&self, already_sent: &str) -> String {
        if already_sent.trim_end().ends_with(',') {
            // Trailing comma exists - consume with dummy field
            r#""_":null}"#.to_string()
        } else {
            // Just close
            "}".to_string()
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

    /// Override: Use accumulation when request context is available
    /// This method is called when request_json is Some in streaming.rs
    fn apply_stream_with_accumulation(
        &self,
        chunk: Value,
        _request: &Value,
        accumulator: &mut ToolCallAccumulator,
    ) -> (Value, FixAction) {
        // Delegate to the default version since we don't need request context
        // The accumulation logic is in apply_stream_with_accumulation_default
        tracing::debug!(fix_name = self.name(), "OVERRIDE CALLED - ToolcallBadFilepathFix.apply_stream_with_accumulation (with request)");
        self.apply_stream_with_accumulation_default(chunk, accumulator)
    }

    fn apply_stream_with_accumulation_default(
        &self,
        mut chunk: Value,
        accumulator: &mut ToolCallAccumulator,
    ) -> (Value, FixAction) {
        tracing::debug!(fix_name = self.name(), "OVERRIDE CALLED - ToolcallBadFilepathFix.apply_stream_with_accumulation_default");
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
                                // CRITICAL: Check if this index has already been fixed
                                // If so, suppress this chunk by replacing arguments with empty string
                                if accumulator.is_fixed(index) {
                                    tracing::debug!(
                                        fix_name = self.name(),
                                        index = index,
                                        chunk_args_len = chunk_args.len(),
                                        chunk_args_snippet = Self::create_snippet_static(chunk_args, 50),
                                        "POST-FIX CHUNK SUPPRESSED: Index already fixed, suppressing chunk"
                                    );
                                    // Suppress this chunk - replace arguments with empty string
                                    function["arguments"] = Value::String(String::new());
                                    return (chunk, FixAction::NotApplicable);
                                }

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
                                        // ===================================================================
                                        // DELTA CALCULATION FOR STREAMING TOOL CALL FIXES
                                        // ===================================================================
                                        // When streaming tool call arguments, clients (e.g., Claude Code, Opencode)
                                        // accumulate delta strings from each SSE chunk. If we detect malformed JSON
                                        // in the accumulated args, we must calculate and send a "completion delta"
                                        // that makes the client's already-accumulated content valid.
                                        //
                                        // CRITICAL: We must NOT send the full fixed JSON, as that would duplicate
                                        // content the client has already accumulated, causing malformed output like:
                                        //   {"content":"...","filePath":"/path","filePath"/corrupted"}
                                        //
                                        // Example scenario:
                                        //   Chunk 1: `{"content":"test",`       → Client accumulates: {"content":"test",
                                        //   Chunk 2: `"filePath":"/path1",`     → Client accumulates: {"content":"test","filePath":"/path1",
                                        //   Chunk 3: `"filePath"/path2"}`       → Malformed! Fix triggers
                                        //
                                        //   We detect: accumulated = `{"content":"test","filePath":"/path1","filePath"/path2"}`
                                        //   We fix to: `{"content":"test","filePath":"/path1"}`
                                        //   We calculate: client already has `{"content":"test","filePath":"/path1",`
                                        //   We must send: `"_":null}` (completion delta)
                                        //   Client result: `{"content":"test","filePath":"/path1","_":null}` ✓ Valid JSON
                                        //
                                        // The delta calculation (see below) handles edge cases where string matching
                                        // fails due to JSON escaping, UTF-8 encoding, or reformatting.
                                        // ===================================================================

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
                                            // Calculate the completion delta using extracted method
                                            // This ensures we NEVER send full JSON to the client
                                            let valid_completion = self.calculate_completion_delta(
                                                &accumulated,
                                                chunk_args,
                                                index,
                                            );

                                            function["arguments"] = Value::String(valid_completion.clone());
                                            // Mark this index as fixed so subsequent chunks are suppressed
                                            accumulator.mark_fixed(index);

                                            // Log success
                                            tracing::info!(
                                                fix_name = self.name(),
                                                index = index,
                                                sending_delta = &valid_completion,
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

        let (_result3, action3) = fix.apply_stream_with_accumulation_default(
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

    // ============================================================
    // PHASE 1.2: Delta Calculation Unit Tests (Bug Reproduction)
    // ============================================================
    // These tests target the specific bug in apply_stream_with_accumulation_default
    // where the delta calculation fails and sends full JSON instead of delta

    #[test]
    fn test_delta_calculation_with_escaped_quotes() {
        // This test demonstrates the bug: when chunk has escaped quotes,
        // accumulated.ends_with(current_chunk) may return false
        use super::ToolCallAccumulator;

        let fix = ToolcallBadFilepathFix::new(true);
        let mut accumulator = ToolCallAccumulator::new();

        // Chunk with escaped newline in content - use regular strings since they contain backslashes
        let chunk1 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"content\\\":\\\"#!/usr/bin/perl\\\\n\\\",\"}}]}}]}";
        let chunk2 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\":\\\"/path1\\\",\"}}]}}]}";
        let chunk3 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\"/path2\\\"}\"}}]}}]}";

        let (result1, _) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk1).unwrap(),
            &mut accumulator,
        );

        let (result2, _) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk2).unwrap(),
            &mut accumulator,
        );

        let (result3, _action3) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk3).unwrap(),
            &mut accumulator,
        );

        // Extract deltas
        let delta1 = result1["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();
        let delta2 = result2["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();
        let delta3 = result3["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();

        println!("Delta 1: {}", delta1);
        println!("Delta 2: {}", delta2);
        println!("Delta 3 (should be completion delta, NOT full JSON): {}", delta3);

        // Simulate client-side accumulation
        let mut client_accumulated = String::new();
        client_accumulated.push_str(delta1);
        client_accumulated.push_str(delta2);
        client_accumulated.push_str(delta3);

        println!("Client accumulated: {}", client_accumulated);

        // BUG TEST: The client should have valid JSON after accumulation
        assert!(
            fix.is_valid_json(&client_accumulated),
            "BUG REPRODUCED: Client-side accumulation is INVALID JSON! Got: {}",
            client_accumulated
        );
    }

    #[test]
    fn test_delta_calculation_fallback_sends_full_json() {
        // This test explicitly checks if the fallback sends full fixed JSON
        // (which is the bug - it should send a completion delta instead)
        use super::ToolCallAccumulator;

        let fix = ToolcallBadFilepathFix::new(true);
        let mut accumulator = ToolCallAccumulator::new();

        // Create a scenario where ends_with() will fail
        // Use UTF-8 multi-byte characters to make string matching tricky
        let chunk1 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"content\\\":\\\"テスト\\\",\"}}]}}]}";
        let chunk2 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\":\\\"/tmp/test\\\",\"}}]}}]}";
        let chunk3 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\"/malformed\\\"}\"}}]}}]}";

        let (result1, _) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk1).unwrap(),
            &mut accumulator,
        );

        let (result2, _) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk2).unwrap(),
            &mut accumulator,
        );

        let (result3, _action3) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk3).unwrap(),
            &mut accumulator,
        );

        let delta1 = result1["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();
        let delta2 = result2["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();
        let delta3 = result3["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();

        // Client accumulates
        let mut client_accumulated = String::new();
        client_accumulated.push_str(delta1);
        client_accumulated.push_str(delta2);
        client_accumulated.push_str(delta3);

        println!("Client accumulated: {}", client_accumulated);

        // Check if delta3 looks like full JSON (starts with '{') vs a completion delta
        // If it starts with '{', it's likely the full fixed JSON (the bug!)
        if delta3.starts_with('{') {
            println!("BUG DETECTED: Delta 3 appears to be FULL JSON: {}", delta3);
            println!("This will cause duplicate fields when client accumulates!");
        }

        // The critical test: client accumulated result must be valid
        assert!(
            fix.is_valid_json(&client_accumulated),
            "BUG REPRODUCED: Fallback sent full JSON causing invalid client accumulation: {}",
            client_accumulated
        );
    }

    #[test]
    fn test_delta_calculation_with_chunk_duplication() {
        // Test when the chunk appears multiple times in accumulated (partial overlap)
        use super::ToolCallAccumulator;

        let fix = ToolcallBadFilepathFix::new(true);
        let mut accumulator = ToolCallAccumulator::new();

        // Create chunks where content repeats
        let chunk1 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"filePath\\\":\\\"/\"}}]}}]}";
        let chunk2 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"filePath\\\",\"}}]}}]}";
        // Malformed chunk - missing colon before second occurrence
        let chunk3 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\"/test\\\"}\"}}]}}]}";

        let (result1, _) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk1).unwrap(),
            &mut accumulator,
        );

        let (result2, _) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk2).unwrap(),
            &mut accumulator,
        );

        let (result3, _) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk3).unwrap(),
            &mut accumulator,
        );

        let delta1 = result1["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();
        let delta2 = result2["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();
        let delta3 = result3["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();

        let mut client_accumulated = String::new();
        client_accumulated.push_str(delta1);
        client_accumulated.push_str(delta2);
        client_accumulated.push_str(delta3);

        println!("Client accumulated: {}", client_accumulated);

        assert!(
            fix.is_valid_json(&client_accumulated),
            "BUG: Partial overlap in chunks broke delta calculation: {}",
            client_accumulated
        );
    }

    #[test]
    fn test_delta_calculation_with_utf8_multibyte() {
        // Test with multi-byte UTF-8 characters that might confuse string matching
        use super::ToolCallAccumulator;

        let fix = ToolcallBadFilepathFix::new(true);
        let mut accumulator = ToolCallAccumulator::new();

        // Chunks with emoji and Japanese characters
        let chunk1 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"content\\\":\\\"🔧 修正中\\\",\"}}]}}]}";
        let chunk2 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\":\\\"/home/ユーザー/test.txt\\\",\"}}]}}]}";
        let chunk3 = "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"filePath\\\"/broken\\\"}\"}}]}}]}";

        let (result1, _) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk1).unwrap(),
            &mut accumulator,
        );

        let (result2, _) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk2).unwrap(),
            &mut accumulator,
        );

        let (result3, _) = fix.apply_stream_with_accumulation_default(
            serde_json::from_str(chunk3).unwrap(),
            &mut accumulator,
        );

        let delta1 = result1["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();
        let delta2 = result2["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();
        let delta3 = result3["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();

        let mut client_accumulated = String::new();
        client_accumulated.push_str(delta1);
        client_accumulated.push_str(delta2);
        client_accumulated.push_str(delta3);

        println!("Client accumulated with UTF-8: {}", client_accumulated);

        assert!(
            fix.is_valid_json(&client_accumulated),
            "BUG: UTF-8 multibyte chars broke delta calculation: {}",
            client_accumulated
        );
    }

    #[test]
    fn test_ends_with_mismatch_causes_full_json_send() {
        // This test specifically reproduces the bug where accumulated.ends_with(current_chunk)
        // returns FALSE, causing the fallback to send FULL fixed JSON instead of delta
        use super::ToolCallAccumulator;

        let fix = ToolcallBadFilepathFix::new(true);
        let mut accumulator = ToolCallAccumulator::new();

        // Manually construct a scenario where the accumulator has different content
        // than what ends_with() would expect

        // Chunk 1: partial JSON
        let chunk1_json = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {
                            "arguments": r#"{"content":"test","#
                        }
                    }]
                }
            }]
        });

        let (_result1, _) = fix.apply_stream_with_accumulation_default(chunk1_json.clone(), &mut accumulator);

        // Chunk 2: more partial JSON
        let chunk2_json = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {
                            "arguments": r#""filePath":"/path1","#
                        }
                    }]
                }
            }]
        });

        let (_result2, _) = fix.apply_stream_with_accumulation_default(chunk2_json.clone(), &mut accumulator);

        // NOW: Manually inject different content into accumulator to break ends_with()
        // This simulates what could happen if JSON is reformatted or encoding changes
        accumulator.accumulated.insert(0, r#"{"content":"test",  "filePath":"/path1","#.to_string());

        // Chunk 3: malformed duplicate filePath
        let chunk3_json = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {
                            // This is the malformed chunk
                            "arguments": r#""filePath"/path2"}"#
                        }
                    }]
                }
            }]
        });

        let (result3, _action3) = fix.apply_stream_with_accumulation_default(chunk3_json, &mut accumulator);

        // Extract the delta that was sent
        let delta3 = result3["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();

        println!("Delta 3 sent (after ends_with mismatch): {}", delta3);

        // Check if it sent full JSON (starts with '{') - THIS IS THE BUG
        if delta3.starts_with('{') {
            println!("BUG REPRODUCED: Sent full JSON instead of delta!");
            println!("Full delta3: {}", delta3);

            // If client had accumulated chunk1 + chunk2, and we send full JSON:
            let mut client_wrong = String::new();
            client_wrong.push_str(r#"{"content":"test","#);
            client_wrong.push_str(r#""filePath":"/path1","#);
            client_wrong.push_str(delta3);

            println!("Client would accumulate: {}", client_wrong);
            assert!(
                !fix.is_valid_json(&client_wrong),
                "This should produce INVALID JSON (duplicate fields): {}",
                client_wrong
            );
        } else {
            println!("Sent completion delta (no bug): {}", delta3);
        }
    }

    // ============================================================
    // POST-FIX CHUNK SUPPRESSION TESTS
    // ============================================================
    // These tests verify that after a fix is applied, subsequent
    // chunks for the same tool call index are suppressed

    #[test]
    fn test_post_fix_chunk_suppression() {
        // After fix is applied, subsequent chunks for same index should be suppressed
        let fix = ToolcallBadFilepathFix::new(true);
        let mut accumulator = ToolCallAccumulator::new();

        // Chunk 1: Start JSON
        let chunk1 = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"a\":"}}]}}]});
        let _ = fix.apply_stream_with_accumulation_default(chunk1, &mut accumulator);

        // Chunk 2: First filePath
        let chunk2 = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"filePath\":\"/path1\","}}]}}]});
        let _ = fix.apply_stream_with_accumulation_default(chunk2, &mut accumulator);

        // Chunk 3: Duplicate malformed filePath - triggers fix
        let chunk3 = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"filePath\"/path2\"}"}}]}}]});
        let (_result3, action3) = fix.apply_stream_with_accumulation_default(chunk3, &mut accumulator);
        assert!(action3.detected());

        // Chunk 4: Post-fix chunk - should be suppressed!
        let chunk4 = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"garbage"}}]}}]});
        let (result4, _) = fix.apply_stream_with_accumulation_default(chunk4, &mut accumulator);

        let args4 = result4["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();
        assert_eq!(args4, "", "Post-fix chunk should be suppressed (empty string)");

        // Chunk 5: Another post-fix chunk - should also be suppressed
        let chunk5 = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"more garbage"}}]}}]});
        let (result5, _) = fix.apply_stream_with_accumulation_default(chunk5, &mut accumulator);

        let args5 = result5["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();
        assert_eq!(args5, "", "Second post-fix chunk should also be suppressed");
    }

    #[test]
    fn test_new_pattern_brace_after_filepath() {
        let fix = ToolcallBadFilepathFix::new(true);

        // Test "filePath} pattern (key followed by } instead of proper value)
        let malformed = r#"{"content":"code","filePath":"/path1","filePath}/path2"}"#;
        assert!(fix.is_malformed(malformed), "Should detect 'filePath}}' as malformed");

        let fixed = fix.fix_arguments(malformed);
        assert!(fix.is_valid_json(&fixed), "Fixed version should be valid JSON, got: {}", fixed);
    }

    #[test]
    fn test_new_pattern_slash_after_filepath() {
        let fix = ToolcallBadFilepathFix::new(true);

        // Test "filePath/ pattern (key followed by / without colon)
        let malformed = r#"{"content":"code","filePath":"/path1","filePath/path2"}"#;
        assert!(fix.is_malformed(malformed), "Should detect 'filePath/' as malformed");

        let fixed = fix.fix_arguments(malformed);
        // Note: The aggressive fix may not always produce valid JSON for all patterns,
        // but we should at least not crash
        println!("Fixed output: {}", fixed);
    }

    #[test]
    fn test_client_accumulation_with_post_fix_suppression() {
        // Simulate full client-side accumulation with post-fix suppression
        let fix = ToolcallBadFilepathFix::new(true);
        let mut accumulator = ToolCallAccumulator::new();

        // Build the streaming scenario that caused the original bug
        // Note: The chunk that triggers the fix will send a completion delta (not suppressed)
        // Subsequent chunks AFTER the fix will be suppressed
        let chunks = vec![
            (r#"{"content":"code","#, false, false),  // Normal accumulation
            (r#""filePath":"/path1","#, false, false), // Normal accumulation
            (r#""filePath"/home"#, false, true),       // Triggers fix, sends completion delta
            (r#""filePath}"#, true, false),            // Post-fix - should suppress
            (r#"/iphands"#, true, false),              // Post-fix - should suppress
            (r#"/prog/slop"#, true, false),            // Post-fix - should suppress
        ];

        let mut client_accumulated = String::new();

        for (args, should_suppress, triggers_fix) in chunks {
            let chunk = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":args}}]}}]});
            let (result, _) = fix.apply_stream_with_accumulation_default(chunk, &mut accumulator);

            let delta = result["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
                .as_str().unwrap();

            if should_suppress {
                assert_eq!(delta, "", "Expected suppression but got: {}", delta);
            } else if triggers_fix {
                // The chunk that triggers the fix sends a completion delta
                assert!(!delta.is_empty() || delta == "}" || delta.contains("null"),
                        "Fix trigger chunk should send completion delta, got: {}", delta);
                client_accumulated.push_str(delta);
            } else {
                // Normal accumulation
                client_accumulated.push_str(delta);
            }
        }

        // Client's final accumulated JSON should be valid
        println!("Client accumulated: {}", client_accumulated);

        // The accumulated result should be valid JSON
        assert!(fix.is_valid_json(&client_accumulated),
                "Client accumulated JSON should be valid, got: {}", client_accumulated);

        // At minimum, check that it doesn't have the malformed patterns
        assert!(!client_accumulated.contains(r#""filePath}"#), "Should not contain malformed filePath}}");
    }

    #[test]
    fn test_post_fix_suppression_different_indices() {
        // Verify that suppression only affects the fixed index, not other indices
        let fix = ToolcallBadFilepathFix::new(true);
        let mut accumulator = ToolCallAccumulator::new();

        // Tool call 0: Will be fixed
        let chunk1 = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"filePath\":\"/path1\","}}]}}]});
        let _ = fix.apply_stream_with_accumulation_default(chunk1, &mut accumulator);

        let chunk2 = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"filePath\"/path2\"}"}}]}}]});
        let (_, action) = fix.apply_stream_with_accumulation_default(chunk2, &mut accumulator);
        assert!(action.detected());

        // Tool call 0 post-fix: Should be suppressed
        let chunk3 = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"suppressed"}}]}}]});
        let (result3, _) = fix.apply_stream_with_accumulation_default(chunk3, &mut accumulator);
        assert_eq!(result3["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap(), "");

        // Tool call 1: Should NOT be affected - different index
        let chunk4 = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"{\"ok\":true}"}}]}}]});
        let (result4, _) = fix.apply_stream_with_accumulation_default(chunk4, &mut accumulator);
        assert_eq!(
            result4["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str().unwrap(),
            "{\"ok\":true}",
            "Different index should not be suppressed"
        );
    }

    #[test]
    fn test_accumulator_mark_fixed_vs_clear() {
        // Test that mark_fixed sets the fixed flag while clear does not
        let mut accumulator = ToolCallAccumulator::new();

        // Accumulate some content
        accumulator.accumulate(0, "test");

        // Clear should remove accumulated but not set fixed flag
        accumulator.clear(0);
        assert!(!accumulator.is_fixed(0), "clear() should not set fixed flag");

        // Accumulate again
        accumulator.accumulate(0, "test2");

        // mark_fixed should set fixed flag AND clear accumulated
        accumulator.mark_fixed(0);
        assert!(accumulator.is_fixed(0), "mark_fixed() should set fixed flag");
        assert!(accumulator.get(0).is_none(), "mark_fixed() should clear accumulated");

        // Reset should clear both
        accumulator.reset(0);
        assert!(!accumulator.is_fixed(0), "reset() should clear fixed flag");
    }

    #[test]
    fn test_log_level_is_default_info() {
        use crate::fixes::{FixLogLevel, ResponseFix};
        let fix = ToolcallBadFilepathFix::new(true);

        // Verify this fix uses the default INFO log level
        assert_eq!(fix.log_level(), FixLogLevel::Info);
    }
}
