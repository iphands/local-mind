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
//! 3. Replaces each malformed property name with the next missing schema name, in order

use super::{FixAction, FixError, ResponseFix};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

pub struct ToolcallMalformedArgumentsFix {
    /// Regex to detect malformed property names like `{}":`
    malformed_pattern: Regex,
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

    /// Attempt to fix malformed arguments using tool schema
    /// Returns Some(fixed_args) on success, None on failure
    fn fix_arguments(&self, args_str: &str, tool_name: &str, schemas: &HashMap<String, Vec<String>>) -> Option<String> {
        // Check if arguments contain malformed pattern
        if !self.malformed_pattern.is_match(args_str) {
            return None;
        }

        // Get schema for this tool
        let schema_params = schemas.get(tool_name)?;

        // Try to parse the malformed JSON to extract the value associated with `{}":`
        // Pattern: ..."key":"value",{}"="other_value"...
        // We need to find what parameters are present and what's missing

        // First, try to parse as-is to see what we get
        let parsed = self.aggressive_parse_json(args_str)?;

        // Find which schema parameters are missing from parsed object, in schema order.
        let parsed_keys: Vec<String> = parsed.keys().map(|k| k.to_string()).collect();
        let missing_params: Vec<&str> = schema_params
            .iter()
            .filter(|p| !parsed_keys.contains(p))
            .map(|p| p.as_str())
            .collect();

        if missing_params.is_empty() {
            return None;
        }

        // Splice the missing keys over the empty-key `{}` slots positionally: occurrence N
        // gets missing key N. A slot beyond the missing count keeps its `{}` token, so the
        // result fails the validity gate below and the original arguments are preserved.
        let fixed_args = self.splice_empty_key_slots(args_str, &missing_params);

        if serde_json::from_str::<Value>(&fixed_args).is_ok() {
            Some(fixed_args)
        } else {
            None
        }
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

    /// Aggressively parse JSON, trying to extract key-value pairs even from malformed input
    fn aggressive_parse_json(&self, json_str: &str) -> Option<HashMap<String, Value>> {
        // First try normal parsing
        if let Ok(val) = serde_json::from_str::<Value>(json_str) {
            if let Some(obj) = val.as_object() {
                return Some(obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
            }
        }

        // Try to extract key-value pairs manually
        let mut result = HashMap::new();

        // Pattern: "key":"value" (quoted key, string value)
        let str_pattern = Regex::new(r#""([^"]+)"\s*:\s*"([^"]*)""#).ok()?;
        for cap in str_pattern.captures_iter(json_str) {
            if let (Some(key), Some(val)) = (cap.get(1), cap.get(2)) {
                result.insert(key.as_str().to_string(), Value::String(val.as_str().to_string()));
            }
        }

        // Pattern: unquoted_key":"value" (UNQUOTED key like {}, string value)
        // Matches sequences like: ,{}"="value" or {{}":"value"
        let unquoted_str_pattern = Regex::new(r#"[,\{]([^\s"]+)"\s*:\s*"([^"]*)""#).ok()?;
        for cap in unquoted_str_pattern.captures_iter(json_str) {
            if let Some(key) = cap.get(1) {
                if key.as_str() == "{}" {
                    continue;
                }
                if let Some(val) = cap.get(2) {
                    result.insert(key.as_str().to_string(), Value::String(val.as_str().to_string()));
                }
            }
        }

        // Pattern: "key":number
        let num_pattern = Regex::new(r#""([^"]+)"\s*:\s*(-?[0-9]+\.?[0-9]*)"#).ok()?;
        for cap in num_pattern.captures_iter(json_str) {
            if let (Some(key), Some(val)) = (cap.get(1), cap.get(2)) {
                if let Ok(num) = val.as_str().parse::<f64>() {
                    result.insert(key.as_str().to_string(), serde_json::json!(num));
                }
            }
        }

        // Pattern: "key":true/false
        let bool_pattern = Regex::new(r#""([^"]+)"\s*:\s*(true|false)"#).ok()?;
        for cap in bool_pattern.captures_iter(json_str) {
            if let (Some(key), Some(val)) = (cap.get(1), cap.get(2)) {
                let bool_val = val.as_str() == "true";
                result.insert(key.as_str().to_string(), Value::Bool(bool_val));
            }
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Fix tool calls in response using request context
    fn fix_response_with_context(&self, mut response: Value, request: &Value) -> (Value, FixAction) {
        let schemas = Self::extract_tool_schemas(request);

        if schemas.is_empty() {
            tracing::warn!(
                fix_name = self.name(),
                "No tool schemas in request - cannot fix malformed arguments without context"
            );
            return (response, FixAction::NotApplicable);
        }

        let mut overall_action = FixAction::NotApplicable;

        // Navigate to tool_calls in response
        if let Some(choices) = response.get_mut("choices").and_then(|c| c.as_array_mut()) {
            for choice in choices {
                if let Some(message) = choice.get_mut("message") {
                    if let Some(tool_calls) = message.get_mut("tool_calls").and_then(|tc| tc.as_array_mut()) {
                        for tool_call in tool_calls {
                            if let Some(function) = tool_call.get_mut("function") {
                                let tool_name = function.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();

                                if let Some(args) = function.get("arguments").and_then(|a| a.as_str()) {
                                    let original = args.to_string();
                                    if let Some(fixed_args) = self.fix_arguments(args, &tool_name, &schemas) {
                                        function["arguments"] = Value::String(fixed_args.clone());
                                        overall_action = FixAction::fixed(&original, &fixed_args);
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

    /// Fix tool calls in streaming delta using request context
    fn fix_stream_with_context(&self, mut chunk: Value, request: &Value) -> (Value, FixAction) {
        let schemas = Self::extract_tool_schemas(request);

        if schemas.is_empty() {
            tracing::debug!(
                fix_name = self.name(),
                "No tool schemas in request - cannot fix malformed arguments in streaming"
            );
            return (chunk, FixAction::NotApplicable);
        }

        let mut overall_action = FixAction::NotApplicable;

        // Navigate to tool_calls in delta
        if let Some(choices) = chunk.get_mut("choices").and_then(|c| c.as_array_mut()) {
            for choice in choices {
                if let Some(delta) = choice.get_mut("delta") {
                    if let Some(tool_calls) = delta.get_mut("tool_calls").and_then(|tc| tc.as_array_mut()) {
                        for tool_call in tool_calls {
                            if let Some(function) = tool_call.get_mut("function") {
                                let tool_name = function.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();

                                if let Some(args) = function.get("arguments").and_then(|a| a.as_str()) {
                                    // For streaming, we might get partial JSON
                                    // Only try to fix if we see the malformed pattern
                                    if self.malformed_pattern.is_match(args) {
                                        let original = args.to_string();
                                        if let Some(fixed_args) = self.fix_arguments(args, &tool_name, &schemas) {
                                            function["arguments"] = Value::String(fixed_args.clone());
                                            overall_action = FixAction::fixed(&original, &fixed_args);
                                        }
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
}

impl ResponseFix for ToolcallMalformedArgumentsFix {
    fn name(&self) -> &str {
        "toolcall_malformed_arguments"
    }

    fn description(&self) -> &str {
        "Fixes malformed tool call arguments with invalid property names like `{}\"`"
    }

    fn applies(&self, response: &Value) -> bool {
        // Check if response has tool_calls
        response
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|msg| msg.get("tool_calls"))
            .is_some()
    }

    fn apply(&self, response: Value) -> (Value, FixAction) {
        // Without context, we can't fix - just pass through
        (response, FixAction::NotApplicable)
    }

    fn apply_stream(&self, chunk: Value) -> (Value, FixAction) {
        // Without context, we can't fix - just pass through
        (chunk, FixAction::NotApplicable)
    }

    fn applies_with_context(&self, response: &Value, request: &Value) -> bool {
        // Check if response has tool_calls AND request has tools
        let has_tool_calls = response
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|msg| msg.get("tool_calls"))
            .is_some();

        let has_tools = request.get("tools").is_some();

        has_tool_calls && has_tools
    }

    fn apply_with_context(&self, response: Value, request: &Value) -> Result<(Value, FixAction), FixError> {
        Ok(self.fix_response_with_context(response, request))
    }

    fn apply_stream_with_context(&self, chunk: Value, request: &Value) -> (Value, FixAction) {
        self.fix_stream_with_context(chunk, request)
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

        let fixed = fix.fix_arguments(malformed, "write", &schemas);

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
        assert!(result.is_none());
    }

    #[test]
    fn test_fix_arguments_unknown_tool() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let schemas = HashMap::new(); // Empty schemas

        let malformed = "{\"content\":\"test\",{}\":\"/path\"}";

        let result = fix.fix_arguments(malformed, "unknown_tool", &schemas);

        // Should return None when tool not in schema
        assert!(result.is_none());
    }

    #[test]
    fn test_aggressive_parse_json() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let malformed = "{\"content\":\"test value\",{}\":\"/some/path\"}";

        let parsed = fix.aggressive_parse_json(malformed);

        assert!(parsed.is_some());
        let parsed = parsed.unwrap();

        // Should have extracted the valid key-value pairs
        assert!(parsed.contains_key("content"));
        assert_eq!(parsed["content"].as_str().unwrap(), "test value");

        // {} garbage key should NOT be extracted (bug #3 fix)
        assert!(!parsed.contains_key("{}"));
    }

    #[test]
    fn test_applies_with_context() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let request = json!({
            "tools": [{"type": "function", "function": {"name": "write"}}]
        });

        let response_with_tools = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "write",
                            "arguments": "{}"
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

        let (fixed, action) = fix.fix_response_with_context(response, &request);

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
    fn test_fix_stream_with_context() {
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

        let chunk = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "function": {
                            "name": "write",
                            "arguments": "{\"content\":\"data\",{}\":\"/path.txt\"}"
                        }
                    }]
                }
            }]
        });

        let (fixed, action) = fix.fix_stream_with_context(chunk, &request);

        let args = fixed["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();

        assert!(args.contains("\"file_path\":"));
        assert!(!args.contains("{}\":"));
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

        let fixed = fix.fix_arguments(malformed, "write", &schemas);

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

    // Given two empty-key slots and two missing keys, occurrence N gets missing key N in
    // schema order (distinct keys, not a single key reused for every slot).
    #[test]
    fn test_two_empty_key_slots_assigned_in_schema_order() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let schemas = schema_map(&["path", "mode", "content"]);

        let malformed = r#"{"content":"data",{}":"/tmp/a",{}":"hello"}"#;
        let fixed = fix
            .fix_arguments(malformed, "write", &schemas)
            .expect("two slots / two missing keys must be fixed");

        assert_eq!(fixed, r#"{"content":"data","path":"/tmp/a","mode":"hello"}"#);
        let parsed: Value = serde_json::from_str(&fixed).expect("fixed args are valid JSON");
        assert_eq!(parsed["path"].as_str().unwrap(), "/tmp/a");
        assert_eq!(parsed["mode"].as_str().unwrap(), "hello");
        assert!(!fixed.contains("{}\":"));
    }

    // Given one slot and two missing keys, the slot receives the first missing key; the
    // second missing key has no slot and stays absent.
    #[test]
    fn test_one_slot_with_multiple_missing_takes_first_key() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let schemas = schema_map(&["path", "mode", "content"]);

        let malformed = r#"{"content":"x",{}":"/tmp/a"}"#;
        let fixed = fix
            .fix_arguments(malformed, "write", &schemas)
            .expect("single slot must be fixed with the first missing key");

        assert_eq!(fixed, r#"{"content":"x","path":"/tmp/a"}"#);
    }

    // Given more slots than missing keys, the splicer fills the first slot and leaves the
    // leftover slot's `{}` token untouched, so the result is invalid JSON and
    // fix_arguments returns None (the caller keeps the original arguments).
    #[test]
    fn test_more_slots_than_missing_leaves_leftover_untouched() {
        let fix = ToolcallMalformedArgumentsFix::new();

        // leftover slot keeps its `{}` token, byte-verbatim
        let spliced = fix.splice_empty_key_slots(r#"{"content":"x",{}":"/tmp/a",{}":"/tmp/b"}"#, &["path"]);
        assert_eq!(spliced, r#"{"content":"x","path":"/tmp/a",{}":"/tmp/b"}"#);

        // invalid leftover fails validation -> None -> original preserved
        let schemas = schema_map(&["path", "content"]);
        let result = fix.fix_arguments(r#"{"content":"x",{}":"/tmp/a",{}":"/tmp/b"}"#, "write", &schemas);
        assert!(result.is_none(), "leftover empty-key slot must fail validation");
    }

    // With no empty-key slots, the splicer returns the input byte-identical and
    // fix_arguments makes no change.
    #[test]
    fn test_no_slots_output_is_byte_identical() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let clean = r#"{"path":"/x","content":"y"}"#;
        assert_eq!(fix.splice_empty_key_slots(clean, &["z"]), clean);

        let schemas = schema_map(&["path", "content"]);
        assert!(fix.fix_arguments(clean, "write", &schemas).is_none());
    }

    // Naive-regex trap: an empty-string key `"":` living inside a string VALUE is not
    // an empty-key slot and must not be touched (the slot form is the brace token `{}`).
    #[test]
    fn test_empty_string_key_in_value_is_not_a_slot() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let decoy = r#"{"x":"{\"\": 1}"}"#;
        assert!(!fix.malformed_pattern.is_match(decoy));

        let schemas = schema_map(&["path"]);
        assert!(fix.fix_arguments(decoy, "write", &schemas).is_none());
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
            .expect("top-level slot must be fixed");
        assert_eq!(fixed, r#"{"outer":{"":1},"content":"x","path":"/tmp/a"}"#);
    }

    // Byte-splice proof: CJK values before and after the slots keep their multibyte
    // payloads verbatim; the splice lands on ASCII structural boundaries regardless.
    #[test]
    fn test_cjk_two_slots_spliced_byte_verbatim() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let schemas = schema_map(&["path", "mode", "content"]);

        let malformed = r#"{"content":"你好世界",{}":"/路径/文件.rs",{}":"第二"}"#;
        let fixed = fix
            .fix_arguments(malformed, "write", &schemas)
            .expect("two slots must be fixed in order");
        assert_eq!(fixed, r#"{"content":"你好世界","path":"/路径/文件.rs","mode":"第二"}"#);
    }

    // Byte-splice proof with escaped quotes in a value: the value region is copied
    // verbatim (the `\"` must survive) while both slots are filled positionally.
    #[test]
    fn test_escaped_quote_value_preserved_verbatim() {
        let fix = ToolcallMalformedArgumentsFix::new();
        let schemas = schema_map(&["path", "mode", "content"]);

        let malformed = r#"{"content":"say \"hi\"",{}":"/tmp/a",{}":"b"}"#;
        let fixed = fix
            .fix_arguments(malformed, "write", &schemas)
            .expect("two slots must be fixed in order");
        assert_eq!(fixed, r#"{"content":"say \"hi\"","path":"/tmp/a","mode":"b"}"#);
    }
}
