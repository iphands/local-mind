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

/// Default props returned by /props
fn default_props_response() -> &'static str {
    r#"{"model_path":"/models/test-model.gguf","n_ctx":8192,"n_batch":512,"gpu_layers":0,"chat_template":"llama3","build_info":{"version":"b3000"}}"#
}

/// Default models list returned by /v1/models
fn default_models_response() -> &'static str {
    r#"{"object":"list","data":[{"id":"test-model","object":"model","created":1700000000,"owned_by":"llamacpp"}]}"#
}

/// Default fallback response when no response is queued
fn default_completion_response() -> MockResponse {
    MockResponse::json(
        r#"{"id":"chatcmpl-default","object":"chat.completion","created":1700000000,"model":"test-model","choices":[{"index":0,"message":{"role":"assistant","content":"Default response (no mock queued)"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#,
    )
}

/// Handle POST /v1/chat/completions - serves pre-configured mock responses
async fn handle_chat_completions(
    State(state): State<SharedBackendState>,
    request: Request<Body>,
) -> Response {
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

    Response::builder()
        .status(mock_response.status)
        .header("Content-Type", &mock_response.content_type)
        .body(Body::from(mock_response.body))
        .unwrap()
        .into_response()
}

/// Handle GET /health and /v1/health
async fn handle_health() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("Content-Type", "application/json")],
        r#"{"status":"ok"}"#,
    )
}

/// Handle GET /slots
async fn handle_slots() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("Content-Type", "application/json")],
        default_slots_response(),
    )
}

/// Handle GET /props
async fn handle_props() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("Content-Type", "application/json")],
        default_props_response(),
    )
}

/// Handle GET /v1/models
async fn handle_models() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("Content-Type", "application/json")],
        default_models_response(),
    )
}

/// Handle GET /metrics (empty for testing)
async fn handle_metrics() -> impl IntoResponse {
    (StatusCode::OK, "# No metrics in test mode\n")
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
        .with_state(state.clone());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await
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

/// Helper to get all requests received since last clear
pub fn drain_requests(state: &SharedBackendState) -> Vec<ReceivedRequest> {
    let mut s = state.lock().unwrap();
    s.received_requests.drain(..).collect()
}

/// Helper to clear the request log
#[allow(dead_code)]
pub fn clear_requests(state: &SharedBackendState) {
    state.lock().unwrap().received_requests.clear();
}
