//! e2e coverage for the AnythingLLM `POST /api/models/vram-estimate` shim.
//!
//! This is the ONE request the proxy answers ABOUT the backend instead of forwarding
//! it. No backend implements the path, so a forwarded request comes back as a backend
//! 404 that the client reads as "this local model has no context window". These tests
//! drive the REAL proxy binary and pin both honest outcomes:
//!
//! * happy — 200 whose `context_length` is the window the backend itself advertised
//!   (`max_model_len` from `/v1/models`, because `/props` is rigged to offer nothing),
//!   plus the real `vllm` KV-cache labels;
//! * failure — the proxy's OWN 404 when no window is advertised, never a fabricated number.
//!
//! Both assert the request never reaches the backend, and both PRINT the body and the
//! mock's recorded-request list so a passing run is observable, not silent.
//!
//! ## Why the failure test runs its own proxy process
//! `CONTEXT_CACHE` (`src/proxy/context.rs`) and `KV_INFO` (`src/proxy/compat.rs`) are
//! process-global inside the proxy and keyed by backend base_url, and the harness spawns
//! ONE proxy for the WHOLE run (`main.rs::do_spawn_and_run`), every test sharing
//! `http://127.0.0.1:18080`. The happy test therefore caches a resolved window under
//! that key for the life of the process, so a failure test on the same key would be
//! served from cache and would print 200 while asserting 404 — a green that proves
//! nothing. The failure test therefore spawns its OWN proxy against its OWN mock on a
//! different port: a different cache key, provably cold, and no ordering dependency
//! between the two tests.

use serde_json::Value;

use crate::backend::{
    clear_requests, drain_requests, install_props_without_context, install_vllm_metrics, install_vllm_models, queue_response,
    set_models_body,
};
use crate::client::{send_non_streaming, send_post};
use crate::isolated;
use crate::runner::TestContext;
use crate::types::MockResponse;

use super::helpers::{assert_true, backend_text_response, basic_request, print_recorded_requests, summarize_requests};

/// The endpoint under test, exactly as AnythingLLM's LocalAI provider calls it.
const SHIM_PATH: &str = "/api/models/vram-estimate";

/// The model name the client asks about; the shim only echoes it back.
const MODEL: &str = "test-model";

/// A vLLM-shaped `/v1/models` that OMITS `max_model_len` — a vLLM node that advertised
/// no window. Everything else about it is identical to `install_vllm_models`, so the
/// 404 can only be about the missing window, not about the backend being a different shape.
const MODELS_WITHOUT_WINDOW: &str =
    r#"{"object":"list","data":[{"id":"test-model","object":"model","created":1700000000,"owned_by":"vllm"}]}"#;

/// Ports for the failure scenario's dedicated proxy + mock. Both sit in the harness'
/// 180xx band (so `ss -ltnp | grep -E ':180[0-9][0-9]'` sees them) and clear of the
/// shared proxy (18066), shared mock (18080) and the backend self-check (18090).
const ISOLATED_PROXY_PORT: u16 = 18071;
const ISOLATED_BACKEND_PORT: u16 = 18081;

/// Happy path: the shim answers locally from the backend's advertised window and is
/// never forwarded, and the ordinary completion path still works afterwards.
pub async fn test_answered_locally_and_never_forwarded(ctx: TestContext) -> anyhow::Result<()> {
    // Given: a vLLM-shaped backend. `/props` yields NO usable `n_ctx`, so the answer
    // MUST come from `/v1/models`; `/metrics` carries the `vllm:cache_config_info` gauge.
    install_props_without_context(&ctx.backend_state);
    install_vllm_models(&ctx.backend_state);
    install_vllm_metrics(&ctx.backend_state);
    clear_requests(&ctx.backend_state);

    // The number the mock advertises, read back out of the mock's own fixture — the
    // expectation comes from the input, never from the response under test.
    let advertised: Value = {
        let body = ctx.backend_state.lock().unwrap().models_body.clone();
        serde_json::from_str(&body).map_err(|e| anyhow::anyhow!("mock /v1/models fixture is not JSON: {e}"))?
    };
    let advertised = advertised
        .pointer("/data/0/max_model_len")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("mock /v1/models fixture must carry data[0].max_model_len"))?;

    // When: AnythingLLM asks the proxy for a VRAM estimate.
    let resp = send_post(
        &ctx.http_client,
        &ctx.proxy_addr,
        SHIM_PATH,
        &serde_json::json!({ "model": MODEL }),
    )
    .await?;
    println!("    shim status : {}", resp.status);
    println!(
        "    body        : {}",
        serde_json::to_string_pretty(&resp.body).unwrap_or_default()
    );

    // Then: 200 + JSON, answering a context-window question in tokens.
    assert_true(
        resp.status == 200,
        &format!("expected 200 from the shim, got {}", resp.status),
    )?;
    assert_true(
        resp.header("content-type").unwrap_or("").contains("application/json"),
        &format!("expected application/json, got {:?}", resp.header("content-type")),
    )?;
    let context_length = resp
        .get("context_length")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("body carries no numeric `context_length`: {}", resp.body))?;
    assert_true(
        context_length == advertised,
        &format!("`context_length` must be the mock's advertised max_model_len: expected {advertised}, got {context_length}"),
    )?;
    assert_true(
        context_length != 8192,
        "context_length is the mock's DEFAULT /props n_ctx — the answer came from a stale or \
         default source, not from this test's `/v1/models` fixture",
    )?;
    assert_true(
        resp.get("model_max_context").and_then(|v| v.as_u64()) == Some(context_length),
        &format!("`model_max_context` must mirror `context_length`: {}", resp.body),
    )?;
    assert_true(
        resp.get_str("context_note").is_some_and(|s| !s.is_empty()),
        &format!("`context_note` must be a non-empty string: {}", resp.body),
    )?;
    assert_true(
        resp.get_str("model") == Some(MODEL),
        &format!("the requested model must be echoed back: {}", resp.body),
    )?;

    // Then: the nested `vllm` object is real data scraped from the gauge, values as strings.
    let num_gpu_blocks = resp.get("vllm.num_gpu_blocks").ok_or_else(|| {
        anyhow::anyhow!(
            "nested `vllm.num_gpu_blocks` missing — the gauge was not scraped: {}",
            resp.body
        )
    })?;
    assert_true(
        num_gpu_blocks.is_string(),
        &format!("vLLM prints every CacheConfig attribute through str(), so labels stay strings: {num_gpu_blocks}"),
    )?;

    // Then: NO VRAM/byte key anywhere in the body — recursively, because `vllm` is nested.
    let mut offenders = Vec::new();
    vram_like_keys(&resp.body, "$", &mut offenders);
    assert_true(
        offenders.is_empty(),
        &format!("body must carry no VRAM/byte/`id` key, found {offenders:?} in {}", resp.body),
    )?;

    // Then: the backend never saw the shim request. Printed first — non-forwarding is
    // only believable if the reader can see what the backend actually recorded.
    let recorded = drain_requests(&ctx.backend_state);
    print_recorded_requests(&recorded);
    assert_true(
        !recorded.iter().any(|r| r.method == "POST" && r.path == SHIM_PATH),
        &format!(
            "the proxy must never forward {SHIM_PATH}; backend recorded {}",
            summarize_requests(&recorded)
        ),
    )?;

    // Then: the completion path is not collateral damage. A follow-on ordinary chat
    // still round-trips through the same proxy, on the same backend.
    queue_response(
        &ctx.backend_state,
        MockResponse::json(backend_text_response("still completing")),
    );
    let chat = send_non_streaming(&ctx.http_client, &ctx.proxy_addr, basic_request("after the shim")).await?;
    assert_true(
        chat.status == 200,
        &format!("follow-on completion must still succeed, got {}", chat.status),
    )?;
    assert_true(
        chat.get_str("choices.0.message.content") == Some("still completing"),
        &format!("follow-on completion content changed: {}", chat.body),
    )?;

    let after_chat = drain_requests(&ctx.backend_state);
    print_recorded_requests(&after_chat);
    assert_true(
        after_chat
            .iter()
            .any(|r| r.method == "POST" && r.path == "/v1/chat/completions"),
        &format!(
            "follow-on chat must reach the backend; it recorded {}",
            summarize_requests(&after_chat)
        ),
    )?;

    Ok(())
}

/// Failure path: a backend that advertises no context window gets the proxy's OWN 404,
/// not a number and not a forwarded backend 404.
///
/// Runs against a dedicated proxy on [`ISOLATED_PROXY_PORT`] backed by a dedicated mock
/// on [`ISOLATED_BACKEND_PORT`]. See the module docs: sharing the run's proxy would mean
/// sharing `CONTEXT_CACHE`/`KV_INFO` under `http://127.0.0.1:18080`, i.e. inheriting the
/// window the happy test resolved, and this assertion would pass on cache leakage.
pub async fn test_own_404_when_context_unknown(_ctx: TestContext) -> anyhow::Result<()> {
    // Given: a mock that looks like vLLM but advertises NO window, and whose /metrics DOES
    // carry the gauge — so the 404 can only be about the missing window, never about the
    // KV scrape. It runs on a dedicated proxy, hence a provably cold cache key.
    let isolated = isolated::spawn_configured(ISOLATED_PROXY_PORT, ISOLATED_BACKEND_PORT, |mock| {
        install_props_without_context(mock);
        install_vllm_metrics(mock);
        set_models_body(mock, MODELS_WITHOUT_WINDOW);
        clear_requests(mock);
    })
    .await?;

    let client = crate::client::build_client();
    let resp = send_post(
        &client,
        &isolated.proxy_addr,
        SHIM_PATH,
        &serde_json::json!({ "model": MODEL }),
    )
    .await?;
    println!("    shim status : {}", resp.status);
    println!(
        "    body        : {}",
        serde_json::to_string_pretty(&resp.body).unwrap_or_default()
    );

    assert_true(
        resp.status == 404,
        &format!(
            "a backend with no advertised window must get the proxy's own 404, got {}: {}",
            resp.status, resp.body
        ),
    )?;
    assert_true(
        resp.get_str("error.type") == Some("vram_estimate_context_unknown"),
        &format!(
            "the 404 must be the proxy's own envelope, not a forwarded backend 404: {}",
            resp.body
        ),
    )?;
    assert_true(
        resp.get_str("error.message")
            .is_some_and(|m| m.contains(&isolated.backend_url)),
        &format!("the envelope must name the backend that advertised nothing: {}", resp.body),
    )?;

    let recorded = drain_requests(&isolated.mock);
    print_recorded_requests(&recorded);
    assert_true(
        !recorded.iter().any(|r| r.method == "POST" && r.path == SHIM_PATH),
        &format!(
            "the proxy must never forward {SHIM_PATH} even when it cannot answer; backend recorded {}",
            summarize_requests(&recorded)
        ),
    )?;
    Ok(())
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Every key anywhere in `value` that would present a VRAM/byte figure, or the `id`
/// the shim must never emit (the client does `{ id, ...est }` and would overwrite the
/// id it already bound from `/v1/models`).
///
/// `memory` and `size` are deliberately NOT matched as bare substrings:
/// `gpu_memory_utilization` is a 0..1 ratio and `kv_cache_size_tokens` is a token
/// count, and the contract keeps both as real vLLM labels.
fn vram_like_keys(value: &Value, path: &str, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let here = format!("{path}.{key}");
                let lowered = key.to_ascii_lowercase();
                if lowered.contains("byte") || lowered.contains("vram") || lowered.ends_with("_display") || lowered == "id" {
                    found.push(here.clone());
                }
                vram_like_keys(child, &here, found);
            }
        }
        Value::Array(items) => {
            for (idx, child) in items.iter().enumerate() {
                vram_like_keys(child, &format!("{path}[{idx}]"), found);
            }
        }
        _ => {}
    }
}
