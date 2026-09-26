//! Mock backend server that simulates llama.cpp server
//!
//! This server handles all the endpoints the proxy expects from a backend.
//! Tests pre-configure responses via SharedBackendState before each request.

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use tokio::net::TcpListener;

use crate::types::{BackendState, MockResponse, ReceivedRequest, SharedBackendState};

/// Default slot info returned by /slots
fn default_slots_response() -> &'static str {
    r#"[{"id":0,"model":"test-model","n_ctx":8192,"n_tokens":0,"is_processing":false,"params":{"n_predict":4096}}]"#
}

/// Default fallback response when no response is queued
fn default_completion_response() -> MockResponse {
    MockResponse::json(
        r#"{"id":"chatcmpl-default","object":"chat.completion","created":1700000000,"model":"test-model","choices":[{"index":0,"message":{"role":"assistant","content":"Default response (no mock queued)"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#,
    )
}

/// Handle POST /v1/chat/completions - serves pre-configured mock responses
async fn handle_chat_completions(State(state): State<SharedBackendState>, request: Request<Body>) -> Response {
    // Read and parse the request body
    let body_bytes = axum::body::to_bytes(request.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap_or_default();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap_or(serde_json::Value::Null);

    // Log the received request
    let received = ReceivedRequest {
        method: "POST".to_string(),
        path: "/v1/chat/completions".to_string(),
        body: body_json,
    };

    // Pop the next configured response (or use default)
    let mock_response = {
        let mut state = state.lock().unwrap();
        state.received_requests.push(received);
        state.response_queue.pop_front().unwrap_or_else(default_completion_response)
    };

    // Simulate generation time so tests can keep requests in flight
    if mock_response.delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(mock_response.delay_ms)).await;
    }

    Response::builder()
        .status(mock_response.status)
        .header("Content-Type", &mock_response.content_type)
        .body(Body::from(mock_response.body))
        .unwrap()
        .into_response()
}

/// Handle GET /health and /v1/health
async fn handle_health() -> impl IntoResponse {
    (StatusCode::OK, [("Content-Type", "application/json")], r#"{"status":"ok"}"#)
}

/// Handle GET /slots
async fn handle_slots() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("Content-Type", "application/json")],
        default_slots_response(),
    )
}

/// Record a request into the shared received-request list.
fn record_request(state: &SharedBackendState, method: String, path: String, body: serde_json::Value) {
    state
        .lock()
        .unwrap()
        .received_requests
        .push(ReceivedRequest { method, path, body });
}

/// Handle GET /props
async fn handle_props(State(state): State<SharedBackendState>) -> impl IntoResponse {
    record_request(&state, "GET".to_string(), "/props".to_string(), serde_json::Value::Null);
    let body = state.lock().unwrap().props_body.clone();
    (StatusCode::OK, [("Content-Type", "application/json")], body)
}

/// Handle GET /v1/models
async fn handle_models(State(state): State<SharedBackendState>) -> impl IntoResponse {
    record_request(&state, "GET".to_string(), "/v1/models".to_string(), serde_json::Value::Null);
    let body = state.lock().unwrap().models_body.clone();
    (StatusCode::OK, [("Content-Type", "application/json")], body)
}

/// Handle GET /metrics
async fn handle_metrics(State(state): State<SharedBackendState>) -> impl IntoResponse {
    record_request(&state, "GET".to_string(), "/metrics".to_string(), serde_json::Value::Null);
    let body = state.lock().unwrap().metrics_body.clone();
    (StatusCode::OK, body)
}

/// Catch-all for every method+path with no explicit route (e.g. a
/// `POST /api/models/vram-estimate`). Records the request, then answers 404 the
/// way a backend that does not implement the path would.
async fn handle_fallback(State(state): State<SharedBackendState>, request: Request<Body>) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let body_bytes = axum::body::to_bytes(request.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap_or_default();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap_or(serde_json::Value::Null);
    record_request(&state, method, path, body_json);

    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("Content-Type", "application/json")
        .body(Body::from(r#"{"detail":"Not Found"}"#))
        .unwrap()
        .into_response()
}

/// Start the mock backend server and return the shared state handle
pub async fn start(port: u16) -> anyhow::Result<SharedBackendState> {
    let state: SharedBackendState = std::sync::Arc::new(std::sync::Mutex::new(BackendState::default()));

    let app = Router::new()
        .route("/v1/chat/completions", post(handle_chat_completions))
        .route("/v1/messages", post(handle_chat_completions)) // Anthropic format too
        .route("/health", get(handle_health))
        .route("/v1/health", get(handle_health))
        .route("/slots", get(handle_slots))
        .route("/props", get(handle_props))
        .route("/v1/models", get(handle_models))
        .route("/metrics", get(handle_metrics))
        .fallback(handle_fallback)
        .with_state(state.clone());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to bind mock backend to {}: {}", addr, e))?;

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("Mock backend server failed");
    });

    // Brief pause to let the server start accepting connections
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    Ok(state)
}

/// Helper to configure the next response for /v1/chat/completions
pub fn queue_response(state: &SharedBackendState, response: MockResponse) {
    state.lock().unwrap().response_queue.push_back(response);
}

/// Helper to drop any responses still queued (e.g. ones a test queued but never consumed)
pub fn clear_queued_responses(state: &SharedBackendState) {
    state.lock().unwrap().response_queue.clear();
}

/// Helper to get all requests received since last clear
pub fn drain_requests(state: &SharedBackendState) -> Vec<ReceivedRequest> {
    let mut s = state.lock().unwrap();
    std::mem::take(&mut s.received_requests)
}

/// Override the body served by `GET /props` for the current test.
pub fn set_props_body(state: &SharedBackendState, body: impl Into<String>) {
    state.lock().unwrap().props_body = body.into();
}

/// Override the body served by `GET /v1/models` for the current test.
pub fn set_models_body(state: &SharedBackendState, body: impl Into<String>) {
    state.lock().unwrap().models_body = body.into();
}

/// Override the body served by `GET /metrics` for the current test.
pub fn set_metrics_body(state: &SharedBackendState, body: impl Into<String>) {
    state.lock().unwrap().metrics_body = body.into();
}

/// Install a vLLM-shaped `/v1/models` whose entries carry `max_model_len`.
pub fn install_vllm_models(state: &SharedBackendState) {
    set_models_body(
        state,
        r#"{"object":"list","data":[{"id":"test-model","object":"model","created":1700000000,"owned_by":"vllm","max_model_len":262144}]}"#,
    );
}

/// Install a `/props` body that carries no context length, i.e. what a non-llama.cpp
/// backend returns so context resolution must fall through to `/v1/models`.
pub fn install_props_without_context(state: &SharedBackendState) {
    set_props_body(
        state,
        r#"{"model_path":"/models/test-model.gguf","build_info":{"version":"b3000"}}"#,
    );
}

/// Install a `/metrics` body containing a `vllm:cache_config_info` gauge line.
pub fn install_vllm_metrics(state: &SharedBackendState) {
    set_metrics_body(
        state,
        "# HELP vllm:cache_config_info Information about the KV cache configuration\n\
         # TYPE vllm:cache_config_info gauge\n\
         vllm:cache_config_info{block_size=\"16\",cache_dtype=\"auto\",engine=\"0\",gpu_memory_utilization=\"0.9\",kv_cache_size_tokens=\"81280\",kv_cache_max_concurrency=\"42\",enable_prefix_caching=\"True\",num_gpu_blocks=\"5080\"} 1\n",
    );
}

/// Helper to clear the request log
pub fn clear_requests(state: &SharedBackendState) {
    state.lock().unwrap().received_requests.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Port for the standalone backend self-check, kept clear of the harness
    /// mock backend (18080) and the proxy (18066).
    const SELF_CHECK_PORT: u16 = 18090;

    /// The mock backend must record EVERY request, not just chat completions.
    ///
    /// Before this todo the router had no fallback and request recording lived
    /// only inside `handle_chat_completions`, so a `POST /api/models/vram-estimate`
    /// never reached the received-request list and was invisible to the harness.
    /// This self-check pins the recording catch-all: it is impossible to satisfy
    /// without the fallback added in this todo.
    #[tokio::test]
    async fn records_non_chat_requests() {
        let state = start(SELF_CHECK_PORT).await.expect("mock backend should bind");
        drain_requests(&state);

        let url = format!("http://127.0.0.1:{SELF_CHECK_PORT}/api/models/vram-estimate");
        let client = reqwest::Client::new();
        let _ = client
            .post(&url)
            .json(&serde_json::json!({ "model": "test-model" }))
            .send()
            .await
            .expect("POST to the mock backend should reach it at the transport level");

        let recorded = drain_requests(&state);
        let found = recorded
            .iter()
            .any(|r| r.method == "POST" && r.path == "/api/models/vram-estimate");
        assert!(
            found,
            "POST /api/models/vram-estimate was not recorded; received = {:?}",
            recorded
                .iter()
                .map(|r| format!("{} {}", r.method, r.path))
                .collect::<Vec<_>>()
        );
    }
}
