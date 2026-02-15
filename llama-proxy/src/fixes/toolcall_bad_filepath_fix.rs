//! Fix for duplicate/malformed filePath in Qwen3-Coder tool calls
//!
//! Handles malformed JSON like:
//! `{"content":"code","filePath":"/path/to/file","filePath"/path/to/file"}`
//!
//! Where:
//! - filePath is duplicated
//! - The second occurrence is malformed JSON (missing colon/value quotes)

use super::ResponseFix;
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
        // Quick check for common malformation patterns
        args.contains("filePath") && !self.is_valid_json(args)
    }

    /// Check if a string is valid JSON
    fn is_valid_json(&self, s: &str) -> bool {
        serde_json::from_str::<Value>(s).is_ok()
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

                    // Find what comes after (should be } or ,)
                    let remaining = &args[keep_until..];

                    // Skip any trailing content until we hit } or end
                    let end_pos = remaining
                        .find('}')
                        .map(|i| keep_until + i + 1)
                        .unwrap_or(args.len());

                    // Reconstruct: take content up to end of first filePath value, then close
                    let mut result = args[..end_pos].to_string();

                    // Ensure proper closing
                    if !result.ends_with('}') {
                        result.push('}');
                    }

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

    fn apply(&self, mut response: Value) -> Value {
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
                                    let fixed = self.fix_arguments(args);
                                    tracing::debug!(
                                        original = %args,
                                        fixed = %fixed,
                                        "Fixed malformed tool call arguments"
                                    );
                                    function["arguments"] = Value::String(fixed);
                                }
                            }
                        }
                    }
                }
            }
        }
        response
    }

    fn apply_stream(&self, mut chunk: Value) -> Value {
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
                                    function["arguments"] = Value::String(self.fix_arguments(args));
                                }
                            }
                        }
                    }
                }
            }
        }
        chunk
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
}
