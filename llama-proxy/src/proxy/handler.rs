//! Request/response handler for the proxy

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    response::{IntoResponse, Response},
};
use std::time::Instant;

use super::server::ProxyState;
use super::streaming::handle_streaming_response;
use crate::proxy::fetch_context_total;
use crate::stats::{format_metrics, RequestMetrics};

/// Proxy request handler
pub struct ProxyHandler {
    state: ProxyState,
}

impl ProxyHandler {
    pub fn new(state: ProxyState) -> Self {
        Self { state }
    }

    /// Handle an incoming request
    pub async fn handle(&self, req: Request<Body>) -> Response {
        let start = Instant::now();
        let method = req.method().clone();
        let uri = req.uri().clone();
        let path = uri.path();

        tracing::debug!(method = %method, path = %path, "Processing request");

        // Route specific endpoints to simple pass-through
        match (&method, path) {
            // llama.cpp monitoring/status endpoints (simple pass-through)
            (&Method::GET, "/props")
            | (&Method::GET, "/slots")
            | (&Method::GET, "/health")
            | (&Method::GET, "/v1/health")
            | (&Method::GET, "/v1/models")
            | (&Method::GET, "/metrics") => {
                return self.proxy_passthrough(req).await;
            }

            // All other routes continue with existing logic
            _ => {}
        }

        // Save headers before consuming the request
        let headers = req.headers().clone();

        // Read request body
        let body_bytes = match to_bytes(req.into_body(), 1024 * 1024 * 100).await {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!(error = %e, "Failed to read request body");
                return (
                    StatusCode::BAD_REQUEST,
                    format!("Failed to read request body: {}", e),
                )
                    .into_response();
            }
        };

        // Parse request for stats (if JSON)
        let request_json: Option<serde_json::Value> = serde_json::from_slice(&body_bytes).ok();

        let is_streaming = request_json
            .as_ref()
            .and_then(|j| j.get("stream"))
            .and_then(|s| s.as_bool())
            .unwrap_or(false);

        // Build backend URL
        let backend_url = format!("{}{}", self.state.config.backend.url(), path);

        // Forward request to backend
        let mut backend_req = self.state.http_client.request(
            Method::from_bytes(method.as_str().as_bytes()).unwrap(),
            &backend_url,
        );

        // Copy headers
        for (name, value) in headers.iter() {
            if name != header::HOST {
                backend_req = backend_req.header(name, value);
            }
        }

        backend_req = backend_req.body(body_bytes.clone());

        let backend_response = match backend_req.send().await {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!(error = %e, "Failed to connect to backend");
                return (
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to connect to backend: {}", e),
                )
                    .into_response();
            }
        };

        // Check if streaming response
        let is_streaming_response = backend_response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|ct| ct.to_str().ok())
            .map(|ct| ct.contains("text/event-stream"))
            .unwrap_or(false);

        if is_streaming_response {
            // Handle streaming response
            handle_streaming_response(
                backend_response,
                self.state.fix_registry.clone(),
                self.state.config.stats.enabled,
                self.state.config.stats.format,
                self.state.exporter_manager.clone(),
                request_json,
                start,
                self.state.http_client.clone(),
                self.state.config.backend.url().to_string(),
            )
            .await
        } else {
            // Handle non-streaming response
            self.handle_non_streaming_response(
                backend_response,
                request_json,
                is_streaming,
                start,
            )
            .await
        }
    }

    /// Handle a non-streaming response
    async fn handle_non_streaming_response(
        &self,
        backend_response: reqwest::Response,
        request_json: Option<serde_json::Value>,
        is_streaming_request: bool,
        start: Instant,
    ) -> Response {
        let status = backend_response.status();
        let headers = backend_response.headers().clone();

        // Read response body
        let body_bytes = match backend_response.bytes().await {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!(error = %e, "Failed to read backend response");
                return (
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to read backend response: {}", e),
                )
                    .into_response();
            }
        };

        // Try to parse as JSON and apply fixes
        let (body_bytes, metrics) =
            if let Ok(mut json) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
                // Apply fixes
                json = self.state.fix_registry.apply_fixes(json);

                // Collect stats if enabled
                let mut metrics = if self.state.config.stats.enabled {
                    if let Some(ref req_json) = request_json {
                        Some(RequestMetrics::from_response(
                            &json,
                            req_json,
                            is_streaming_request,
                            start.elapsed().as_millis() as f64,
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Fetch and set context_total for stats
                if let Some(ref mut m) = metrics {
                    if let Some(ctx_total) = fetch_context_total(
                        &self.state.http_client,
                        &self.state.config.backend.url(),
                    )
                    .await
                    {
                        m.context_total = Some(ctx_total);
                        m.calculate_context_percent();
                    }
                }

                // Serialize back to bytes
                match serde_json::to_vec(&json) {
                    Ok(bytes) => (bytes.into(), metrics),
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to serialize fixed response");
                        (body_bytes, None)
                    }
                }
            } else {
                (body_bytes, None)
            };

        // Log stats
        if let Some(ref m) = metrics {
            let formatted = format_metrics(m, self.state.config.stats.format);
            tracing::info!("\n{}", formatted);

            // Export to remote systems
            let exporters = self.state.exporter_manager.clone();
            let metrics_clone = m.clone();
            tokio::spawn(async move {
                exporters.export_all(&metrics_clone).await;
            });
        }

        // Build response
        let mut response = Response::builder().status(status);

        for (name, value) in headers {
            if let Some(name) = name {
                response = response.header(name, value);
            }
        }

        response.body(Body::from(body_bytes)).unwrap().into_response()
    }

    /// Simple pass-through with no fix application or stats collection
    /// Used for monitoring endpoints like /props, /slots, /health
    async fn proxy_passthrough(&self, req: Request<Body>) -> Response {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let headers = req.headers().clone();
        let path = uri.path();

        tracing::debug!(method = %method, path = %path, "Pass-through request");

        // Read body
        let body_bytes = match to_bytes(req.into_body(), 1024 * 1024 * 10).await {
            Ok(bytes) => bytes,
            Err(e) => {
                return (StatusCode::BAD_REQUEST, format!("Failed to read body: {}", e))
                    .into_response();
            }
        };

        // Build backend URL
        let backend_url = format!("{}{}", self.state.config.backend.url(), path);

        // Forward to backend
        let mut backend_req = self.state.http_client.request(
            Method::from_bytes(method.as_str().as_bytes()).unwrap(),
            &backend_url,
        );

        // Copy headers
        for (name, value) in headers.iter() {
            if name != header::HOST {
                backend_req = backend_req.header(name, value);
            }
        }
        backend_req = backend_req.body(body_bytes);

        let backend_response = match backend_req.send().await {
            Ok(resp) => resp,
            Err(e) => {
                return (StatusCode::BAD_GATEWAY, format!("Backend error: {}", e))
                    .into_response();
            }
        };

        // Pass through response
        let status = backend_response.status();
        let headers = backend_response.headers().clone();
        let body = match backend_response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return (StatusCode::BAD_GATEWAY, format!("Failed to read response: {}", e))
                    .into_response();
            }
        };

        let mut response = Response::builder().status(status);
        for (name, value) in headers {
            if let Some(name) = name {
                response = response.header(name, value);
            }
        }
        response.body(Body::from(body)).unwrap()
    }
}
