//! Basic proxy behavior tests - pass-through, text responses, streaming synthesis

use crate::backend::{drain_requests, queue_response};
use crate::client::{send_non_streaming, send_streaming};
use crate::runner::TestContext;
use crate::types::MockResponse;

use super::helpers::*;

/// Simple text response - non-streaming
/// Tests: proxy passes complete JSON through unchanged, no fixes needed
pub async fn test_simple_text_non_streaming(ctx: TestContext) -> anyhow::Result<()> {
    queue_response(&ctx.backend_state, MockResponse::json(backend_text_response("Hello, world!")));

    let resp = send_non_streaming(&ctx.http_client, &ctx.proxy_addr, basic_request("say hello")).await?;

    assert_true(resp.status == 200, &format!("Expected 200, got {}", resp.status))?;
    let content = resp
        .get_str("choices.0.message.content")
        .ok_or_else(|| anyhow::anyhow!("Missing choices[0].message.content"))?;
    assert_eq_str(content, "Hello, world!", "message content")?;

    // Verify proxy sent stream:false to backend
    let reqs = drain_requests(&ctx.backend_state);
    assert_true(!reqs.is_empty(), "Backend received no request")?;
    let stream_val = reqs[0].body.get("stream").and_then(|v| v.as_bool());
    assert_true(
        stream_val == Some(false),
        &format!("Proxy should force stream:false to backend, got {:?}", stream_val),
    )?;

    Ok(())
}

/// Simple text response - streaming synthesis
/// Tests: client requests stream:true, proxy forces non-streaming to backend,
/// then synthesizes SSE chunks from the complete JSON
pub async fn test_simple_text_streaming_synthesis(ctx: TestContext) -> anyhow::Result<()> {
    queue_response(&ctx.backend_state, MockResponse::json(backend_text_response("Count: 1 2 3")));

    let resp = send_streaming(&ctx.http_client, &ctx.proxy_addr, basic_request("count")).await?;

    assert_true(resp.has_done_marker(), "SSE stream must end with [DONE]")?;
    assert_true(!resp.data_events().is_empty(), "Stream has no data events")?;

    // Backend should have received stream:false
    let reqs = drain_requests(&ctx.backend_state);
    assert_true(!reqs.is_empty(), "Backend received no request")?;
    let stream_val = reqs[0].body.get("stream").and_then(|v| v.as_bool());
    assert_true(
        stream_val == Some(false),
        "Proxy must forward stream:false to backend even when client wants streaming",
    )?;

    // Client should be able to accumulate complete content
    let content = resp.accumulated_content();
    assert_true(!content.is_empty(), "Accumulated content should not be empty")?;
    assert_true(
        content.contains("Count: 1 2 3"),
        &format!("Expected full content in stream, got: {:?}", content),
    )?;

    Ok(())
}

/// Streaming synthesis preserves finish_reason
pub async fn test_streaming_finish_reason(ctx: TestContext) -> anyhow::Result<()> {
    queue_response(&ctx.backend_state, MockResponse::json(backend_text_response("Done")));

    let resp = send_streaming(&ctx.http_client, &ctx.proxy_addr, basic_request("hi")).await?;

    assert_true(resp.has_done_marker(), "Must end with [DONE]")?;
    let finish = resp.finish_reason();
    assert_true(
        finish.as_deref() == Some("stop"),
        &format!("Expected finish_reason=stop, got: {:?}", finish),
    )?;

    Ok(())
}

/// Backend error passes through to client
pub async fn test_backend_error_passthrough(ctx: TestContext) -> anyhow::Result<()> {
    queue_response(
        &ctx.backend_state,
        MockResponse::error(503, r#"{"error":"model overloaded"}"#),
    );

    let url = format!("http://{}/v1/chat/completions", ctx.proxy_addr);
    let resp = ctx
        .http_client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&basic_request("hi"))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Request failed: {}", e))?;

    // Proxy should propagate the error status
    assert_true(
        resp.status().is_server_error(),
        &format!("Expected 5xx error, got {}", resp.status()),
    )?;

    Ok(())
}

/// Proxy forces stream:false even when client sends stream:true
/// This is the fundamental behavior we rely on for fix application
pub async fn test_proxy_forces_non_streaming_to_backend(ctx: TestContext) -> anyhow::Result<()> {
    queue_response(&ctx.backend_state, MockResponse::json(backend_text_response("test")));

    // Send with stream:true
    let mut req = basic_request("test");
    req["stream"] = serde_json::Value::Bool(true);
    let _ = send_streaming(&ctx.http_client, &ctx.proxy_addr, req).await?;

    let reqs = drain_requests(&ctx.backend_state);
    assert_true(!reqs.is_empty(), "Backend got no request")?;
    let stream_val = reqs[0].body.get("stream").and_then(|v| v.as_bool());
    assert_true(
        stream_val == Some(false),
        &format!("Backend must receive stream:false, got {:?}", stream_val),
    )?;

    Ok(())
}

/// SSE stream has correct structure: role chunk first, content chunks, final chunk, [DONE]
pub async fn test_sse_chunk_structure(ctx: TestContext) -> anyhow::Result<()> {
    queue_response(&ctx.backend_state, MockResponse::json(backend_text_response("Hello")));

    let resp = send_streaming(&ctx.http_client, &ctx.proxy_addr, basic_request("hi")).await?;

    assert_true(resp.has_done_marker(), "Must end with [DONE]")?;

    let data_events = resp.data_events();
    assert_true(data_events.len() >= 2, "Need at least 2 data events (role + content)")?;

    // First event should have role: assistant
    let first = data_events[0].parse_json()?;
    let role = first.pointer("/choices/0/delta/role").and_then(|v| v.as_str());
    assert_true(
        role == Some("assistant"),
        &format!("First chunk should have role=assistant, got {:?}", role),
    )?;

    // Some chunk should have content
    let has_content = data_events.iter().any(|e| {
        e.parse_json()
            .ok()
            .and_then(|j| j.pointer("/choices/0/delta/content").cloned())
            .and_then(|v| v.as_str().map(|s| !s.is_empty()))
            .unwrap_or(false)
    });
    assert_true(has_content, "At least one chunk should have content")?;

    // Final data event should have finish_reason
    let last_data = data_events.last().unwrap().parse_json()?;
    let finish = last_data.pointer("/choices/0/finish_reason");
    assert_true(finish.is_some(), "Final chunk should have finish_reason")?;

    Ok(())
}
