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
//! ## Schema coupling is by-design config domain
//!
//! The truncation above is only sound because of a fact about the CLIENT's
//! tool configuration, not about models: the Write tool schemas published by
//! Opencode/Claude Code declare exactly two fields — `content` and `filePath`
//! — with `additionalProperties: false`. A schema-valid Write call therefore
//! cannot carry any key after `filePath`, which is what makes "everything after
//! the first complete filePath is garbage" a repair rule rather than a guess.
//!
//! This coupling to the `filePath`-shaped schema is deliberate and is config
//! domain, not a bug to generalize away: an operator whose deployed tool
//! schemas differ (extra fields, a different path key) opts this fix out with
//! `fixes.modules.toolcall_bad_filepath.enabled: false` rather than the fix
//! growing schema-awareness of its own. Triggering is still gated on the
//! structural duplicate/malformed predicates, so a well-formed Write call of
//! ANY shape passes through untouched.

use super::json_scan::{top_level_key_count, top_level_key_spans};
use super::registry::{truncate_snippet, SnippetLimit};
use super::{FixAction, FixError, ResponseFix};
use serde_json::Value;

/// Fix for malformed filePath in Qwen3-Coder tool calls
///
/// Uses schema-based truncation: Since the Write tool schema only allows
/// `content` and `filePath` fields (no additional properties), we truncate
/// after the first complete `"filePath":"value"` occurrence.
pub struct ToolcallBadFilepathFix {}

impl ToolcallBadFilepathFix {
    /// Create new fix instance
    pub fn new() -> Self {
        Self {}
    }

    /// Check if arguments are malformed: invalid JSON, or syntactically valid
    /// JSON carrying more than one depth-1 `filePath` key. The count is
    /// structural (`json_scan::top_level_key_count`), so content-embedded
    /// `"filePath"` literals and nested-only keys never register.
    fn is_malformed(&self, args: &str) -> bool {
        serde_json::from_str::<Value>(args).is_err() || top_level_key_count(args, "filePath") > 1
    }

    /// Check if a string is valid JSON
    fn is_valid_json(&self, s: &str) -> bool {
        serde_json::from_str::<Value>(s).is_ok()
    }

    /// Attempt to fix malformed arguments string using schema-based truncation
    /// Key insight: Write tool schema has only 2 fields (content, filePath) with no
    /// additional properties allowed. Once we find the first complete "filePath":"value",
    /// everything after is garbage by definition.
    ///
    /// `Ok` guarantees valid JSON. `Err` means the arguments are unparseable and
    /// no schema surgery can rebuild them — the old `"{}"` fallback destroyed the
    /// whole payload here and was reported as Fixed (B-H1); the fixer now reports
    /// the failure so callers keep the ORIGINAL arguments.
    fn fix_arguments(&self, args: &str) -> Result<String, FixError> {
        // Valid JSON? Normalize it — but resolve duplicate depth-1 `filePath`
        // keys FIRST-wins ([B-M2]), the winner the legacy streaming path gave
        // the client before task 29 deleted that machinery. Non-duplicate
        // fields keep this serde round-trip byte-for-byte; only the winner
        // choice is decided here.
        if let Ok(mut json) = serde_json::from_str::<Value>(args) {
            if let Some(first) = Self::first_wins_filepath(args) {
                if let Value::Object(map) = &mut json {
                    map.insert("filePath".to_string(), first);
                }
            }
            return serde_json::to_string(&json)
                .map_err(|e| FixError::Rebuild(format!("re-serializing parsed arguments failed: {e}")));
        }

        // Invalid JSON - apply schema-based truncation
        // Find first "filePath":"value", truncate after, close with }
        let filepath_key = r#""filePath":"#;
        let Some(start) = args.find(filepath_key) else {
            return Err(FixError::Parse(format!(
                "unparseable arguments carry no repairable \"filePath\":\"<value>\" pattern: {}",
                truncate_snippet(args, 200, SnippetLimit::Chars)
            )));
        };
        let after_colon = &args[start + filepath_key.len()..];

        // Find the end of the string value (handles escapes correctly)
        let Some(value_end) = self.find_string_end(after_colon) else {
            return Err(FixError::Parse(format!(
                "first filePath value has no closing quote: {}",
                truncate_snippet(args, 200, SnippetLimit::Chars)
            )));
        };
        let end_pos = start + filepath_key.len() + value_end;
        let mut result = args[..end_pos].to_string();

        // Remove trailing comma if present (invalid before closing brace)
        if result.trim_end().ends_with(',') {
            result = result.trim_end().trim_end_matches(',').to_string();
        }

        result.push('}');

        if self.is_valid_json(&result) {
            return Ok(result);
        }
        Err(FixError::Rebuild(format!(
            "schema-truncation candidate is still invalid JSON: {}",
            truncate_snippet(&result, 200, SnippetLimit::Chars)
        )))
    }

    /// Second parse pass over the RAW `args` using the task-23 structural
    /// scanner: ordered depth-1 `filePath` spans. serde_json's `Value` map
    /// collapses duplicate keys to the LAST occurrence, while the client
    /// consuming streamed deltas keeps the FIRST value — its bytes were
    /// forwarded before the object closes. [B-M2]
    ///
    /// `Some` only when a duplicate exists; `None` leaves serde's collapsed
    /// value in place, which is already the correct single winner. The value
    /// parse cannot fail behind the valid-JSON gate that guards this call —
    /// the scanner emits spans only for structurally sound input — and its
    /// degenerate arm merely preserves the pre-existing behavior anyway.
    fn first_wins_filepath(args: &str) -> Option<Value> {
        let spans = top_level_key_spans(args, "filePath");
        let (first, rest) = spans.split_first()?;
        if rest.is_empty() {
            return None;
        }
        args.get(first.value.clone()).and_then(|raw| serde_json::from_str(raw).ok())
    }

    /// Find the end of a JSON string value starting from position after colon.
    /// Returns a **byte offset** (not a char count) so callers can safely slice `&str`.
    fn find_string_end(&self, s: &str) -> Option<usize> {
        let mut iter = s.char_indices().peekable();

        // Skip whitespace and colon
        while let Some(&(_, c)) = iter.peek() {
            if c.is_whitespace() || c == ':' {
                iter.next();
            } else {
                break;
            }
        }

        // Expect opening quote
        match iter.next() {
            Some((_, '"')) => {}
            _ => return None,
        }

        // Find closing quote (handle escapes), return byte position after it
        while let Some((pos, c)) = iter.next() {
            if c == '\\' {
                iter.next(); // skip the escaped character
                continue;
            }
            if c == '"' {
                return Some(pos + '"'.len_utf8());
            }
        }

        None
    }

    /// Buffered repair, Result-typed: aborts with `Err` on the first malformed
    /// call whose arguments cannot be rebuilt. A caller that receives `Err`
    /// must DISCARD the returned-by-value response entirely (it may be
    /// partially repaired) — which is why [`Self::apply`] keeps an original.
    ///
    /// [`Self::apply`]: ToolcallBadFilepathFix::apply
    fn apply_checked(&self, mut response: Value) -> Result<(Value, FixAction), FixError> {
        let mut repairs: Vec<FixAction> = Vec::new();

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
                                    let fixed = self.fix_arguments(args)?;
                                    function["arguments"] = Value::String(fixed.clone());
                                    repairs.push(FixAction::fixed(&original, &fixed));
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok((response, FixAction::aggregate_repairs(&repairs)))
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

    fn apply(&self, response: Value) -> (Value, FixAction) {
        // Fail-safe mirror of apply_checked: FixError carries no response, so
        // an original is kept to return when the fixer reports irreparable
        // content. Callers therefore receive the ORIGINAL or a fully-fixed
        // response — never partial repairs, never the destructive "{}".
        // The clone is the fail-safe (task-31 precedent); apply() only runs
        // once applies() matched.
        let original = response.clone();
        match self.apply_checked(response) {
            Ok(outcome) => outcome,
            Err(error) => {
                let snippet = truncate_snippet(&original.to_string(), 200, SnippetLimit::Chars);
                (original, FixAction::failed(&snippet, &error.to_string()))
            }
        }
    }

    /// Report the fixer's structural failure as `Err` so the registry records
    /// a `FixAction::Failed` and forwards the ORIGINAL response untouched
    /// (task-31 fail-safe interface). [`Self::apply`] stays the infallible
    /// mirror for the context-free path.
    ///
    /// [`Self::apply`]: ResponseFix::apply
    fn apply_with_context(&self, response: Value, _request: &Value) -> Result<(Value, FixAction), FixError> {
        self.apply_checked(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fix_duplicate_filepath() {
        let fix = ToolcallBadFilepathFix::new();

        let malformed = r#"{"content":"code","filePath":"/path/to/file","filePath"/path/to/file"}"#;
        assert!(fix.is_malformed(malformed));

        let fixed = fix.fix_arguments(malformed).expect("truncation must repair");
        assert!(fix.is_valid_json(&fixed));
    }

    #[test]
    fn test_valid_json_passthrough() {
        let fix = ToolcallBadFilepathFix::new();

        let valid = r#"{"content":"code","filePath":"/path/to/file"}"#;
        assert!(!fix.is_malformed(valid));

        let fixed = fix.fix_arguments(valid).expect("valid JSON round-trips");
        assert_eq!(fixed, valid);
    }

    #[test]
    fn test_is_valid_json() {
        let fix = ToolcallBadFilepathFix::new();

        assert!(fix.is_valid_json("{}"));
        assert!(fix.is_valid_json("{\"key\": \"value\"}"));
        assert!(fix.is_valid_json("[]"));
        assert!(!fix.is_valid_json("invalid"));
        assert!(!fix.is_valid_json("{broken"));
    }

    #[test]
    fn test_is_malformed() {
        let fix = ToolcallBadFilepathFix::new();

        // Valid JSON with filePath - not malformed
        assert!(!fix.is_malformed(r#"{"filePath": "/path"}"#));

        // Invalid JSON with filePath - malformed
        assert!(fix.is_malformed(r#"{"filePath": "/path" broken"#));

        // Valid JSON without filePath - not malformed (no filePath to check)
        assert!(!fix.is_malformed(r#"{"other": "value"}"#));
    }

    #[test]
    fn test_fix_arguments_empty() {
        let fix = ToolcallBadFilepathFix::new();

        let empty = "{}";
        let fixed = fix.fix_arguments(empty).expect("valid JSON round-trips");
        assert_eq!(fixed, "{}");
    }

    #[test]
    fn test_fix_arguments_non_ascii_filepath() {
        let fix = ToolcallBadFilepathFix::new();

        // Non-ASCII path — previously would panic due to char/byte index mismatch
        let malformed = r#"{"filePath":"/日本語/file.txt","extra":"dropped","filePath":"/日本語/file.txt"}"#;
        let fixed = fix.fix_arguments(malformed).expect("dup keys round-trip");
        assert!(
            fix.is_valid_json(&fixed),
            "fix_arguments panicked or produced invalid JSON for non-ASCII path: {fixed}"
        );

        // German umlaut path
        let malformed2 = r#"{"filePath":"/über/lösung.rs","x":1,"filePath":"/über/lösung.rs"}"#;
        let fixed2 = fix.fix_arguments(malformed2).expect("dup keys round-trip");
        assert!(
            fix.is_valid_json(&fixed2),
            "fix_arguments panicked or produced invalid JSON for umlaut path: {fixed2}"
        );
    }

    #[test]
    fn test_fix_arguments_complex_valid() {
        let fix = ToolcallBadFilepathFix::new();

        let valid = r#"{"content":"some code","filePath":"/home/user/file.txt"}"#;
        let fixed = fix.fix_arguments(valid).expect("valid JSON round-trips");
        // Should return valid JSON (might be reformatted)
        assert!(fix.is_valid_json(&fixed));
    }

    #[test]
    fn test_applies_no_choices() {
        let fix = ToolcallBadFilepathFix::new();

        let response = serde_json::json!({"other": "data"});
        assert!(!fix.applies(&response));
    }

    #[test]
    fn test_applies_no_tool_calls() {
        let fix = ToolcallBadFilepathFix::new();

        let response = serde_json::json!({
            "choices": [{
                "message": {"content": "Hello"}
            }]
        });
        assert!(!fix.applies(&response));
    }

    #[test]
    fn test_applies_valid_tool_call() {
        let fix = ToolcallBadFilepathFix::new();

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
        let fix = ToolcallBadFilepathFix::new();

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
        let fix = ToolcallBadFilepathFix::new();

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
        let fix = ToolcallBadFilepathFix::new();

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
    fn test_fix_malformed_json_no_filepath() {
        let fix = ToolcallBadFilepathFix::new();

        // Malformed JSON and no filePath. Task 24 (B-H4): ANY unparseable
        // arguments are malformed — detection is `from_str::<Value>(args).is_err()
        // || top_level_key_count(...) > 1` and no longer requires an intact
        // `"filePath"` substring (the old heuristic's false negative let a broken
        // key like `{"filePath: ...` slip through unfixed). The original comment
        // of this test already stated the desired behavior ("should still try to
        // fix"); its old assertion pinned the substring heuristic, not the intent.
        let malformed = r#"{"key": "value" broken"#;
        assert!(fix.is_malformed(malformed), "invalid JSON is malformed");

        // Task 22 (B-H1): the OLD shape of this test asserted a valid-JSON
        // return, pinning the "{}" fallback that destroyed the payload while
        // apply() reported it as Fixed. The behavior the test's comment always
        // demanded ("still try to fix") now means: repair it or report Err —
        // never fabricate content.
        let outcome = fix.fix_arguments(malformed);
        assert!(
            matches!(outcome, Err(FixError::Parse(_))),
            "unparseable arguments without a repair pattern must Err, got: {outcome:?}"
        );
    }

    #[test]
    fn test_fix_keep_duplicate_mode() {
        let fix = ToolcallBadFilepathFix::new();
        let malformed = r#"{"filePath":"/path","filePath"/broken"}"#;
        let fixed = fix
            .fix_arguments(malformed)
            .expect("first filePath is intact, truncation must repair");
        assert!(fix.is_valid_json(&fixed), "Fixed output should be valid JSON");
    }

    #[test]
    fn test_multiple_tool_calls() {
        let fix = ToolcallBadFilepathFix::new();

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
        let fix = ToolcallBadFilepathFix::new();

        let valid = r#"{"content":"line1\nline2","filePath":"/path/to/file"}"#;
        assert!(!fix.is_malformed(valid));

        let fixed = fix.fix_arguments(valid).expect("valid JSON round-trips");
        assert!(fix.is_valid_json(&fixed));
    }

    #[test]
    fn test_user_reported_malformed_pattern() {
        // This tests the exact pattern reported by the user:
        // {"content":"...","filePath":"/path","filePath"/path"}
        // Where the second filePath is missing the colon
        let fix = ToolcallBadFilepathFix::new();

        let malformed = r#"{"content":"some code","filePath":"/home/user/file.c","filePath"/home/user/file.c"}"#;

        // Verify it's detected as malformed
        assert!(fix.is_malformed(malformed), "Should detect malformed pattern");

        // Verify the fix produces valid JSON
        let fixed = fix.fix_arguments(malformed).expect("truncation must repair");
        assert!(fix.is_valid_json(&fixed), "Fixed output should be valid JSON, got: {}", fixed);

        // Verify the fixed output contains the expected content
        let parsed: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        assert_eq!(parsed["content"], "some code");
        assert_eq!(parsed["filePath"], "/home/user/file.c");
    }

    // ============================================================
    // PHASE 1: Test-First Fix - Tests from the Plan
    // ============================================================

    #[test]
    fn test_user_exact_error_pattern_non_streaming() {
        let fix = ToolcallBadFilepathFix::new();

        // EXACT pattern from user's error
        let malformed = "{\"content\":\"#!/usr/bin/perl\\n# test\\n\",\"filePath\":\"/home/iphands/prog/slop/trash/primes.pl\",\"filePath\"/home/iphands/prog/slop/llama-proxy/trash/primes.pl\"}";

        println!("Testing malformed: {}", malformed);

        // Step 1: Verify it's detected as malformed
        assert!(fix.is_malformed(malformed), "Should detect as malformed");

        // Step 2: Apply the fix
        let fixed = fix.fix_arguments(malformed).expect("truncation must repair");
        println!("Fixed result: {}", fixed);

        // Step 3: Verify the fixed version is valid JSON
        assert!(fix.is_valid_json(&fixed), "Fixed version MUST be valid JSON, got: {}", fixed);

        // Step 4: Parse and verify structure
        let parsed: serde_json::Value = serde_json::from_str(&fixed).expect("Should parse as JSON");
        assert!(parsed.get("content").is_some(), "Should have content field");
        assert!(parsed.get("filePath").is_some(), "Should have filePath field");

        // Step 5: Verify we kept the FIRST filePath value
        let filepath = parsed["filePath"].as_str().unwrap();
        assert_eq!(
            filepath, "/home/iphands/prog/slop/trash/primes.pl",
            "Should keep first filePath value, got: {}",
            filepath
        );
    }

    #[test]
    fn test_user_pattern_in_full_response() {
        let fix = ToolcallBadFilepathFix::new();

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
        assert!(
            parsed_result.is_ok(),
            "Fixed arguments MUST be valid JSON! Parse error: {:?}\nArguments: {}",
            parsed_result.err(),
            fixed_args
        );

        // Verify structure
        let parsed = parsed_result.unwrap();
        assert!(parsed.get("content").is_some());
        assert!(parsed.get("filePath").is_some());
        assert_eq!(parsed["filePath"], "/home/iphands/prog/slop/trash/primes.pl");
    }

    #[test]
    fn test_simpler_malformed_patterns() {
        let fix = ToolcallBadFilepathFix::new();

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

            assert!(
                fix.is_malformed(malformed),
                "Case '{}' should be detected as malformed",
                description
            );

            let fixed = fix.fix_arguments(malformed).expect("truncation must repair");
            println!("Fixed: {}", fixed);

            assert!(
                fix.is_valid_json(&fixed),
                "Case '{}' should produce valid JSON after fix, got: {}",
                description,
                fixed
            );
        }
    }

    #[test]
    fn test_new_pattern_brace_after_filepath() {
        let fix = ToolcallBadFilepathFix::new();

        // Test "filePath} pattern (key followed by } instead of proper value)
        let malformed = r#"{"content":"code","filePath":"/path1","filePath}/path2"}"#;
        assert!(fix.is_malformed(malformed), "Should detect 'filePath}}' as malformed");

        let fixed = fix.fix_arguments(malformed).expect("truncation must repair");
        assert!(
            fix.is_valid_json(&fixed),
            "Fixed version should be valid JSON, got: {}",
            fixed
        );
    }

    #[test]
    fn test_new_pattern_slash_after_filepath() {
        let fix = ToolcallBadFilepathFix::new();

        // Test "filePath/ pattern (key followed by / without colon)
        let malformed = r#"{"content":"code","filePath":"/path1","filePath/path2"}"#;
        assert!(fix.is_malformed(malformed), "Should detect 'filePath/' as malformed");

        let fixed = fix.fix_arguments(malformed).expect("truncation must repair");
        // Note: The aggressive fix may not always produce valid JSON for all patterns,
        // but we should at least not crash
        println!("Fixed output: {}", fixed);
    }

    #[test]
    fn test_log_level_is_default_info() {
        use crate::fixes::{FixLogLevel, ResponseFix};
        let fix = ToolcallBadFilepathFix::new();

        // Verify this fix uses the default INFO log level
        assert_eq!(fix.log_level(), FixLogLevel::Info);
    }

    // ---- Multibyte regression tests for the buffered `apply()` path ----
    //
    // These pin the fix's OWN char-safety on the schema-based truncation path
    // (`fix_arguments` -> `find_string_end` -> `&args[..end_pos]` byte slicing).
    // The task-2 stats truncator is irrelevant here: a panic or a wrong slice in
    // this module would fail these tests independently of that code.
    //
    // Every payload below is INVALID JSON whose SECOND `filePath` key is malformed
    // (missing colon/quotes), which is what routes `fix_arguments` through the
    // truncation path rather than the serde round-trip. `content` is placed BEFORE
    // `filePath` so the truncation preserves it, proving first-wins + content-kept.

    #[test]
    fn test_apply_buffered_cjk_multibyte_first_wins() {
        // Given a buffered response whose tool-call arguments hold CJK content and a
        // CJK filePath followed by a malformed duplicate filePath.
        let fix = ToolcallBadFilepathFix::new();
        let args = r#"{"content":"中文测试内容","filePath":"/x/中文测试目录/file.rs","filePath"/x/第二个坏路径"}"#;
        let response = serde_json::json!({
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "name": "write", "arguments": args }
                    }]
                }
            }]
        });
        assert!(fix.applies(&response), "CJK payload must be detected as malformed");

        // When the buffered fix runs. (A panic here fails the test = no-panic proof.)
        let (result, action) = fix.apply(response);

        // Then the rebuilt arguments are valid JSON, first-wins, and content survives.
        let args_out = result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .expect("arguments must remain a string");
        assert!(fix.is_valid_json(args_out), "CJK fix produced invalid JSON: {args_out}");
        let parsed: Value = serde_json::from_str(args_out).expect("fixed args must parse");
        assert_eq!(
            parsed["filePath"].as_str(),
            Some("/x/中文测试目录/file.rs"),
            "first (multibyte) filePath must win"
        );
        assert_eq!(
            parsed["content"].as_str(),
            Some("中文测试内容"),
            "multibyte content before filePath must be preserved"
        );
        assert!(matches!(action, FixAction::Fixed { .. }), "action must be Fixed");
    }

    #[test]
    fn test_apply_buffered_emoji_4byte_multibyte() {
        // Given arguments whose content and winning filePath contain 4-byte UTF-8 emoji.
        let fix = ToolcallBadFilepathFix::new();
        let args = r#"{"content":"写入🀄内容🎉","filePath":"/x/🀄目录/文件.rs","filePath"/x/坏🎉"}"#;
        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "name": "write", "arguments": args }
                    }]
                }
            }]
        });
        assert!(fix.applies(&response));

        // When (panic on a 4-byte char boundary would fail here).
        let (result, action) = fix.apply(response);

        // Then valid JSON, first-wins, emoji content preserved.
        let args_out = result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .expect("arguments must remain a string");
        assert!(fix.is_valid_json(args_out), "emoji fix produced invalid JSON: {args_out}");
        let parsed: Value = serde_json::from_str(args_out).expect("fixed args must parse");
        assert_eq!(parsed["filePath"].as_str(), Some("/x/🀄目录/文件.rs"));
        assert_eq!(parsed["content"].as_str(), Some("写入🀄内容🎉"));
        assert!(matches!(action, FixAction::Fixed { .. }));
    }

    #[test]
    fn test_apply_buffered_escaped_unicode_probe() {
        // Given arguments whose winning filePath carries \uXXXX escapes (raw backslash-u
        // inside the arguments string) followed by a malformed duplicate filePath.
        // \u4e2d\u6587\u4ef6 decodes to 中文件.
        let fix = ToolcallBadFilepathFix::new();
        let args = r#"{"content":"esc content","filePath":"/x/\u4e2d\u6587\u4ef6/file.rs","filePath"/x/bad"}"#;
        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "name": "write", "arguments": args }
                    }]
                }
            }]
        });
        assert!(fix.applies(&response));

        // When.
        let (result, action) = fix.apply(response);

        // Then the truncation kept the escape-sequence value intact and serde decodes it.
        let args_out = result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .expect("arguments must remain a string");
        assert!(
            fix.is_valid_json(args_out),
            "escaped-unicode fix produced invalid JSON: {args_out}"
        );
        let parsed: Value = serde_json::from_str(args_out).expect("fixed args must parse");
        assert_eq!(
            parsed["filePath"].as_str(),
            Some("/x/中文件/file.rs"),
            r"\uXXXX escapes must survive truncation and decode"
        );
        assert_eq!(parsed["content"].as_str(), Some("esc content"));
        assert!(matches!(action, FixAction::Fixed { .. }));
    }

    // ============================================================
    // TASK 24 (B-M1, B-H4): structural detection via json_scan
    // ============================================================
    // is_malformed must be exactly: invalid JSON OR more than one depth-1
    // `filePath` key. Content-embedded `"filePath"` literals, nested-only keys,
    // and escaped-quote decoys must NOT trigger; real duplicates (valid or
    // malformed) and any unparseable payload MUST trigger.

    #[test]
    fn test_content_embedded_filepath_literal_not_malformed() {
        // Given valid JSON whose content value carries a literal `{"filePath":`
        // decoy plus exactly one real top-level filePath key.
        let fix = ToolcallBadFilepathFix::new();
        let args = r#"{"content":"write it like {\"filePath\": \"x\"} please","filePath":"/real/path"}"#;
        serde_json::from_str::<Value>(args).expect("fixture must be valid JSON");

        // Then detection stays false (old substring heuristic counted 2 -> RED).
        assert!(!fix.is_malformed(args), "content-embedded decoy must not trigger");

        // Same for the closing-quote-completion decoy: the value ends in
        // `\"filePath` so its closing quote completes a raw `"filePath"`
        // substring; the old heuristic counted it plus the real key = 2 (RED).
        let completion_decoy = r#"{"content":"say \"filePath","filePath":"/real/path"}"#;
        serde_json::from_str::<Value>(completion_decoy).expect("decoy fixture must be valid JSON");
        assert!(
            !fix.is_malformed(completion_decoy),
            "value-completion `\"filePath\"` substring must not trigger"
        );

        // And the buffered path leaves the response untouched.
        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "name": "write", "arguments": args }
                    }]
                }
            }]
        });
        assert!(!fix.applies(&response), "applies() must not fire on a decoy");
        let (result, action) = fix.apply(response.clone());
        assert!(!action.detected(), "no fix action for a decoy");
        assert_eq!(result, response);
    }

    #[test]
    fn test_real_duplicate_top_level_keys_malformed() {
        // Given real duplicate top-level filePath keys, in both the
        // valid-JSON and the malformed (missing-colon) variant.
        let fix = ToolcallBadFilepathFix::new();
        let valid_dup = r#"{"filePath":"/path1","filePath":"/path2"}"#;
        serde_json::from_str::<Value>(valid_dup).expect("valid-JSON dup fixture");
        let malformed_dup = r#"{"content":"code","filePath":"/path","filePath"/path"}"#;

        // Then both are malformed (green before AND after the swap).
        assert!(fix.is_malformed(valid_dup), "valid JSON with dup keys is malformed");
        assert!(fix.is_malformed(malformed_dup), "malformed dup payload is malformed");
    }

    #[test]
    fn test_nested_only_filepath_not_malformed() {
        // Given a purely nested filePath key. Old heuristic already said false
        // here (count == 1, valid JSON) - pinned so it stays false under the
        // structural scanner.
        let fix = ToolcallBadFilepathFix::new();
        let args = r#"{"meta":{"filePath":"/x"},"content":"hi"}"#;
        serde_json::from_str::<Value>(args).expect("nested fixture must be valid JSON");
        assert!(!fix.is_malformed(args), "nested-only filePath must not trigger");
    }

    #[test]
    fn test_duplicate_inside_nested_not_malformed() {
        // Given duplicate filePath keys INSIDE a nested object and none at the
        // top level. Old heuristic counted 2 and triggered (RED); the depth-1
        // scanner sees zero top-level keys.
        let fix = ToolcallBadFilepathFix::new();
        let args = r#"{"meta":{"filePath":"/a","filePath":"/b"},"content":"x"}"#;
        serde_json::from_str::<Value>(args).expect("nested-dup fixture must be valid JSON");
        assert!(
            !fix.is_malformed(args),
            "duplicate keys below depth 1 are not this fix's schema violation"
        );
    }

    #[test]
    fn test_unparseable_garbage_malformed() {
        // Given unparseable payloads with and without a filePath literal.
        let fix = ToolcallBadFilepathFix::new();
        let garbage_no_key = r#"{"key": "value" broken"#;
        let garbage_with_key = r#"{"filePath": "/path" broken"#;

        // Then BOTH are malformed: validity alone carries the detection
        // (the no-key variant is the behavior flip pinned above).
        assert!(fix.is_malformed(garbage_no_key), "unparseable garbage is malformed");
        assert!(fix.is_malformed(garbage_with_key), "invalid filePath JSON is malformed");
    }

    #[test]
    fn test_escaped_quote_decoy_keys_not_malformed() {
        // Given escaped-quote decoys: a value ending in `\"filePath"` and a key
        // that merely CONTAINS the target between escaped quotes.
        let fix = ToolcallBadFilepathFix::new();
        let value_decoy = r#"{"msg":"he said \"filePath"}"#;
        let key_decoy = r#"{"say\"filePath\"x":1,"filePath":2}"#;
        serde_json::from_str::<Value>(value_decoy).expect("value decoy must be valid JSON");
        serde_json::from_str::<Value>(key_decoy).expect("key decoy must be valid JSON");

        // Then neither triggers: one (or zero) real depth-1 key each.
        assert!(!fix.is_malformed(value_decoy), "escaped-quote value must not trigger");
        assert!(!fix.is_malformed(key_decoy), "decoy key containing target must not trigger");
    }

    #[test]
    fn test_unicode_escape_key_counts_as_real_duplicate() {
        // Given a duplicate expressed as a `\u0050` Unicode escape key
        // (`file\u0050ath` decodes to `filePath`).
        let fix = ToolcallBadFilepathFix::new();
        let args = r#"{"file\u0050ath":"/a","filePath":"/b"}"#;
        serde_json::from_str::<Value>(args).expect("escaped-key fixture must be valid JSON");

        // Then it triggers: keys compare decoded (old literal heuristic missed
        // this duplicate entirely - the false-negative twin of B-M1).
        assert!(fix.is_malformed(args), "escaped-key duplicate must trigger");
    }

    #[test]
    fn test_multibyte_and_escaped_content_duplicates_malformed() {
        // Given real duplicates riding alongside multibyte and escaped-quote
        // content (the shapes that previously relied on miscounting).
        let fix = ToolcallBadFilepathFix::new();
        let cjk_dup = r#"{"content":"你好世界","filePath":"/路径/文件.rs","filePath":"/第二"}"#;
        let escaped_content_dup = r#"{"content":"x = \"filePath\"; y","filePath":"/a","filePath":"/b"}"#;
        serde_json::from_str::<Value>(cjk_dup).expect("CJK fixture must be valid JSON");
        serde_json::from_str::<Value>(escaped_content_dup).expect("escaped fixture must be valid JSON");

        // Then both trigger on the structural count of 2 (green AND after).
        assert!(fix.is_malformed(cjk_dup), "CJK duplicate payload must trigger");
        assert!(
            fix.is_malformed(escaped_content_dup),
            "escaped-quote content with real dups must trigger"
        );
    }

    // ============================================================
    // TASK 22 (B-H1): fail-safe on fixer errors
    // ============================================================
    // An unparseable-but-triggering payload whose schema surgery is impossible
    // must return Err (registry then logs Failed and forwards the ORIGINAL),
    // never the destructive "{}" fallback.

    fn fuzz_response(arguments: Option<&str>) -> Value {
        let call = match arguments {
            Some(args) => serde_json::json!({
                "index": 0,
                "function": { "name": "write", "arguments": args }
            }),
            None => serde_json::json!({
                "index": 0,
                "function": { "name": "write" }
            }),
        };
        serde_json::json!({
            "choices": [{
                "message": { "tool_calls": [call] }
            }]
        })
    }

    #[test]
    fn test_apply_with_context_unparseable_irreparable_returns_err() {
        use super::ResponseFix;

        let fix = ToolcallBadFilepathFix::new();
        let args = r#"{"content":"x",,"filePath":garbage}"#;
        let response = fuzz_response(Some(args));
        let request = serde_json::json!({});
        assert!(fix.applies(&response), "payload must trigger the fix");

        let outcome = fix.apply_with_context(response, &request);

        assert!(outcome.is_err(), "irreparable payload must Err, got {outcome:?}");
    }

    #[test]
    fn test_apply_unparseable_irreparable_keeps_original_arguments() {
        let fix = ToolcallBadFilepathFix::new();
        let args = r#"{"content":"x",,"filePath":garbage}"#;
        let response = fuzz_response(Some(args));

        let (result, action) = fix.apply(response);

        let args_out = result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .expect("arguments must stay a string");
        assert_eq!(args_out, args, "original arguments must be preserved verbatim");
        assert_ne!(
            args_out, "{}",
            "the destructive empty-object fallback must never reach clients"
        );
        assert!(
            matches!(action, FixAction::Failed { .. }),
            "irreparable payload must report Failed, got {action:?}"
        );
    }

    #[test]
    fn test_fuzz_malformed_payloads_never_panic_and_keep_original_or_valid() {
        let fix = ToolcallBadFilepathFix::new();
        let cjk_base = r#"{"content":"中文测试内容","filePath":"/路径/文件.rs","filePath"/坏路径"}"#;
        let cases: Vec<Option<&str>> = vec![
            Some(r#"{"content":"x",,"filePath":garbage}"#),
            Some(r#"{"content":"a"#),
            Some(&cjk_base[..cjk_base.char_indices().nth(10).map(|(i, _)| i).unwrap_or(cjk_base.len())]),
            Some(&cjk_base[..cjk_base.char_indices().nth(23).map(|(i, _)| i).unwrap_or(cjk_base.len())]),
            Some(r#"{"filePath":"/x/\ud800","y":1"#),
            Some(r#"{"file\u0050ath":"/a","filePath"/b"#),
            Some(""),
            Some("   {"),
            Some(r#"{"filePath":"#),
            None,
        ];

        for args in cases {
            let response = fuzz_response(args);
            let (result, _action) = fix.apply(response.clone());
            let args_out = result["choices"][0]["message"]["tool_calls"][0]
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(|a| a.as_str());
            match (args, args_out) {
                (None, None) => assert_eq!(result, response, "argument-less call must pass through"),
                (Some(input), Some(out)) => assert!(
                    out == input || fix.is_valid_json(out),
                    "output must be the original or valid JSON, got: {out}"
                ),
                _ => panic!("tool call slot mutated shape for input {args:?}: {result}"),
            }
        }
    }

    #[test]
    fn test_fix_arguments_candidate_invalid_reports_rebuild_error() {
        let fix = ToolcallBadFilepathFix::new();
        // Pattern found, value terminated, but the truncated candidate
        // `[{"filePath":"/x"}` can never parse - the Rebuild failure class.
        let args = r#"[{"filePath":"/x"}garbage"#;
        let outcome = fix.fix_arguments(args);
        assert!(
            matches!(outcome, Err(FixError::Rebuild(_))),
            "truncated candidate that stays invalid must Err(Rebuild), got {outcome:?}"
        );
    }

    #[test]
    fn test_apply_with_context_matches_apply_on_repairable_payloads() {
        use super::ResponseFix;

        let fix = ToolcallBadFilepathFix::new();
        let request = serde_json::json!({});
        for args in [
            r#"{"filePath":"/path1","filePath":"/path2"}"#,
            r#"{"content":"中文测试内容","filePath":"/x/中文/file.rs","filePath"/x/bad"}"#,
            r#"{"content":"esc","filePath":"/x/\u4e2d/file.rs","filePath"/x/bad"}"#,
        ] {
            let response = fuzz_response(Some(args));
            let (applied, apply_action) = fix.apply(response.clone());
            let (contextual, ctx_action) = fix
                .apply_with_context(response, &request)
                .expect("repairable payload must be Ok");
            assert_eq!(
                serde_json::to_string(&applied).unwrap(),
                serde_json::to_string(&contextual).unwrap(),
                "apply() and apply_with_context() must agree byte-for-byte for: {args}"
            );
            assert_eq!(format!("{apply_action:?}"), format!("{ctx_action:?}"));
        }
    }

    // ============================================================
    // TASK 25 (B-M2): buffered duplicate keys keep FIRST, matching streaming
    // ============================================================
    // A valid-JSON payload with duplicate depth-1 `filePath` keys collapsed to
    // the LAST occurrence inside serde_json's Value map, while the streaming
    // path's client kept the FIRST value (its bytes were forwarded before the
    // object closed). Task 29 deleted the streaming accumulator machinery;
    // this test now pins the buffered path against the literal FIRST winner
    // the streaming path used to deliver.

    #[test]
    fn test_buffered_duplicate_keeps_first_matching_streaming_winner() {
        let fix = ToolcallBadFilepathFix::new();
        let args = r#"{"filePath":"/first","content":"x","filePath":"/last"}"#;

        // Given: a valid-JSON duplicate payload (detection is structural).
        serde_json::from_str::<Value>(args).expect("dup fixture must be valid JSON");
        assert!(fix.is_malformed(args), "duplicate keys must trigger");

        // When: the buffered path repairs it.
        let fixed = fix.fix_arguments(args).expect("valid dup stays repairable");
        let parsed: Value = serde_json::from_str(&fixed).expect("buffered fix must be valid JSON");

        // Then: the FIRST value wins - the literal documented streaming winner.
        assert_eq!(
            parsed["filePath"].as_str(),
            Some("/first"),
            "buffered winner must be the first occurrence, fixed = {fixed}"
        );
        assert_eq!(parsed["content"].as_str(), Some("x"), "non-dup field must survive");
    }

    #[test]
    fn test_buffered_duplicate_cjk_first_value_wins() {
        let fix = ToolcallBadFilepathFix::new();
        let args = r#"{"content":"你好世界","filePath":"/第一/文件.rs","filePath":"/第二"}"#;
        serde_json::from_str::<Value>(args).expect("CJK dup fixture must be valid JSON");

        let response = fuzz_response(Some(args));
        let (result, action) = fix.apply(response);

        let args_out = result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .expect("arguments must remain a string");
        let parsed: Value = serde_json::from_str(args_out).expect("CJK dup fix must be valid JSON");
        assert_eq!(
            parsed["filePath"].as_str(),
            Some("/第一/文件.rs"),
            "multibyte first value must win, got {args_out}"
        );
        assert_eq!(parsed["content"].as_str(), Some("你好世界"));
        assert!(matches!(action, FixAction::Fixed { .. }));
    }

    #[test]
    fn test_buffered_triple_duplicate_keeps_first() {
        let fix = ToolcallBadFilepathFix::new();
        let args = r#"{"filePath":"/one","content":"x","filePath":"/two","filePath":"/three"}"#;

        let fixed = fix.fix_arguments(args).expect("triple dup stays repairable");
        let parsed: Value = serde_json::from_str(&fixed).expect("triple dup fix must be valid JSON");
        assert_eq!(parsed["filePath"].as_str(), Some("/one"), "got {fixed}");
        assert_eq!(parsed["content"].as_str(), Some("x"));
        assert_eq!(top_level_key_count(&fixed, "filePath"), 1, "all later dups dropped");
    }

    #[test]
    fn test_buffered_escaped_key_duplicate_first_wins() {
        // The task-23 scanner decodes key escapes (`file\u0050ath` == "filePath"),
        // so the escaped key FIRST is the winner the scanner reports.
        let fix = ToolcallBadFilepathFix::new();
        let args = r#"{"file\u0050ath":"/first","filePath":"/last"}"#;
        serde_json::from_str::<Value>(args).expect("escaped-key dup fixture must be valid JSON");
        assert!(fix.is_malformed(args), "escaped-key duplicate must trigger");

        let fixed = fix.fix_arguments(args).expect("escaped-key dup stays repairable");
        let parsed: Value = serde_json::from_str(&fixed).expect("escaped-key dup fix must be valid JSON");
        assert_eq!(parsed["filePath"].as_str(), Some("/first"), "got {fixed}");
    }

    #[test]
    fn test_buffered_nested_duplicates_untouched_and_single_key_byte_identical() {
        let fix = ToolcallBadFilepathFix::new();

        // Duplicates ONLY inside a nested object: never the fix's problem
        // (task-23 depth discipline) - response passes through untouched.
        let nested_dups = r#"{"meta":{"filePath":"/a","filePath":"/b"},"content":"x"}"#;
        let response = fuzz_response(Some(nested_dups));
        assert!(!fix.applies(&response), "nested dups must not trigger");
        let (result, action) = fix.apply(response.clone());
        assert!(!action.detected());
        assert_eq!(result, response, "nested-dup response must be byte-identical");

        // Nested dups PLUS exactly one top-level key: single winner, untouched.
        let nested_plus_one = r#"{"meta":{"filePath":"/a","filePath":"/b"},"filePath":"/keep"}"#;
        let response = fuzz_response(Some(nested_plus_one));
        let (result, action) = fix.apply(response.clone());
        assert!(!action.detected());
        assert_eq!(result, response, "single top-level key must be byte-identical");

        // Non-duplicate parseable payloads keep the pre-task-25 serde
        // round-trip byte-for-byte (normalization only, winner untouched).
        for args in [
            r#"{"content":"x","filePath":"/only"}"#,
            r#"{ "filePath" : "/spaced" }"#,
            r#"{"file\u0050ath":"/escaped-only"}"#,
        ] {
            let fixed = fix.fix_arguments(args).expect("valid JSON round-trips");
            assert_eq!(
                fixed,
                serde_json::to_string(&serde_json::from_str::<Value>(args).unwrap()).unwrap(),
                "non-dup path must stay the plain serde round-trip for: {args}"
            );
        }
    }

    // ============================================================
    // TASK 30 (B-M3): multi-call repairs aggregate into ONE action
    // ============================================================
    // The historical single `overall_action` variable was overwritten by every
    // repair, so a response with two repaired calls reported ONLY the last
    // call's snippets — the earlier repair happened invisibly in the log. The
    // fixer now collects the per-call `FixAction`s into a Vec and folds them
    // into the single action the registry logs once per fix.

    #[test]
    fn task30_two_repaired_calls_aggregate_both_snippets() {
        let fix = ToolcallBadFilepathFix::new();
        let response = serde_json::json!({
            "choices": [{
                "message": { "tool_calls": [
                    { "index": 0, "function": { "name": "write", "arguments": r#"{"filePath":"/first","filePath":"/dup-a"}"# } },
                    { "index": 1, "function": { "name": "write", "arguments": r#"{"filePath":"/second","filePath":"/dup-b"}"# } }
                ] }
            }]
        });

        let (result, action) = fix.apply(response);

        // Both calls are really repaired (per-call FIRST-wins, task 25).
        for (idx, winner) in [(0, "/first"), (1, "/second")] {
            let args_out = result["choices"][0]["message"]["tool_calls"][idx]["function"]["arguments"]
                .as_str()
                .unwrap();
            let parsed: Value = serde_json::from_str(args_out).expect("every repaired call must be valid JSON");
            assert_eq!(parsed["filePath"].as_str(), Some(winner), "call {idx} keeps FIRST winner");
        }

        match action {
            FixAction::Fixed {
                original_snippet,
                fixed_snippet,
            } => {
                assert!(
                    original_snippet.contains("/dup-a") && original_snippet.contains("/dup-b"),
                    "the aggregated action must carry EVERY repaired call's original, got: {original_snippet}"
                );
                assert!(
                    fixed_snippet.contains("/first") && fixed_snippet.contains("/second"),
                    "the aggregated fixed snippet must carry every winner, got: {fixed_snippet}"
                );
            }
            other => panic!("aggregated action must be Fixed, got {other:?}"),
        }
    }

    #[test]
    fn task30_single_repair_snippets_pass_through_unchanged() {
        // One repair => the aggregated action is that very action, snippets
        // byte-identical to the pre-task-30 shape (no "1 repairs:" wrapper).
        let fix = ToolcallBadFilepathFix::new();
        let args = r#"{"filePath":"/first","filePath":"/dup"}"#;
        let response = fuzz_response(Some(args));

        let (_, action) = fix.apply(response);

        match action {
            FixAction::Fixed {
                original_snippet,
                fixed_snippet,
            } => {
                assert_eq!(original_snippet, args);
                assert_eq!(fixed_snippet, r#"{"filePath":"/first"}"#);
            }
            other => panic!("expected Fixed, got {other:?}"),
        }
    }
}
