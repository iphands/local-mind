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
//! 3. Replaces the malformed property name with the correct one from the schema

use super::ResponseFix;
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
            // Matches: ,{}":" or {{}":
            // This pattern detects the specific case where {} is used as a property name
            malformed_pattern: Regex::new(r#"[,\{]\{\}":\s*"#).unwrap(),
        }
    }

    /// Extract tool schemas from request
    fn extract_tool_schemas(request: &Value) -> HashMap<String, Vec<String>> {
        let mut schemas = HashMap::new();

        if let Some(tools) = request.get("tools").and_then(|t| t.as_array()) {
            for tool in tools {
                if let Some(function) = tool.get("function") {
                    let name = function
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();

                    let parameters = function
                        .get("parameters")
                        .and_then(|p| p.get("properties"))
                        .and_then(|props| props.as_object())
                        .map(|obj| {
                            obj.keys()
                                .map(|k| k.to_string())
                                .collect::<Vec<String>>()
                        })
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
    fn fix_arguments(&self, args_str: &str, tool_name: &str, schemas: &HashMap<String, Vec<String>>) -> Option<String> {
        // Check if arguments contain malformed pattern
        if !self.malformed_pattern.is_match(args_str) {
            return None;
        }

        tracing::warn!(
            fix_name = self.name(),
            tool_name = tool_name,
            malformed_args = args_str,
            "Detected malformed arguments with {{}}\" pattern"
        );

        // Get schema for this tool
        let schema_params = schemas.get(tool_name)?;

        // Try to parse the malformed JSON to extract the value associated with `{}":`
        // Pattern: ..."key":"value",{}"="other_value"...
        // We need to find what parameters are present and what's missing

        // First, try to parse as-is to see what we get
        let parsed = match self.aggressive_parse_json(args_str) {
            Some(obj) => obj,
            None => {
                tracing::error!(
                    fix_name = self.name(),
                    tool_name = tool_name,
                    malformed_args = args_str,
                    "Could not parse malformed arguments even with aggressive parsing"
                );
                return None;
            }
        };

        // Find which schema parameters are missing from parsed object
        let parsed_keys: Vec<String> = parsed.keys().map(|k| k.to_string()).collect();
        let missing_params: Vec<&String> = schema_params
            .iter()
            .filter(|p| !parsed_keys.contains(p) && *p != "{}")
            .collect();

        if missing_params.is_empty() {
            tracing::error!(
                fix_name = self.name(),
                tool_name = tool_name,
                "No missing parameters found, cannot determine replacement"
            );
            return None;
        }

        // If there's exactly one missing parameter and one `{}"` key, it's clear what to do
        if missing_params.len() == 1 && parsed.contains_key("{}") {
            let correct_param = missing_params[0];

            // Replace the unquoted {} with quoted correct parameter
            // Pattern: ,{}"= becomes ,"file_path":
            let fixed_args = args_str.replace("{}\":", &format!("\"{}\":", correct_param));

            // Validate the fixed JSON is actually valid
            if serde_json::from_str::<Value>(&fixed_args).is_ok() {
                tracing::info!(
                    fix_name = self.name(),
                    tool_name = tool_name,
                    correct_param = correct_param,
                    original_args = args_str,
                    fixed_args = &fixed_args,
                    "Fixed malformed argument: replaced {{}}\" with correct parameter"
                );
                return Some(fixed_args);
            } else {
                tracing::error!(
                    fix_name = self.name(),
                    tool_name = tool_name,
                    fixed_args = &fixed_args,
                    "Fixed arguments are still invalid JSON"
                );
                return None;
            }
        }

        // Multiple missing parameters - try heuristic matching
        if missing_params.len() > 1 && parsed.contains_key("{}") {
            tracing::debug!(
                missing = ?missing_params,
                "Multiple missing parameters, trying heuristic matching"
            );

            // Common heuristics for parameter names
            let heuristics = [
                "file_path",
                "path",
                "filepath",
                "filename",
                "output",
                "output_path",
                "destination",
                "target",
            ];

            for guess in &heuristics {
                if missing_params.iter().any(|p| p.as_str() == *guess) {
                    let fixed_args = args_str.replace("{}\":", &format!("\"{}\":", guess));
                    if serde_json::from_str::<Value>(&fixed_args).is_ok() {
                        tracing::info!(
                            fix_name = self.name(),
                            tool_name = tool_name,
                            guessed_param = guess,
                            original_args = args_str,
                            fixed_args = &fixed_args,
                            "Fixed malformed argument using heuristic"
                        );
                        return Some(fixed_args);
                    }
                }
            }
        }

        None
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
            if let (Some(key), Some(val)) = (cap.get(1), cap.get(2)) {
                result.insert(key.as_str().to_string(), Value::String(val.as_str().to_string()));
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
    fn fix_response_with_context(&self, mut response: Value, request: &Value) -> Value {
        let schemas = Self::extract_tool_schemas(request);

        if schemas.is_empty() {
            tracing::warn!(
                fix_name = self.name(),
                "No tool schemas in request - cannot fix malformed arguments without context"
            );
            return response;
        }

        // Navigate to tool_calls in response
        if let Some(choices) = response.get_mut("choices").and_then(|c| c.as_array_mut()) {
            for choice in choices {
                if let Some(message) = choice.get_mut("message") {
                    if let Some(tool_calls) = message.get_mut("tool_calls").and_then(|tc| tc.as_array_mut()) {
                        for tool_call in tool_calls {
                            if let Some(function) = tool_call.get_mut("function") {
                                let tool_name = function
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("")
                                    .to_string();

                                if let Some(args) = function.get("arguments").and_then(|a| a.as_str()) {
                                    if let Some(fixed_args) = self.fix_arguments(args, &tool_name, &schemas) {
                                        function["arguments"] = Value::String(fixed_args);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        response
    }

    /// Fix tool calls in streaming delta using request context
    fn fix_stream_with_context(&self, mut chunk: Value, request: &Value) -> Value {
        let schemas = Self::extract_tool_schemas(request);

        if schemas.is_empty() {
            tracing::debug!(
                fix_name = self.name(),
                "No tool schemas in request - cannot fix malformed arguments in streaming"
            );
            return chunk;
        }

        // Navigate to tool_calls in delta
        if let Some(choices) = chunk.get_mut("choices").and_then(|c| c.as_array_mut()) {
            for choice in choices {
                if let Some(delta) = choice.get_mut("delta") {
                    if let Some(tool_calls) = delta.get_mut("tool_calls").and_then(|tc| tc.as_array_mut()) {
                        for tool_call in tool_calls {
                            if let Some(function) = tool_call.get_mut("function") {
                                let tool_name = function
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("")
                                    .to_string();

                                if let Some(args) = function.get("arguments").and_then(|a| a.as_str()) {
                                    // For streaming, we might get partial JSON
                                    // Only try to fix if we see the malformed pattern
                                    if self.malformed_pattern.is_match(args) {
                                        if let Some(fixed_args) = self.fix_arguments(args, &tool_name, &schemas) {
                                            function["arguments"] = Value::String(fixed_args);
                                        }
                                    }
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

    fn apply(&self, response: Value) -> Value {
        // Without context, we can't fix - just pass through
        response
    }

    fn apply_stream(&self, chunk: Value) -> Value {
        // Without context, we can't fix - just pass through
        chunk
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

    fn apply_with_context(&self, response: Value, request: &Value) -> Value {
        self.fix_response_with_context(response, request)
    }

    fn apply_stream_with_context(&self, chunk: Value, request: &Value) -> Value {
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
        assert!(!fix.malformed_pattern.is_match("{\"file_path\":\"test\",\"content\":\"data\"}"));
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

        // Should have extracted the malformed key
        assert!(parsed.contains_key("{}"));
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

        let fixed = fix.fix_response_with_context(response, &request);

        let args = fixed["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();

        // Should have fixed the malformed argument
        assert!(args.contains("\"file_path\":"));
        assert!(!args.contains("{}\":"));

        // Should be valid JSON
        let parsed: Result<Value, _> = serde_json::from_str(args);
        assert!(parsed.is_ok());
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

        let fixed = fix.fix_stream_with_context(chunk, &request);

        let args = fixed["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();

        assert!(args.contains("\"file_path\":"));
        assert!(!args.contains("{}\":"));
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
        let result = fix.apply(response.clone());
        assert_eq!(result, response);
    }

    #[test]
    fn test_heuristic_matching() {
        let fix = ToolcallMalformedArgumentsFix::new();

        let mut schemas = HashMap::new();
        // Multiple parameters, but we'll guess file_path
        schemas.insert(
            "write".to_string(),
            vec![
                "file_path".to_string(),
                "content".to_string(),
                "mode".to_string(),
            ],
        );

        let malformed = "{\"content\":\"data\",\"mode\":\"0755\",{}\":\"/tmp/file\"}";

        let fixed = fix.fix_arguments(malformed, "write", &schemas);

        assert!(fixed.is_some());
        let fixed = fixed.unwrap();

        // Should have guessed file_path
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
        let fixed = fix.apply_with_context(response, &request);

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
    }
}
