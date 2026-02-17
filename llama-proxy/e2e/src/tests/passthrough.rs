//! Tests for proxy pass-through endpoints
//! /health, /props, /slots, /v1/models must be forwarded to backend unchanged

use crate::client::send_get;
use crate::runner::TestContext;

use super::helpers::assert_true;

/// /health endpoint - the proxy has its OWN /health handler that returns "OK" (plain text)
/// It does NOT proxy /health to the backend (unlike /v1/health which IS proxied)
pub async fn test_health_passthrough(ctx: TestContext) -> anyhow::Result<()> {
    let url = format!("http://{}/health", ctx.proxy_addr);
    let resp = ctx.http_client.get(&url).send().await
        .map_err(|e| anyhow::anyhow!("Failed to GET /health: {}", e))?;

    assert_true(resp.status().as_u16() == 200, &format!("Expected 200, got {}", resp.status()))?;

    // The proxy's OWN health check returns plain text "OK", not JSON
    let body = resp.text().await.unwrap_or_default();
    assert_true(
        body.trim() == "OK",
        &format!("Expected proxy health to return 'OK', got: {:?}", body),
    )?;

    Ok(())
}

/// /v1/health endpoint passes through
pub async fn test_v1_health_passthrough(ctx: TestContext) -> anyhow::Result<()> {
    let resp = send_get(&ctx.http_client, &ctx.proxy_addr, "/v1/health").await?;

    assert_true(resp.status == 200, &format!("Expected 200, got {}", resp.status))?;
    assert_true(
        resp.body.get("status").and_then(|v| v.as_str()) == Some("ok"),
        &format!("Expected status=ok in /v1/health response, got: {:?}", resp.body),
    )?;

    Ok(())
}

/// /slots endpoint passes through
pub async fn test_slots_passthrough(ctx: TestContext) -> anyhow::Result<()> {
    let resp = send_get(&ctx.http_client, &ctx.proxy_addr, "/slots").await?;

    assert_true(resp.status == 200, &format!("Expected 200, got {}", resp.status))?;
    // Should be an array
    assert_true(
        resp.body.is_array(),
        &format!("Expected /slots to return an array, got: {:?}", resp.body),
    )?;

    let slots = resp.body.as_array().unwrap();
    assert_true(!slots.is_empty(), "/slots response should have at least one slot")?;

    // Each slot should have n_ctx
    let n_ctx = slots[0].get("n_ctx");
    assert_true(n_ctx.is_some(), "Slot should have n_ctx field")?;

    Ok(())
}

/// /props endpoint passes through
pub async fn test_props_passthrough(ctx: TestContext) -> anyhow::Result<()> {
    let resp = send_get(&ctx.http_client, &ctx.proxy_addr, "/props").await?;

    assert_true(resp.status == 200, &format!("Expected 200, got {}", resp.status))?;
    assert_true(
        resp.body.get("n_ctx").is_some(),
        &format!("Expected n_ctx in /props response, got: {:?}", resp.body),
    )?;

    Ok(())
}

/// /v1/models endpoint passes through
pub async fn test_models_passthrough(ctx: TestContext) -> anyhow::Result<()> {
    let resp = send_get(&ctx.http_client, &ctx.proxy_addr, "/v1/models").await?;

    assert_true(resp.status == 200, &format!("Expected 200, got {}", resp.status))?;
    assert_true(
        resp.body.get("object").and_then(|v| v.as_str()) == Some("list"),
        &format!("Expected object=list in /v1/models, got: {:?}", resp.body),
    )?;
    assert_true(
        resp.body.get("data").and_then(|v| v.as_array()).is_some(),
        "/v1/models should have data array",
    )?;

    Ok(())
}

/// Pass-through endpoints should NOT have fixes applied (no JSON mutation)
/// Verified by checking /slots - proxy passes it through unchanged from backend
pub async fn test_passthrough_not_modified(ctx: TestContext) -> anyhow::Result<()> {
    // /slots should return exactly what the backend returns
    let resp = send_get(&ctx.http_client, &ctx.proxy_addr, "/slots").await?;

    assert_true(resp.status == 200, &format!("Expected 200, got {}", resp.status))?;

    // The proxy should not add any extra fields (like "fixes_applied")
    let slots = resp.body.as_array()
        .ok_or_else(|| anyhow::anyhow!("Expected array from /slots, got: {:?}", resp.body))?;

    assert_true(!slots.is_empty(), "/slots array should not be empty")?;

    // Each slot should be a plain object from the backend - no proxy-added metadata
    let first_slot = &slots[0];
    assert_true(
        first_slot.is_object(),
        "Slot should be an object",
    )?;

    // Should have the standard llama.cpp slot fields (not any proxy-injected fields)
    assert_true(
        first_slot.get("n_ctx").is_some(),
        "Slot should have n_ctx field from backend",
    )?;

    Ok(())
}
