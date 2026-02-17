//! Test registry - all test cases are registered here

pub mod basic;
pub mod helpers;
pub mod passthrough;
pub mod toolcall;

use crate::runner::TestCase;

/// Build and return all test cases
///
/// Tests are grouped by category. Each test:
/// 1. Queues a mock backend response (what llama.cpp would return)
/// 2. Sends a request to the REAL proxy
/// 3. Validates the response
pub fn all_tests() -> Vec<TestCase> {
    macro_rules! test {
        ($name:expr, $desc:expr, $func:path) => {
            TestCase {
                name: $name,
                description: $desc,
                run: Box::new(|ctx| Box::pin($func(ctx))),
            }
        };
    }

    vec![
        // ── Basic behavior ────────────────────────────────────────────────────
        test!(
            "basic/simple_text_non_streaming",
            "Non-streaming text response passes through correctly",
            basic::test_simple_text_non_streaming
        ),
        test!(
            "basic/simple_text_streaming_synthesis",
            "Streaming synthesis: proxy gets complete JSON, synthesizes SSE for client",
            basic::test_simple_text_streaming_synthesis
        ),
        test!(
            "basic/streaming_finish_reason",
            "Synthesized SSE stream has correct finish_reason",
            basic::test_streaming_finish_reason
        ),
        test!(
            "basic/proxy_forces_non_streaming_to_backend",
            "Proxy always sends stream:false to backend, even when client wants streaming",
            basic::test_proxy_forces_non_streaming_to_backend
        ),
        test!(
            "basic/sse_chunk_structure",
            "SSE stream has proper OpenAI chunk structure: role, content, finish",
            basic::test_sse_chunk_structure
        ),
        test!(
            "basic/backend_error_passthrough",
            "Backend error responses (5xx) are forwarded to client",
            basic::test_backend_error_passthrough
        ),

        // ── Tool call pass-through ─────────────────────────────────────────────
        test!(
            "toolcall/valid_non_streaming",
            "Valid tool call passes through unchanged (non-streaming)",
            toolcall::test_valid_toolcall_non_streaming
        ),
        test!(
            "toolcall/valid_streaming",
            "Valid tool call: synthesized SSE has complete valid JSON args",
            toolcall::test_valid_toolcall_streaming
        ),
        test!(
            "toolcall/no_fix_when_not_needed",
            "Non-write-file tool calls are not incorrectly modified",
            toolcall::test_no_fix_when_not_needed
        ),
        test!(
            "toolcall/multiple_tool_calls",
            "Multiple tool calls all pass through correctly",
            toolcall::test_multiple_tool_calls
        ),
        test!(
            "toolcall/empty_arguments",
            "Empty {} arguments are valid and pass through",
            toolcall::test_empty_arguments
        ),

        // ── Bad filePath fix (Qwen3-Coder bug) ─────────────────────────────────
        test!(
            "toolcall/bad_filepath/fixed_non_streaming",
            "Duplicate filePath key is fixed in non-streaming response",
            toolcall::test_bad_filepath_fixed_non_streaming
        ),
        test!(
            "toolcall/bad_filepath/fixed_streaming",
            "Duplicate filePath fixed; synthesized SSE accumulates to valid JSON",
            toolcall::test_bad_filepath_fixed_streaming
        ),
        test!(
            "toolcall/bad_filepath/realworld_paths",
            "Fix works with real-world absolute paths (tsx, deep dirs)",
            toolcall::test_bad_filepath_realworld_paths
        ),
        test!(
            "toolcall/bad_filepath/special_chars",
            "Fix handles filePaths with spaces and Unicode",
            toolcall::test_filepath_with_special_chars
        ),

        // ── Null index fix ──────────────────────────────────────────────────────
        test!(
            "toolcall/null_index/fixed_non_streaming",
            "Tool call with null index gets index=0 in non-streaming response",
            toolcall::test_null_index_fixed
        ),
        test!(
            "toolcall/null_index/fixed_streaming",
            "Tool call with null index gets integer index in synthesized SSE",
            toolcall::test_null_index_fixed_streaming
        ),

        // ── Malformed arguments fix ─────────────────────────────────────────────
        test!(
            "toolcall/malformed_args/fixed",
            r#"Tool call with {}"" pattern is fixed to valid JSON"#,
            toolcall::test_malformed_arguments_fixed
        ),

        // ── Pass-through endpoints ───────────────────────────────────────────────
        test!(
            "passthrough/health",
            "/health proxies to backend and returns status:ok",
            passthrough::test_health_passthrough
        ),
        test!(
            "passthrough/v1_health",
            "/v1/health proxies to backend and returns status:ok",
            passthrough::test_v1_health_passthrough
        ),
        test!(
            "passthrough/slots",
            "/slots proxies to backend and returns slot array",
            passthrough::test_slots_passthrough
        ),
        test!(
            "passthrough/props",
            "/props proxies to backend and returns server properties",
            passthrough::test_props_passthrough
        ),
        test!(
            "passthrough/models",
            "/v1/models proxies to backend and returns model list",
            passthrough::test_models_passthrough
        ),
        test!(
            "passthrough/not_modified",
            "Pass-through endpoints are not modified by proxy fix logic",
            passthrough::test_passthrough_not_modified
        ),
    ]
}
