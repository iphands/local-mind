//! Concurrent request limiting tests
//!
//! These exercise `server.max_concurrent_requests`: that the proxy sheds load once
//! it is saturated, that it says so in a way clients can act on, that permits are
//! actually returned afterwards, and that monitoring routes stay reachable while
//! completions are being rejected.

use crate::backend::{clear_queued_responses, queue_response};
use crate::client::{send_get, send_non_streaming};
use crate::runner::TestContext;
use crate::types::MockResponse;

use super::helpers::*;

/// Must match `server.max_concurrent_requests` in test_configs/proxy_fixes_on.yaml.
const TEST_MAX_CONCURRENT: usize = 4;

/// How long the mock backend holds each request. Long enough that an entire burst
/// is in flight at once, short enough to keep the suite fast.
const HOLD_MS: u64 = 1_500;

/// Fire `count` completion requests at the proxy at the same time, returning each status.
async fn burst(ctx: &TestContext, count: usize) -> anyhow::Result<Vec<u16>> {
    let handles: Vec<_> = (0..count)
        .map(|i| {
            let client = ctx.http_client.clone();
            let addr = ctx.proxy_addr.clone();
            tokio::spawn(async move { send_non_streaming(&client, &addr, basic_request(&format!("burst-{i}"))).await })
        })
        .collect();

    let mut statuses = Vec::with_capacity(count);
    for h in handles {
        statuses.push(h.await??.status);
    }
    Ok(statuses)
}

/// Queue enough slow backend responses to occupy every permit for the whole burst.
///
/// Only admitted requests reach the backend - shed ones are answered by the proxy - so
/// one slow response per permit is enough. Anything admitted beyond that (which the
/// tests assert does not happen) falls through to the backend's instant default.
fn queue_slow_responses(ctx: &TestContext) {
    for _ in 0..TEST_MAX_CONCURRENT {
        queue_response(
            &ctx.backend_state,
            MockResponse::json(backend_text_response("slow")).with_delay(HOLD_MS),
        );
    }
}

/// A burst larger than the limit is partly admitted and partly shed with 429.
pub async fn test_concurrent_limit_enforcement(ctx: TestContext) -> anyhow::Result<()> {
    const BURST: usize = 24;

    queue_slow_responses(&ctx);
    let statuses = burst(&ctx, BURST).await?;

    let admitted = statuses.iter().filter(|s| **s == 200).count();
    let rejected = statuses.iter().filter(|s| **s == 429).count();

    let unexpected: Vec<u16> = statuses.iter().copied().filter(|s| *s != 200 && *s != 429).collect();
    assert_true(
        unexpected.is_empty(),
        &format!("Expected only 200/429 from a saturating burst, also saw: {unexpected:?}"),
    )?;

    assert_true(
        admitted > 0,
        "Limiter rejected the entire burst - no request was ever admitted",
    )?;
    assert_true(
        admitted <= TEST_MAX_CONCURRENT,
        &format!("Limiter admitted {admitted} concurrent requests, over the configured cap of {TEST_MAX_CONCURRENT}"),
    )?;
    assert_true(
        rejected > 0,
        &format!("Sent {BURST} concurrent requests against a cap of {TEST_MAX_CONCURRENT} but nothing was shed"),
    )?;

    Ok(())
}

/// A shed request is a well-formed 429 that tells the client when to come back.
pub async fn test_429_response_format(ctx: TestContext) -> anyhow::Result<()> {
    const BURST: usize = 24;

    queue_slow_responses(&ctx);

    // Saturate, then send one more request that is near-certain to be shed.
    let handles: Vec<_> = (0..BURST)
        .map(|i| {
            let client = ctx.http_client.clone();
            let addr = ctx.proxy_addr.clone();
            tokio::spawn(async move { send_non_streaming(&client, &addr, basic_request(&format!("fill-{i}"))).await })
        })
        .collect();

    // Give the first requests time to claim every permit.
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    let resp = send_non_streaming(&ctx.http_client, &ctx.proxy_addr, basic_request("overflow")).await?;

    assert_true(
        resp.status == 429,
        &format!("Expected 429 once saturated, got {}", resp.status),
    )?;
    assert_true(
        resp.header("retry-after") == Some("5"),
        &format!("Expected Retry-After: 5, got {:?}", resp.header("retry-after")),
    )?;
    assert_true(
        resp.get_str("error.type") == Some("too_many_requests"),
        &format!("Expected error.type=too_many_requests, got body: {:?}", resp.body),
    )?;
    assert_true(
        resp.get("error.message").is_some(),
        "429 body should carry a human-readable error.message",
    )?;

    for h in handles {
        let _ = h.await;
    }

    Ok(())
}

/// Permits are returned after a burst, so later traffic is not stuck at 429.
///
/// This is the regression test for permits being released early or leaked: if the
/// limiter ever failed to hand slots back, this second wave would be shed.
pub async fn test_permits_released_after_burst(ctx: TestContext) -> anyhow::Result<()> {
    const BURST: usize = 24;

    queue_slow_responses(&ctx);
    burst(&ctx, BURST).await?;

    // Drop any slow responses the shed requests never consumed, so the wave below
    // measures permit availability rather than backend delay.
    clear_queued_responses(&ctx.backend_state);

    // Every request from the burst has now returned, so all permits must be back.
    for i in 0..(TEST_MAX_CONCURRENT * 2) {
        let resp = send_non_streaming(&ctx.http_client, &ctx.proxy_addr, basic_request("after-burst")).await?;
        assert_true(
            resp.status == 200,
            &format!("Request {i} after the burst got {} - permits were not released", resp.status),
        )?;
    }

    Ok(())
}

/// Permits survive the streaming-fallback path and are released when the stream ends.
///
/// When the backend streams despite `stream:false`, the proxy returns a lazy body and
/// the permit rides along with it. That handoff is where a permit can leak: if it were
/// moved into a stream that never drops, the limiter would wedge shut and every later
/// request would be shed.
pub async fn test_permits_released_after_streaming_fallback(ctx: TestContext) -> anyhow::Result<()> {
    let sse = "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n\
               data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
               data: [DONE]\n\n";

    // Drive the fallback path once per permit, consuming each response fully.
    for i in 0..TEST_MAX_CONCURRENT {
        queue_response(&ctx.backend_state, MockResponse::sse(sse));
        let resp = send_non_streaming(&ctx.http_client, &ctx.proxy_addr, basic_request("fallback")).await;
        // The proxy may surface this as SSE rather than JSON; either way the permit
        // must come back. Only a transport failure is interesting here.
        if let Err(e) = resp {
            let msg = e.to_string();
            assert_true(
                msg.contains("not valid JSON"),
                &format!("Streaming fallback request {i} failed unexpectedly: {msg}"),
            )?;
        }
    }

    clear_queued_responses(&ctx.backend_state);

    // If any permit leaked into an undropped stream, these are shed instead of served.
    for i in 0..(TEST_MAX_CONCURRENT * 2) {
        let resp = send_non_streaming(&ctx.http_client, &ctx.proxy_addr, basic_request("after-fallback")).await?;
        assert_true(
            resp.status == 200,
            &format!(
                "Request {i} after the streaming fallback got {} - a permit leaked into the stream",
                resp.status
            ),
        )?;
    }

    Ok(())
}

/// Monitoring routes are exempt from limiting, so a saturated proxy stays observable.
pub async fn test_monitoring_not_rejected_at_capacity(ctx: TestContext) -> anyhow::Result<()> {
    const BURST: usize = 24;

    queue_slow_responses(&ctx);

    let handles: Vec<_> = (0..BURST)
        .map(|i| {
            let client = ctx.http_client.clone();
            let addr = ctx.proxy_addr.clone();
            tokio::spawn(async move { send_non_streaming(&client, &addr, basic_request(&format!("fill-{i}"))).await })
        })
        .collect();

    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    // While saturated, every monitoring route must still answer.
    for path in ["/health", "/v1/health", "/props", "/slots", "/v1/models", "/proxy/metrics"] {
        let resp = send_get(&ctx.http_client, &ctx.proxy_addr, path).await?;
        assert_true(
            resp.status == 200,
            &format!("{path} returned {} while the proxy was at capacity", resp.status),
        )?;
    }

    for h in handles {
        let _ = h.await;
    }

    Ok(())
}

/// /proxy/metrics exposes the proxy's own counters in Prometheus text format.
pub async fn test_proxy_metrics_endpoint(ctx: TestContext) -> anyhow::Result<()> {
    let resp = send_get(&ctx.http_client, &ctx.proxy_addr, "/proxy/metrics").await?;

    assert_true(resp.status == 200, &format!("Expected 200, got {}", resp.status))?;

    // Body is plain text, so send_get leaves it as a JSON string.
    let body = resp.body.as_str().unwrap_or_default();

    for metric in [
        "llama_proxy_backend_streaming_fallback_total",
        "llama_proxy_concurrent_requests",
        "llama_proxy_rejected_requests_total",
    ] {
        assert_true(
            body.contains(&format!("# TYPE {metric} ")),
            &format!("Missing TYPE line for {metric} in:\n{body}"),
        )?;
    }

    // Only this scrape is in flight, and it discounts itself.
    assert_true(
        body.contains("llama_proxy_concurrent_requests 0"),
        &format!("Expected an idle proxy to report 0 in-flight requests, got:\n{body}"),
    )?;

    Ok(())
}
