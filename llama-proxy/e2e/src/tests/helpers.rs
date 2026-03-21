//! Common test helpers and JSON builders

use serde_json::{json, Value};

// ─── Request builders ────────────────────────────────────────────────────────

/// Build a basic non-streaming chat request
pub fn basic_request(prompt: &str) -> Value {
    json!({
        "model": "test-model",
        "messages": [{"role": "user", "content": prompt}],
        "stream": false
    })
}

/// Build a request with tools attached (write_file tool)
pub fn request_with_write_tool(prompt: &str) -> Value {
    json!({
        "model": "test-model",
        "messages": [{"role": "user", "content": prompt}],
        "stream": false,
        "tools": [{
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Write content to a file",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "content": {"type": "string"},
                        "filePath": {"type": "string"}
                    },
                    "required": ["content", "filePath"]
                }
            }
        }]
    })
}

// ─── Response builders ────────────────────────────────────────────────────────

/// Build a normal text completion response from the "backend"
pub fn backend_text_response(content: &str) -> String {
    json!({
        "id": "chatcmpl-test001",
        "object": "chat.completion",
        "created": 1700000000,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        }
    })
    .to_string()
}

/// Build a valid tool call response (no malformation)
pub fn backend_valid_tool_call_response(tool_name: &str, args_json: &str) -> String {
    json!({
        "id": "chatcmpl-test002",
        "object": "chat.completion",
        "created": 1700000000,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call-001",
                    "type": "function",
                    "index": 0,
                    "function": {
                        "name": tool_name,
                        "arguments": args_json
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 20,
            "completion_tokens": 30,
            "total_tokens": 50
        }
    })
    .to_string()
}

/// Build a malformed filePath tool call - duplicate key (the Qwen3 bug)
/// The second "filePath" has a missing colon (real bug pattern)
pub fn backend_duplicate_filepath_response(content: &str, filepath: &str) -> String {
    // Simulates: {"content":"...","filePath":"/foo","filePath""/foo"}
    // This is the actual malformed JSON that Qwen3-Coder emits
    let malformed_args = format!(r#"{{"content":"{content}","filePath":"{filepath}","filePath""{filepath}"}}"#);
    json!({
        "id": "chatcmpl-test003",
        "object": "chat.completion",
        "created": 1700000000,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call-002",
                    "type": "function",
                    "index": 0,
                    "function": {
                        "name": "write_file",
                        "arguments": malformed_args
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 20,
            "completion_tokens": 50,
            "total_tokens": 70
        }
    })
    .to_string()
}

/// Build a malformed filePath response where there are two valid "filePath" keys (valid JSON, wrong schema)
#[allow(dead_code)]
pub fn backend_double_filepath_valid_json_response(content: &str, path1: &str, path2: &str) -> String {
    // Build manually because serde_json deduplicates keys
    format!(
        r#"{{"id":"chatcmpl-test004","object":"chat.completion","created":1700000000,"model":"test-model","choices":[{{"index":0,"message":{{"role":"assistant","content":null,"tool_calls":[{{"id":"call-003","type":"function","index":0,"function":{{"name":"write_file","arguments":"{args}"}}}}]}},"finish_reason":"tool_calls"}}],"usage":{{"prompt_tokens":20,"completion_tokens":50,"total_tokens":70}}}}"#,
        args =
            format!(r#"{{\"content\":\"{content}\",\"filePath\":\"{path1}\",\"filePath\":\"{path2}\"}}"#).replace('\\', "\\\\")
    )
}

/// Build a tool call with null index (should be fixed to 0)
pub fn backend_null_index_response() -> String {
    // Build manually since serde_json would use a proper null
    r#"{"id":"chatcmpl-test005","object":"chat.completion","created":1700000000,"model":"test-model","choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call-004","type":"function","index":null,"function":{"name":"get_info","arguments":"{}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}}"#.to_string()
}

/// Build a tool call with malformed arguments - the {}":/value pattern
///
/// Qwen3-Coder sometimes emits `{}":` where a quoted property name should be.
/// Example: `{"content":"#!/bin/bash",{}":"/path/script.sh"}`
/// The `{}` before `":` is where the property name should be (e.g. "filePath").
///
/// Note: The fix requires a request with tool schemas to know what parameter to substitute.
/// Use `request_with_write_tool()` as the client request for this backend response.
pub fn backend_malformed_arguments_response() -> String {
    // Malformed: {}": appears where "filePath": should be
    // The aggressive parser extracts "content" and "{}" as keys from this malformed JSON
    // Note: content must NOT contain "# as that would close our r##"..."## raw string
    // The pattern ,{}":/path is what the fix detects via regex [,\{]\{\}":\s*
    let malformed_args = r##"{"content":"echo hello world",{}":"/scripts/hello.sh"}"##;
    // Build manually to avoid json! macro issues with the {} characters in malformed_args
    format!(
        r#"{{"id":"chatcmpl-test006","object":"chat.completion","created":1700000000,"model":"test-model","choices":[{{"index":0,"message":{{"role":"assistant","content":null,"tool_calls":[{{"id":"call-005","type":"function","index":0,"function":{{"name":"write_file","arguments":"{args}"}}}}]}},"finish_reason":"tool_calls"}}],"usage":{{"prompt_tokens":10,"completion_tokens":15,"total_tokens":25}}}}"#,
        args = malformed_args.replace('"', "\\\"")
    )
}

// ─── Assertion helpers ────────────────────────────────────────────────────────

/// Assert that a string is valid JSON, return parsed value
pub fn assert_valid_json(s: &str, label: &str) -> anyhow::Result<Value> {
    serde_json::from_str(s).map_err(|e| anyhow::anyhow!("{} is not valid JSON: {}\nContent: {}", label, e, s))
}

/// Assert two strings are equal, with context on failure
pub fn assert_eq_str(actual: &str, expected: &str, label: &str) -> anyhow::Result<()> {
    if actual != expected {
        Err(anyhow::anyhow!("{label}: expected {:?} but got {:?}", expected, actual))
    } else {
        Ok(())
    }
}

/// Assert condition is true, with message
pub fn assert_true(cond: bool, msg: &str) -> anyhow::Result<()> {
    if !cond {
        Err(anyhow::anyhow!("{}", msg))
    } else {
        Ok(())
    }
}
