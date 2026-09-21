//! Fix for malformed tool call arguments with invalid property names like `{}":`
//!
//! Some LLMs (like Qwen3-Coder) occasionally generate malformed JSON in tool call
//! arguments where they use `{}"` as a property name instead of the correct parameter name.
//!
//! Example malformed JSON:
//! ```json
//! {"content":"#!/bin/bash...",{}":"/path/to/file.sh"}
//! ```
//!
//! Expected JSON:
//! ```json
//! {"content":"#!/bin/bash...","file_path":"/path/to/file.sh"}
//! ```
//!
//! This fix:
//! 1. Detects tool calls with malformed arguments containing `{}"` property names
//! 2. Uses tool schemas from the request to determine the correct parameter name
//! 3. Fills slots by BIJECTION only when the count of missing schema parameters
//!    equals the slot count — the model emits slot values in schema order by
//!    construction, so an order-isomorphic assignment is not a guess. Zero,
//!    fewer-than-slots, or more-than-slots candidates return `Err` (subset
//!    selection is guesswork, root adjudication) so the registry forwards the
//!    ORIGINAL response untouched

use super::{json_scan::top_level_key_count, FixAction, FixError, ResponseFix};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

pub struct ToolcallMalformedArgumentsFix {
    /// Regex to detect malformed property names like `{}":`
    malformed_pattern: Regex,
}

impl Default for ToolcallMalformedArgumentsFix {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolcallMalformedArgumentsFix {
    pub fn new() -> Self {
        Self {
            // Matches the empty-key slot: a `,` or `{` delimiter, the `{}` token (captured
            // as `key`) that stands in for the real property name, then `":` and any space.
            malformed_pattern: Regex::new(r#"[,\{](?P<key>\{\})":\s*"#).unwrap(),
        }
    }

    /// Extract tool schemas from request
    fn extract_tool_schemas(request: &Value) -> HashMap<String, Vec<String>> {
        let mut schemas = HashMap::new();

        if let Some(tools) = request.get("tools").and_then(|t| t.as_array()) {
            for tool in tools {
                if let Some(function) = tool.get("function") {
                    let name = function.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();

                    let parameters = function
                        .get("parameters")
                        .and_then(|p| p.get("properties"))
                        .and_then(|props| props.as_object())
                        .map(|obj| obj.keys().map(|k| k.to_string()).collect::<Vec<String>>())
                        .unwrap_or_default();

                    if !name.is_empty() {
                        schemas.insert(name, parameters);
                    }
                }
            }
        }

        schemas
    }

    /// Attempt to fix malformed arguments using the tool schema, WITHOUT guessing.
    ///
    /// Candidates = schema keys (schema order) minus keys the task-23 scanner
    /// proves present. The scanner cannot read a malformed document, so presence
    /// is proven through a masked copy: every `{}` slot is spliced to an
    /// unguessable sentinel key, and the scanner counts each schema key in that
    /// copy. `candidates == slots` is an order-isomorphic bijection and splices
    /// (the model emits slot values in schema order by construction). Zero
    /// candidates, fewer than slots, or more than slots (subset selection)
    /// return `Err(Rebuild)` so the registry keeps the ORIGINAL response.
    /// `Ok(None)` means the input is not our malformed shape at all.
    fn fix_arguments(
        &self,
        args_str: &str,
        tool_name: &str,
        schemas: &HashMap<String, Vec<String>>,
    ) -> Result<Option<String>, FixError> {
        if !self.malformed_pattern.is_match(args_str) {
            return Ok(None);
        }

        let Some(schema_params) = schemas.get(tool_name) else {
            return Ok(None);
        };

        let candidates = self.candidate_keys(args_str, schema_params);
        let slots = self.malformed_pattern.find_iter(args_str).count();
        match candidates.len() {
            0 => Err(FixError::Rebuild("no candidate keys".to_string())),
            n if n == slots => {
                let fixed_args = self.splice_empty_key_slots(args_str, &candidates);
                if serde_json::from_str::<Value>(&fixed_args).is_ok() {
                    Ok(Some(fixed_args))
                } else {
                    let plural = if n == 1 { "" } else { "s" };
                    Err(FixError::Rebuild(format!(
                        "fill invalid: {slots} slots, {n} candidate{plural}"
                    )))
                }
            }
            n if n < slots => {
                let plural = if n == 1 { "" } else { "s" };
                Err(FixError::Rebuild(format!(
                    "fill invalid: {slots} slots, {n} candidate{plural}"
                )))
            }
            n => Err(FixError::Rebuild(format!("ambiguous: {n} candidates for {slots} slots"))),
        }
    }

    /// Schema keys not present in `args_str`, in schema order. Presence is proven
    /// on a masked copy of the arguments — every `{}` slot spliced to a sentinel
    /// key no schema can contain — because the task-23 scanner only reads valid
    /// JSON; on a document that stays invalid even masked, no key is provably
    /// present, so every schema key is a candidate and ambiguity errors out.
    fn candidate_keys<'a>(&self, args_str: &str, schema_params: &'a [String]) -> Vec<&'a str> {
        const SENTINEL: &str = "\\u0000llama-proxy-slot";
        let slots = self.malformed_pattern.find_iter(args_str).count();
        let masked = self.splice_empty_key_slots(args_str, &vec![SENTINEL; slots]);
        schema_params
            .iter()
            .map(String::as_str)
            .filter(|k| top_level_key_count(&masked, k) == 0)
            .collect()
    }

    /// Splice `missing` keys over the empty-key `{}` slots in `args_str`, left to right.
    ///
    /// Walks every slot matched by `malformed_pattern` in source order, rebuilding the
    /// output by copying the source between matches verbatim. Each slot's `{}` token is
    /// overwritten with `"<key>`; the slot's own trailing `"` and `:` close the key and
    /// stay put. Occurrence N consumes `missing[N]`; occurrences past `missing.len()` are
    /// copied untouched, their `{}` token surviving. Operates on raw bytes and never
    /// re-serializes, so every other region stays byte-verbatim.
    fn splice_empty_key_slots(&self, args_str: &str, missing: &[&str]) -> String {
        let mut out = String::with_capacity(args_str.len());
        let mut cursor = 0usize;
        let mut slot = 0usize;
        for caps in self.malformed_pattern.captures_iter(args_str) {
            // The whole match and the `key` group always exist for a match of this
            // pattern; should they ever not, the region is left verbatim (never corrupt).
            let (Some(whole), Some(key_tok)) = (caps.get(0), caps.name("key")) else {
                continue;
            };
            out.push_str(&args_str[cursor..whole.start()]);
            if slot < missing.len() {
                out.push_str(&args_str[whole.start()..key_tok.start()]);
                out.push('"');
                out.push_str(missing[slot]);
                out.push_str(&args_str[key_tok.end()..whole.end()]);
                slot += 1;
            } else {
                out.push_str(whole.as_str());
            }
            cursor = whole.end();
        }
        out.push_str(&args_str[cursor..]);
        out
    }

    /// True when one choice side — its `message` or its `delta` object — carries
    /// a tool call whose string `arguments` matches the malformed slot pattern.
    /// Non-string/absent arguments and structurally odd entries never trigger.
    fn side_has_trigger(&self, side: Option<&Value>) -> bool {
        side.and_then(|o| o.get("tool_calls"))
            .and_then(|tc| tc.as_array())
            .is_some_and(|tool_calls| {
                tool_calls.iter().any(|tool_call| {
                    tool_call
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|a| a.as_str())
                        .is_some_and(|args| self.malformed_pattern.is_match(args))
                })
            })
    }

    /// Every choice of BOTH response shapes (`message.tool_calls` and
    /// `delta.tool_calls`) gets the same trigger check — traversal shape
    /// mirrored from [`ToolCallNullIndexFix`](super::ToolCallNullIndexFix),
    /// predicate kept as this fix's own slot pattern.
    fn any_choice_triggers(&self, response: &Value) -> bool {
        response.get("choices").and_then(|c| c.as_array()).is_some_and(|choices| {
            choices
                .iter()
                .any(|choice| self.side_has_trigger(choice.get("message")) || self.side_has_trigger(choice.get("delta")))
        })
    }

    /// Run `fix_arguments` over one choice side's `tool_calls` array, collecting
    /// each repair into `repairs` for the caller's single aggregated action
    /// [B-M3]. The FIRST irreparable-but-triggering call aborts the whole
    /// response with `Err` — the pre-existing convention of this fix
    /// (`Err(e) => return Err(e)`, task-22 all-or-nothing per-response), not a
    /// new one: the registry fail-safe then forwards the ORIGINAL response
    /// untouched.
    fn fix_tool_calls(
        &self,
        tool_calls: &mut [Value],
        schemas: &HashMap<String, Vec<String>>,
        repairs: &mut Vec<FixAction>,
    ) -> Result<(), FixError> {
        for tool_call in tool_calls {
            let Some(function) = tool_call.get_mut("function") else {
                continue;
            };
            let tool_name = function.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
            let Some(args) = function.get("arguments").and_then(|a| a.as_str()) else {
                continue;
            };
            let args = args.to_string();
            match self.fix_arguments(&args, &tool_name, schemas) {
                Err(error) => return Err(error),
                Ok(Some(fixed_args)) => {
                    function["arguments"] = Value::String(fixed_args.clone());
                    repairs.push(FixAction::fixed(&args, &fixed_args));
                }
                Ok(None) => {}
            }
        }
        Ok(())
    }

    /// Fix tool calls in response using request context. EVERY choice of BOTH
    /// shapes (message and delta) gets the same `fix_arguments` attempt; the
    /// first irreparable-but-triggering call aborts the whole repair with
    /// `Err` (per-response all-or-nothing, see [`Self::fix_tool_calls`]) so the
    /// registry fail-safe forwards the ORIGINAL response.
    fn fix_response_with_context(&self, mut response: Value, request: &Value) -> Result<(Value, FixAction), FixError> {
        let schemas = Self::extract_tool_schemas(request);

        if schemas.is_empty() {
            tracing::warn!(
                fix_name = self.name(),
                "No tool schemas in request - cannot fix malformed arguments without context"
            );
            return Ok((response, FixAction::NotApplicable));
        }

        let mut repairs: Vec<FixAction> = Vec::new();

        if let Some(choices) = response.get_mut("choices").and_then(|c| c.as_array_mut()) {
            for choice in choices {
                if let Some(message) = choice.get_mut("message") {
                    if let Some(tool_calls) = message.get_mut("tool_calls").and_then(|tc| tc.as_array_mut()) {
                        self.fix_tool_calls(tool_calls, &schemas, &mut repairs)?;
                    }
                }
                if let Some(delta) = choice.get_mut("delta") {
                    if let Some(tool_calls) = delta.get_mut("tool_calls").and_then(|tc| tc.as_array_mut()) {
                        self.fix_tool_calls(tool_calls, &schemas, &mut repairs)?;
                    }
                }
            }
        }

        Ok((response, FixAction::aggregate_repairs(&repairs)))
    }
}

impl ResponseFix for ToolcallMalformedArgumentsFix {
    fn name(&self) -> &str {
        "toolcall_malformed_arguments"
    }

    fn description(&self) -> &str {
        "Fixes malformed tool call arguments with invalid property names like `{}\"`"
    }

    fn applies(&self, response: &Value) -> bool {
        self.any_choice_triggers(response)
    }

    fn apply(&self, response: Value) -> (Value, FixAction) {
        // Without context, we can't fix - just pass through
        (response, FixAction::NotApplicable)
    }

    fn applies_with_context(&self, response: &Value, request: &Value) -> bool {
        // Same all-choices gate as applies(), plus the schema precondition:
        // without `tools` in the request there is no candidate, so the fixer
        // could only warn-and-pass.
        self.any_choice_triggers(response) && request.get("tools").is_some()
    }

    fn apply_with_context(&self, response: Value, request: &Value) -> Result<(Value, FixAction), FixError> {
        self.fix_response_with_context(response, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_tool_schemas() {
        let request = json!({
            "model": "qwen3",
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "write",
                        "description": "Write a file",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "file_path": {"type": "string"},
                                "content": {"type": "string"}
                            },
                            "required": ["file_path", "content"]
                        }
                    }
                },
                {
                    "type": "function",
                    "function": {
                        "name": "read",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "path": {"type": "string"}
                            }
                        }
                    }
                }
            ]
        });

        let schemas = ToolcallMalformedArgumentsFix::extract_tool_schemas(&request);

        assert_eq!(schemas.len(), 2);
        assert!(schemas.contains_key("write"));
        assert!(schemas.contains_key("read"));

        let write_params = &schemas["write"];
        assert_eq!(write_params.len(), 2);
        assert!(write_params.contains(&"file_path".to_string()));
        assert!(write_params.contains(&"content".to_string()));

        let read_params = &schemas["read"];
        assert_eq!(read_params.len(), 1);
        assert!(read_params.contains(&"path".to_string()));
    }

    #[test]
    fn test_malformed_pattern_detection() {
        let fix = ToolcallMalformedArgumentsFix::new();

        // Should match malformed patterns
        assert!(fix.malformed_pattern.is_match("{\"content\":\"test\",{}\":\"/path\"}"));
        assert!(fix.malformed_pattern.is_match("{{}\":\"value\"}"));

        // Should not match valid JSON
        assert!(!fix
            .malformed_pattern
            .is_match("{\"file_path\":\"test\",\"content\":\"data\"}"));
    }

    #[test]
    fn test_fix_arguments_single_missing_param() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let mut schemas = HashMap::new();
        schemas.insert("write".to_string(), vec!["file_path".to_string(), "content".to_string()]);

        let malformed = "{\"content\":\"#!/bin/bash\\necho hello\",{}\":\"/tmp/test.sh\"}";

        let fixed = fix.fix_arguments(malformed, "write", &schemas).unwrap();

        assert!(fixed.is_some());
        let fixed = fixed.unwrap();

        // Should have replaced {} with file_path
        assert!(fixed.contains("\"file_path\":"));
        assert!(!fixed.contains("{}\":"));

        // Should be valid JSON
        let parsed: Result<Value, _> = serde_json::from_str(&fixed);
        assert!(parsed.is_ok());

        let parsed = parsed.unwrap();
        assert_eq!(parsed["file_path"].as_str().unwrap(), "/tmp/test.sh");
        // Note: The escaped \n in the test string is just two characters in the JSON string
        assert!(parsed["content"].as_str().unwrap().contains("bash"));
    }

    #[test]
    fn test_fix_arguments_no_malformed_pattern() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let mut schemas = HashMap::new();
        schemas.insert("write".to_string(), vec!["file_path".to_string(), "content".to_string()]);

        let valid = "{\"file_path\":\"/tmp/test.sh\",\"content\":\"data\"}";

        let result = fix.fix_arguments(valid, "write", &schemas);

        // Should return None for valid JSON
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_fix_arguments_unknown_tool() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let schemas = HashMap::new(); // Empty schemas

        let malformed = "{\"content\":\"test\",{}\":\"/path\"}";

        let result = fix.fix_arguments(malformed, "unknown_tool", &schemas);

        // Should return None when tool not in schema
        assert!(result.unwrap().is_none());
    }

    // The masked-presence check that replaces aggressive_parse_json must never
    // see the `{}` garbage token as a key: masked splices it away, so candidates
    // stay pure schema keys and the lone real candidate fills.
    #[test]
    fn test_garbage_token_never_a_candidate() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let schemas = schema_map(&["path", "content"]);

        let malformed = "{\"content\":\"test value\",{}\":\"/some/path\"}";
        assert_eq!(fix.candidate_keys(malformed, &schemas["write"]), vec!["path"]);

        let fixed = fix.fix_arguments(malformed, "write", &schemas).unwrap();
        let fixed = fixed.expect("lone candidate fills");
        assert!(!fixed.contains("{}"));
        serde_json::from_str::<Value>(&fixed).expect("fill is valid JSON");
    }

    #[test]
    fn test_applies_with_context() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let request = json!({
            "tools": [{"type": "function", "function": {"name": "write"}}]
        });

        // Retargeted by task 28 [B-M6]: the gate is trigger-based now, so this
        // fixture carries a real empty-key slot (the old `"{}"` payload had no
        // slot and no longer claims a detection - see
        // task28_clean_tool_calls_do_not_trigger_gate for that pin).
        let response_with_tools = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "write",
                            "arguments": r#"{"content":"data",{}":"/tmp/file.txt"}"#
                        }
                    }]
                }
            }]
        });

        let response_without_tools = json!({
            "choices": [{
                "message": {
                    "content": "Hello"
                }
            }]
        });

        assert!(fix.applies_with_context(&response_with_tools, &request));
        assert!(!fix.applies_with_context(&response_without_tools, &request));
    }

    #[test]
    fn test_fix_response_with_context() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let request = json!({
            "tools": [{
                "type": "function",
                "function": {
                    "name": "write",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "file_path": {"type": "string"},
                            "content": {"type": "string"}
                        }
                    }
                }
            }]
        });

        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "write",
                            "arguments": "{\"content\":\"#!/bin/bash\\necho test\",{}\":\"/tmp/script.sh\"}"
                        }
                    }]
                }
            }]
        });

        let (fixed, action) = fix.fix_response_with_context(response, &request).unwrap();

        let args = fixed["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();

        // Should have fixed the malformed argument
        assert!(args.contains("\"file_path\":"));
        assert!(!args.contains("{}\":"));

        // Should be valid JSON
        let parsed: Result<Value, _> = serde_json::from_str(args);
        assert!(parsed.is_ok());

        // Should have detected the fix
        assert!(action.detected());
    }

    #[test]
    fn test_apply_without_context_passes_through() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "write",
                            "arguments": "{\"content\":\"test\",{}\":\"/path\"}"
                        }
                    }]
                }
            }]
        });

        // Without context, should pass through unchanged
        let (result, action) = fix.apply(response.clone());
        assert_eq!(result, response);
        assert!(!action.detected());
    }

    #[test]
    fn test_single_missing_param_among_three() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let mut schemas = HashMap::new();
        // Three params, only file_path missing (content and mode are present), one slot.
        schemas.insert(
            "write".to_string(),
            vec!["file_path".to_string(), "content".to_string(), "mode".to_string()],
        );

        let malformed = "{\"content\":\"data\",\"mode\":\"0755\",{}\":\"/tmp/file\"}";

        let fixed = fix.fix_arguments(malformed, "write", &schemas).unwrap();

        assert!(fixed.is_some());
        let fixed = fixed.unwrap();

        // The lone slot is filled with the only missing key.
        assert!(fixed.contains("\"file_path\":"));

        // Should be valid JSON
        let parsed: Result<Value, _> = serde_json::from_str(&fixed);
        assert!(parsed.is_ok());
    }

    #[test]
    fn test_original_user_issue() {
        // This is the exact issue from the user's problem
        // LLM generated: {"content":"#!/bin/bash...",{}":"/path/to/file.sh"}
        // Expected: {"content":"#!/bin/bash...","file_path":"/path/to/file.sh"}

        let fix = ToolcallMalformedArgumentsFix::new();

        let request = json!({
            "model": "qwen3",
            "tools": [{
                "type": "function",
                "function": {
                    "name": "write",
                    "description": "Write a file",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "file_path": {"type": "string", "description": "Path to the file"},
                            "content": {"type": "string", "description": "File content"}
                        },
                        "required": ["file_path", "content"]
                    }
                }
            }]
        });

        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "write",
                            "arguments": "{\"content\":\"#!/bin/bash\\n\\n# Calculate prime numbers from 0 to 1024\\n\\necho \\\"Prime numbers from 0 to 1024:\\\"\\n\\nfor ((n = 2; n <= 1024; n++)); do\\n    is_prime=true\\n\\n    # Check divisibility from 2 to sqrt(n)\\n    for ((i = 2; i * i <= n; i++)); do\\n        ((n % i == 0)) && { is_prime=false; break; }\\n    done\\n\\n    $is_prime && echo \\\"$n\\\"\\ndone\\n\",{}\":\"/home/iphands/prog/slop/llama-proxy/trash/primes.sh\"}"
                        }
                    }]
                }
            }]
        });

        // Apply the fix
        let (fixed, action) = fix.apply_with_context(response, &request).unwrap();

        // Verify the fix was applied
        let args_str = fixed["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();

        // Should have file_path, not {}
        assert!(args_str.contains("\"file_path\":"));
        assert!(!args_str.contains("{}\":"));

        // Should be valid JSON
        let parsed_args: Result<Value, _> = serde_json::from_str(args_str);
        assert!(parsed_args.is_ok());

        let args = parsed_args.unwrap();
        assert_eq!(
            args["file_path"].as_str().unwrap(),
            "/home/iphands/prog/slop/llama-proxy/trash/primes.sh"
        );
        assert!(args["content"].as_str().unwrap().contains("#!/bin/bash"));

        // Should have detected the fix
        assert!(action.detected());
    }

    // ---- positional empty-key-slot replacement ----

    // Single-entry schema map whose key order is exactly `order`, the schema order the
    // splicer must honour.
    fn schema_map(order: &[&str]) -> HashMap<String, Vec<String>> {
        let mut m = HashMap::new();
        m.insert("write".to_string(), order.iter().map(|k| (*k).to_string()).collect());
        m
    }

    // Root adjudication (fix-forward from d239188): Qwen3-Coder empty-key slots
    // carry values in schema order BY CONSTRUCTION (task-26 B-H3, HIGH severity),
    // so candidates == slots is an order-isomorphic BIJECTION, not a guess:
    // occurrence N gets candidate N. Splicer-mechanism pin (task 26) kept below.
    #[test]
    fn test_two_empty_key_slots_assigned_in_schema_order() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let schemas = schema_map(&["path", "mode", "content"]);

        let malformed = r#"{"content":"data",{}":"/tmp/a",{}":"hello"}"#;
        let fixed = fix
            .fix_arguments(malformed, "write", &schemas)
            .expect("2 candidates / 2 slots is a bijection")
            .expect("bijection must assign, not no-op");
        assert_eq!(fixed, r#"{"content":"data","path":"/tmp/a","mode":"hello"}"#);

        // Splicer-mechanism pin (task 26, unchanged): given the key list it splices
        // distinct keys in order — occurrence N gets missing key N.
        let spliced = fix.splice_empty_key_slots(malformed, &["path", "mode"]);
        assert_eq!(spliced, r#"{"content":"data","path":"/tmp/a","mode":"hello"}"#);
    }

    // One slot with two candidate keys (path, mode) must Err: taking candidate[0]
    // is SUBSET SELECTION — the condemned guess class (adjudication concurs).
    // Splicer-level pin kept: one key spliced into one slot fills it.
    #[test]
    fn test_one_slot_with_multiple_candidates_ambiguous() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let schemas = schema_map(&["path", "mode", "content"]);

        let malformed = r#"{"content":"x",{}":"/tmp/a"}"#;
        let err = fix
            .fix_arguments(malformed, "write", &schemas)
            .expect_err("two candidate keys for one slot is subset selection -> Err");
        assert_eq!(err.to_string(), "rebuild error: ambiguous: 2 candidates for 1 slots");

        assert_eq!(
            fix.splice_empty_key_slots(malformed, &["path"]),
            r#"{"content":"x","path":"/tmp/a"}"#
        );
    }

    // Given more slots than viable keys: the lone candidate (content is present)
    // cannot cover both slots — no bijection exists, so Err(Rebuild) and the
    // registry keeps the original. Splicer-level leftover pin kept:
    // slots past the key list survive byte-verbatim.
    #[test]
    fn test_more_slots_than_candidates_fill_invalid_err() {
        let fix = ToolcallMalformedArgumentsFix::new();

        // leftover slot keeps its `{}` token, byte-verbatim
        let spliced = fix.splice_empty_key_slots(r#"{"content":"x",{}":"/tmp/a",{}":"/tmp/b"}"#, &["path"]);
        assert_eq!(spliced, r#"{"content":"x","path":"/tmp/a",{}":"/tmp/b"}"#);

        let schemas = schema_map(&["path", "content"]);
        let err = fix
            .fix_arguments(r#"{"content":"x",{}":"/tmp/a",{}":"/tmp/b"}"#, "write", &schemas)
            .expect_err("one candidate cannot fill two slots -> Err, original preserved upstream");
        assert_eq!(err.to_string(), "rebuild error: fill invalid: 2 slots, 1 candidate");
    }

    // Zero candidates: every schema key is already present and the `{}` slot is
    // pure extra garbage. Nothing may be filled — Err("no candidate keys").
    #[test]
    fn test_all_schema_keys_present_yields_no_candidate_err() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let schemas = schema_map(&["path", "content"]);

        let err = fix
            .fix_arguments(r#"{"path":"/p","content":"c",{}":"/tmp/x"}"#, "write", &schemas)
            .expect_err("all schema keys present: the slot is extra garbage, never guessed away");
        assert_eq!(err.to_string(), "rebuild error: no candidate keys");
    }

    // With no empty-key slots, the splicer returns the input byte-identical and
    // fix_arguments makes no change.
    #[test]
    fn test_no_slots_output_is_byte_identical() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let clean = r#"{"path":"/x","content":"y"}"#;
        assert_eq!(fix.splice_empty_key_slots(clean, &["z"]), clean);

        let schemas = schema_map(&["path", "content"]);
        assert!(fix.fix_arguments(clean, "write", &schemas).unwrap().is_none());
    }

    // Naive-regex trap: an empty-string key `"":` living inside a string VALUE is not
    // an empty-key slot and must not be touched (the slot form is the brace token `{}`).
    #[test]
    fn test_empty_string_key_in_value_is_not_a_slot() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let decoy = r#"{"x":"{\"\": 1}"}"#;
        assert!(!fix.malformed_pattern.is_match(decoy));

        let schemas = schema_map(&["path"]);
        assert!(fix.fix_arguments(decoy, "write", &schemas).unwrap().is_none());
        // Byte-identical: the splicer has nothing to rewrite.
        assert_eq!(fix.splice_empty_key_slots(decoy, &["path"]), decoy);
    }

    // A literal `{}` inside a string VALUE is not a slot: only the real key-position
    // slot is replaced; the value region stays verbatim.
    #[test]
    fn test_braces_inside_string_value_are_not_slots() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let schemas = schema_map(&["path", "content"]);

        let malformed = r#"{"content":"brace {} here",{}":"/tmp/a"}"#;
        let fixed = fix
            .fix_arguments(malformed, "write", &schemas)
            .unwrap()
            .expect("real slot must be fixed");
        assert_eq!(fixed, r#"{"content":"brace {} here","path":"/tmp/a"}"#);
    }

    // A nested `{"": ...}` (empty-string key at depth 2) is not a top-level slot and
    // stays byte-verbatim; only the top-level brace slot is replaced.
    #[test]
    fn test_nested_empty_string_key_is_not_a_slot() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let schemas = schema_map(&["path", "content"]);

        let malformed = r#"{"outer":{"":1},"content":"x",{}":"/tmp/a"}"#;
        let fixed = fix
            .fix_arguments(malformed, "write", &schemas)
            .unwrap()
            .expect("top-level slot must be fixed");
        assert_eq!(fixed, r#"{"outer":{"":1},"content":"x","path":"/tmp/a"}"#);
    }

    // Byte-splice proof (task 26 splicer, mechanism unchanged): CJK values before
    // and after the slots keep their multibyte payloads verbatim. At fix_arguments
    // level this is 2 candidates / 2 slots — bijection, so the ordered splice runs
    // (adjudication) — and the multibyte regions survive byte-for-byte.
    #[test]
    fn test_cjk_two_slots_splicer_verbatim_assigned_at_fix_level() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let schemas = schema_map(&["path", "mode", "content"]);

        let malformed = r#"{"content":"你好世界",{}":"/路径/文件.rs",{}":"第二"}"#;
        let spliced = fix.splice_empty_key_slots(malformed, &["path", "mode"]);
        assert_eq!(spliced, r#"{"content":"你好世界","path":"/路径/文件.rs","mode":"第二"}"#);

        let fixed = fix
            .fix_arguments(malformed, "write", &schemas)
            .expect("2 candidates / 2 slots is a bijection")
            .expect("bijection assigns in schema order");
        assert_eq!(fixed, r#"{"content":"你好世界","path":"/路径/文件.rs","mode":"第二"}"#);
    }

    // Byte-splice proof with escaped quotes (task 26 splicer, mechanism unchanged):
    // the `\"` value region survives verbatim. 2 candidates / 2 slots -> bijection
    // assigns; escaped region byte-verbatim at fix level too.
    #[test]
    fn test_escaped_quote_splicer_verbatim_assigned_at_fix_level() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let schemas = schema_map(&["path", "mode", "content"]);

        let malformed = r#"{"content":"say \"hi\"",{}":"/tmp/a",{}":"b"}"#;
        let spliced = fix.splice_empty_key_slots(malformed, &["path", "mode"]);
        assert_eq!(spliced, r#"{"content":"say \"hi\"","path":"/tmp/a","mode":"b"}"#);

        let fixed = fix
            .fix_arguments(malformed, "write", &schemas)
            .expect("2 candidates / 2 slots is a bijection")
            .expect("bijection assigns in schema order");
        assert_eq!(fixed, r#"{"content":"say \"hi\"","path":"/tmp/a","mode":"b"}"#);
    }

    // ---- task 27 (bijection-gated, root adjudication): no-guess candidate parsing ----
    //
    // Candidates = (schema keys, iterated in SCHEMA order) minus (keys proven
    // present by the task-23 scanner on the slot-masked copy). candidates == slots
    // is an order-isomorphic bijection and assigns; candidates < slots, candidates
    // == 0, and candidates > slots (subset selection) all Err(Rebuild) so the
    // registry keeps the ORIGINAL response.

    // The candidate set is computed correctly under CJK schema keys, dotted key
    // names, and escaped-\" string values mid-object: the scanner sees through
    // all three, and a dotted key living in a VALUE stays a candidate.
    #[test]
    fn task27_candidate_scanner_sanity_cjk_dotted_escaped() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let cjk = schema_map(&["路径", "内容"]);
        assert_eq!(fix.candidate_keys(r#"{"内容":"甲",{}":"/乙"}"#, &cjk["write"]), vec!["路径"]);

        let dotted = schema_map(&["a.b", "path"]);
        assert_eq!(
            fix.candidate_keys(r#"{"a.b":1,{}":"v"}"#, &dotted["write"]),
            vec!["path"],
            "present dotted key drops out"
        );
        assert_eq!(
            fix.candidate_keys(r#"{"x":"a.b",{}":"v"}"#, &dotted["write"]),
            vec!["a.b", "path"],
            "dotted key inside a value is not present: both stay candidates, schema order"
        );

        let escaped = schema_map(&["path", "say \"a.b\""]);
        assert_eq!(
            fix.candidate_keys(r#"{"say \"a.b\"":"x",{}":"/tmp/f"}"#, &escaped["write"]),
            vec!["path"],
            "escaped-quote key is present; scanner must not desync on the value"
        );
    }

    // Ordering independence: the schema list, not the document's insertion order,
    // is the iteration source. Same missing pair, document unchanged: reversing
    // the schema reverses the candidate list.
    #[test]
    fn task27_candidate_set_follows_schema_order_not_scan_order() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let input = r#"{"content":"c",{}":"x"}"#;

        let order_a = schema_map(&["path", "mode"]);
        assert_eq!(
            fix.candidate_keys(input, &order_a["write"]),
            vec!["path", "mode"],
            "candidates must be enumerated in schema order"
        );

        let order_b = schema_map(&["mode", "path"]);
        assert_eq!(
            fix.candidate_keys(input, &order_b["write"]),
            vec!["mode", "path"],
            "candidate order tracks the schema, independent of scan/insertion order"
        );
    }

    // RETARGETED by the adjudication: the old fixture was 2 slots / 2 candidates,
    // which the bijection gate now ASSIGNS. The genuine subset shape (2 slots, 3
    // candidates — `content` absent, `note` not in schema) is the refused guess
    // class: the fixer errors and the response reaches the client BYTE-IDENTICAL
    // to the input (the task-31 registry forwards the untouched original on Err).
    #[test]
    fn task27_subset_selection_error_and_response_bytes_preserved() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let request = write_request(&["path", "mode", "content"]);

        let args = r#"{"note":"data",{}":"/tmp/a",{}":"b"}"#;
        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "write", "arguments": args }
                    }]
                }
            }]
        });
        let input_bytes = response.to_string();

        let result = fix.apply_with_context(response.clone(), &request);
        match result {
            Ok((v, _a)) => panic!(
                "RED (harm demonstration) — a guesser would subset-select here; \
                 the wrong-key fill it produced for the client was: {:?}",
                v["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"].as_str()
            ),
            Err(err) => {
                assert!(matches!(err, FixError::Rebuild(_)), "must be Rebuild, got {err:?}");
                assert_eq!(err.to_string(), "rebuild error: ambiguous: 3 candidates for 2 slots");
            }
        }
        assert_eq!(
            response.to_string(),
            input_bytes,
            "fixer must not mutate what the fail-safe forwards"
        );
    }

    // Subset-selection refusal at the unit level: 2 slots with 3 candidates must
    // Err — picking any 2-of-3 is the condemned guess class. CJK candidates and
    // duplicate slot values included (parsing-adversarial shapes).
    #[test]
    fn test_two_slots_three_candidates_ambiguous() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let cjk = schema_map(&["路径", "模式", "备注"]);
        let err = fix
            .fix_arguments(r#"{"杂项":"x",{}":"/甲",{}":"/乙"}"#, "write", &cjk)
            .expect_err("3 candidates for 2 slots is subset selection -> Err");
        assert_eq!(err.to_string(), "rebuild error: ambiguous: 3 candidates for 2 slots");

        let latin = schema_map(&["path", "mode", "content"]);
        let err = fix
            .fix_arguments(r#"{"other":"x",{}":"same",{}":"same"}"#, "write", &latin)
            .expect_err("duplicate slot values do not make a bijection");
        assert_eq!(err.to_string(), "rebuild error: ambiguous: 3 candidates for 2 slots");
    }

    /// Request carrying a `write` tool whose schema key order is exactly `order`.
    fn write_request(order: &[&str]) -> serde_json::Value {
        let mut props = serde_json::Map::new();
        for k in order {
            props.insert(k.to_string(), json!({"type": "string"}));
        }
        json!({
            "tools": [{
                "type": "function",
                "function": {
                    "name": "write",
                    "parameters": { "type": "object", "properties": props }
                }
            }]
        })
    }

    // ---- task 28 (B-M6): applies()/apply() traverse EVERY choice, message AND delta ----
    //
    // Mirrored from ToolCallNullIndexFix: its applies() checks message.tool_calls
    // AND delta.tool_calls of every choice, and fix_tool_calls_in_choices mutates
    // both shapes for every choice. Only the TRAVERSAL SHAPE is mirrored — the
    // repair stays this fix's bijection-gated fix_arguments. The Err convention
    // is likewise mirrored, not invented: fix_response_with_context already did
    // `Err(e) => return Err(e)` on the first irreparable-but-triggering call
    // (task-22 all-or-nothing per-response), so a failure in ANY choice aborts
    // the whole response repair and the registry fail-safe forwards the ORIGINAL.

    /// Registry carrying only this fix — exercises the buffered context path's
    /// public sequence (applies_with_context gate -> apply_with_context) exactly
    /// as FixRegistry::apply_fixes_with_context runs it.
    fn registry_with_malformed_fix() -> crate::fixes::FixRegistry {
        let mut registry = crate::fixes::FixRegistry::new();
        registry.register(std::sync::Arc::new(ToolcallMalformedArgumentsFix::new()));
        registry
    }

    // Acceptance [B-M6]: a malformed call in choices[1] triggers the fix.
    // choices[0] is a clean final message (content, no tool_calls), so the
    // baseline choices[0]-only gate stays silent and the payload reaches the
    // client unmodified.
    #[test]
    fn task28_malformed_in_choices_one_message_triggers_fix() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let request = write_request(&["file_path", "content"]);
        let response = json!({
            "choices": [
                { "index": 0, "message": { "role": "assistant", "content": "done" }, "finish_reason": "stop" },
                { "index": 1, "message": { "role": "assistant", "tool_calls": [{
                    "id": "call_1", "type": "function",
                    "function": { "name": "write", "arguments": r#"{"content":"data",{}":"/tmp/file.txt"}"# }
                }] }, "finish_reason": "tool_calls" }
            ]
        });

        assert!(
            fix.applies(&response),
            "applies() must trigger on a choices[1] message tool call"
        );
        assert!(fix.applies_with_context(&response, &request));

        let result = registry_with_malformed_fix().apply_fixes_with_context(response, &request);
        let args = result["choices"][1]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert!(args.contains(r#""file_path":"#));
        assert!(!args.contains(r#"{}":"#));
        serde_json::from_str::<Value>(args).expect("fixed arguments are valid JSON");
    }

    // Delta-shape variant of the acceptance: choices[1].delta.tool_calls carries
    // the malformed slot. Baseline is doubly narrow — the gate never looks at a
    // delta, and fix_response_with_context only traverses `message`.
    #[test]
    fn task28_malformed_in_choices_one_delta_triggers_fix() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let request = write_request(&["file_path", "content"]);
        let response = json!({
            "choices": [
                { "index": 0, "message": { "role": "assistant", "content": "done" }, "finish_reason": "stop" },
                { "index": 1, "delta": { "tool_calls": [{
                    "index": 0,
                    "function": { "name": "write", "arguments": r#"{"content":"data",{}":"/tmp/file.txt"}"# }
                }] } }
            ]
        });

        assert!(
            fix.applies(&response),
            "applies() must trigger on a choices[1] delta tool call"
        );
        assert!(fix.applies_with_context(&response, &request));

        let result = registry_with_malformed_fix().apply_fixes_with_context(response, &request);
        let args = result["choices"][1]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert!(args.contains(r#""file_path":"#));
        assert!(!args.contains(r#"{}":"#));
        serde_json::from_str::<Value>(args).expect("fixed delta arguments are valid JSON");
    }

    // Green-on-baseline guard by construction (baseline already looped every
    // choice's message once the choices[0] gate passed): pins that extending the
    // traversal did not regress multi-call repair — both malformed choices come
    // back fixed in one apply pass.
    #[test]
    fn task28_two_malformed_choices_both_fixed() {
        let request = write_request(&["file_path", "content"]);
        let response = json!({
            "choices": [
                { "message": { "tool_calls": [{ "function": { "name": "write", "arguments": r#"{"content":"a",{}":"/tmp/0"}"# } }] } },
                { "message": { "tool_calls": [{ "function": { "name": "write", "arguments": r#"{"content":"b",{}":"/tmp/1"}"# } }] } }
            ]
        });

        let result = registry_with_malformed_fix().apply_fixes_with_context(response, &request);
        for i in 0..2 {
            let args = result["choices"][i]["message"]["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .unwrap();
            assert!(args.contains(r#""file_path":"#), "choice[{i}] must be fixed");
            assert!(!args.contains(r#"{}":"#), "choice[{i}] slot must be gone");
        }
    }

    // Mirrored error semantics (NOT invented): the first irreparable-but-
    // triggering call Errs out of apply_with_context — task-22 all-or-nothing
    // per-response, the same shape fix_response_with_context already had for
    // choice[0]-shaped inputs. choice[0] WOULD have been repaired; on Err the
    // registry fail-safe forwards the ORIGINAL bytes. Also green-on-baseline:
    // this pins the convention the extension preserves, not the traversal bug.
    #[test]
    fn task28_irreparable_later_choice_aborts_whole_response() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let request = write_request(&["path", "mode", "content"]);
        let response = json!({
            "choices": [
                { "message": { "tool_calls": [{ "function": { "name": "write", "arguments": r#"{"content":"a","mode":"0644",{}":"/tmp/0"}"# } }] } },
                { "message": { "tool_calls": [{ "function": { "name": "write", "arguments": r#"{"content":"b",{}":"/tmp/1"}"# } }] } }
            ]
        });
        let input_bytes = response.to_string();

        let err = fix
            .apply_with_context(response.clone(), &request)
            .expect_err("choice[1] ambiguity aborts the whole response repair");
        assert!(matches!(err, FixError::Rebuild(_)));
        assert_eq!(err.to_string(), "rebuild error: ambiguous: 2 candidates for 1 slots");
        assert_eq!(
            response.to_string(),
            input_bytes,
            "the fixer took a clone; caller's copy untouched"
        );

        let result = registry_with_malformed_fix().apply_fixes_with_context(response, &request);
        assert_eq!(result.to_string(), input_bytes, "fail-safe forwards the ORIGINAL bytes");
    }

    // Gate semantics pin [B-M6]: the predicate is now trigger-based — well-formed
    // arguments are NOT a triggering arguments string (message OR delta), and
    // odd-but-legal entries (null arguments, missing arguments, missing
    // function, null tool-call entry) neither trigger nor panic.
    #[test]
    fn task28_clean_tool_calls_do_not_trigger_gate() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let clean = json!({
            "choices": [
                { "message": { "tool_calls": [{ "function": { "name": "write", "arguments": r#"{"content":"d","file_path":"/tmp/f"}"# } }] } },
                { "delta": { "tool_calls": [{ "function": { "name": "write", "arguments": r#"{"content":"d"}"# } }] } }
            ]
        });
        assert!(!fix.applies(&clean), "valid arguments must not claim a detection");
        assert!(!fix.applies_with_context(&clean, &write_request(&["file_path", "content"])));

        let odd = json!({
            "choices": [{
                "delta": { "tool_calls": [
                    { "function": { "name": "write", "arguments": null } },
                    { "function": { "name": "write" } },
                    {},
                    null
                ] }
            }]
        });
        assert!(!fix.applies(&odd));
    }

    // New-input pin: a null entry between valid choices must not mask the later
    // trigger and must not panic the traversal.
    #[test]
    fn task28_null_choice_between_valid_choices_is_skipped() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let request = write_request(&["file_path", "content"]);
        let response = json!({
            "choices": [
                { "message": { "content": "done" } },
                Value::Null,
                { "message": { "tool_calls": [{ "function": { "name": "write", "arguments": r#"{"content":"x",{}":"/tmp/n"}"# } }] } }
            ]
        });

        assert!(fix.applies(&response), "null entry must not mask the choices[2] trigger");
        let result = registry_with_malformed_fix().apply_fixes_with_context(response, &request);
        assert!(result["choices"][1].is_null(), "null choice survives untouched");
        let args = result["choices"][2]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert!(args.contains(r#""file_path":"#));
    }

    // New-input pin: a delta tool_calls entry missing the `function` key is the
    // real streaming shape (first chunk carries only {"index","id","type"}).
    // No panic, traversal continues, the next entry still fixes.
    #[test]
    fn task28_delta_tool_call_without_function_key_no_panic() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let request = write_request(&["file_path", "content"]);
        let response = json!({
            "choices": [
                { "delta": { "content": "" } },
                { "delta": { "tool_calls": [
                    { "index": 0, "id": "call_1", "type": "function" },
                    { "index": 0, "function": { "name": "write", "arguments": r#"{"content":"x",{}":"/tmp/f"}"# } }
                ] } }
            ]
        });

        assert!(fix.applies(&response));
        let result = registry_with_malformed_fix().apply_fixes_with_context(response, &request);
        let deltas = result["choices"][1]["delta"]["tool_calls"].as_array().unwrap();
        assert!(deltas[0].get("function").is_none(), "function-less entry stays as sent");
        let args = deltas[1]["function"]["arguments"].as_str().unwrap();
        assert!(args.contains(r#""file_path":"#));
    }

    // TASK 30 (B-M3): the single `overall_action` was overwritten by every
    // repair, so with one repaired call per choice ONLY the last choice's
    // snippets reached the log. Per-call actions are now collected and folded
    // into the ONE action the registry logs per fix.
    #[test]
    fn task30_two_repaired_calls_across_choices_aggregate_both_snippets() {
        let request = write_request(&["file_path", "content"]);
        let response = json!({
            "choices": [
                { "message": { "tool_calls": [{ "function": { "name": "write", "arguments": r#"{"content":"AAA",{}":"/tmp/a.txt"}"# } }] } },
                { "message": { "tool_calls": [{ "function": { "name": "write", "arguments": r#"{"content":"BBB",{}":"/tmp/b.txt"}"# } }] } }
            ]
        });

        let (result, action) = ToolcallMalformedArgumentsFix::new()
            .apply_with_context(response, &request)
            .expect("one candidate for one slot per call — bijective, both repairable");

        for (choice, marker) in [(0, "AAA"), (1, "BBB")] {
            let args = result["choices"][choice]["message"]["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .unwrap();
            assert!(
                args.contains(r#""file_path""#) && args.contains(marker),
                "choice {choice} repaired"
            );
        }

        match action {
            FixAction::Fixed {
                original_snippet,
                fixed_snippet,
            } => {
                assert!(
                    original_snippet.contains("AAA") && original_snippet.contains("BBB"),
                    "aggregated originals must cover every repaired call, got: {original_snippet}"
                );
                assert!(
                    fixed_snippet.contains("/tmp/a.txt") && fixed_snippet.contains("/tmp/b.txt"),
                    "aggregated fixed snippets must cover every repaired call, got: {fixed_snippet}"
                );
            }
            other => panic!("aggregated action must be Fixed, got {other:?}"),
        }
    }
}
