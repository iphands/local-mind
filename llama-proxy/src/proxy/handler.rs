//! Request/response handler for the proxy

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    response::{IntoResponse, Response},
};
use std::io::Read;
use std::time::Instant;

use super::server::ProxyState;
use super::streaming::handle_streaming_response;
use super::{synthesize_anthropic_streaming_response, synthesize_streaming_response};
use crate::api::{AnthropicMessage, ChatCompletionResponse};
use crate::config::StatsFormat;
use crate::proxy::fetch_context_total;
use crate::stats::{format_metrics, format_request_log, RequestMetrics};

/// Create a preview of JSON with nested objects/arrays replaced by "[object]"
fn json_preview(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut preview_map = serde_json::Map::new();
            for (key, val) in map {
                match val {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        preview_map.insert(key.clone(), serde_json::Value::String("[object]".to_string()));
                    }
                    _ => {
                        preview_map.insert(key.clone(), val.clone());
                    }
                }
            }
            serde_json::to_string_pretty(&serde_json::Value::Object(preview_map))
                .unwrap_or_else(|_| "[failed to serialize]".to_string())
        }
        _ => serde_json::to_string_pretty(value)
            .unwrap_or_else(|_| "[failed to serialize]".to_string())
    }
}

/// Decompress response body based on Content-Encoding header
fn decompress_body(body_bytes: &[u8], content_encoding: Option<&str>) -> Result<Vec<u8>, String> {
    let encoding = match content_encoding {
        Some(enc) => enc,
        None => return Ok(body_bytes.to_vec()), // No compression
    };

    match encoding.to_lowercase().as_str() {
        "gzip" => {
            use flate2::read::GzDecoder;
            let mut decoder = GzDecoder::new(body_bytes);
            let mut decompressed = Vec::new();
            decoder
                .read_to_end(&mut decompressed)
                .map_err(|e| format!("gzip decompression failed: {}", e))?;
            tracing::debug!(
                original_size = body_bytes.len(),
                decompressed_size = decompressed.len(),
                "Decompressed gzip response"
            );
            Ok(decompressed)
        }
        "deflate" => {
            use flate2::read::DeflateDecoder;
            let mut decoder = DeflateDecoder::new(body_bytes);
            let mut decompressed = Vec::new();
            decoder
                .read_to_end(&mut decompressed)
                .map_err(|e| format!("deflate decompression failed: {}", e))?;
            tracing::debug!(
                original_size = body_bytes.len(),
                decompressed_size = decompressed.len(),
                "Decompressed deflate response"
            );
            Ok(decompressed)
        }
        "br" => {
            let mut decompressed = Vec::new();
            brotli::BrotliDecompress(&mut std::io::Cursor::new(body_bytes), &mut decompressed)
                .map_err(|e| format!("brotli decompression failed: {}", e))?;
            tracing::debug!(
                original_size = body_bytes.len(),
                decompressed_size = decompressed.len(),
                "Decompressed brotli response"
            );
            Ok(decompressed)
        }
        "zstd" => {
            let decompressed = zstd::decode_all(body_bytes)
                .map_err(|e| format!("zstd decompression failed: {}", e))?;
            tracing::debug!(
                original_size = body_bytes.len(),
                decompressed_size = decompressed.len(),
                "Decompressed zstd response"
            );
            Ok(decompressed)
        }
        other => {
            tracing::warn!(
                encoding = other,
                "Unsupported Content-Encoding, returning original body"
            );
            Ok(body_bytes.to_vec())
        }
    }
}

/// Proxy request handler
pub struct ProxyHandler {
    state: ProxyState,
}

impl ProxyHandler {
    pub fn new(state: ProxyState) -> Self {
        Self { state }
    }

    // REMOVED: should_stream() method
    // We now ALWAYS force non-streaming backend requests and synthesize streaming responses
    // when clients request them. This simplifies fix application significantly.

    /// Check if Content-Type indicates JSON response
    fn is_json_content_type(content_type: &str) -> bool {
        let ct_lower = content_type.to_lowercase();
        ct_lower.contains("application/json") || ct_lower.contains("application/vnd.") && ct_lower.contains("+json")
    }

    /// Handle an incoming request
    pub async fn handle(&self, req: Request<Body>) -> Response {
        let start = Instant::now();
        let method = req.method().clone();
        let uri = req.uri().clone();
        let path = uri.path();
        let query = uri.query();

        tracing::debug!(method = %method, path = %path, query = ?query, "Processing request");

        let is_anthropic_api = path.starts_with("/v1/messages");
        tracing::debug!(is_anthropic_api = is_anthropic_api, "Detected API format");

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

        // User-Agent no longer needed since we always force non-streaming

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

        // Remember if client wants streaming (for synthesis later)
        let client_wants_streaming = request_json
            .as_ref()
            .and_then(|j| j.get("stream"))
            .and_then(|s| s.as_bool())
            .unwrap_or(false);

        // Log request
        if let Some(ref req_json) = request_json {
            tracing::info!("{}", format_request_log(req_json));
        }

        // Build complete URL with query string as-is (don't parse/re-encode)
        let backend_url = if let Some(q) = query {
            format!("{}{}?{}", self.state.config.backend.base_url(), path, q)
        } else {
            format!("{}{}", self.state.config.backend.base_url(), path)
        };

        tracing::debug!(
            backend_url = %backend_url,
            has_query = query.is_some(),
            "Building backend request"
        );

        // Create request with complete URL (query string included)
        let mut backend_req = self.state.http_client.request(
            Method::from_bytes(method.as_str().as_bytes()).unwrap(),
            &backend_url,
        );

        // Copy headers (skip Content-Length, Host, and Authorization as we'll set those explicitly)
        for (name, value) in headers.iter() {
            // Skip headers that will be set explicitly or handled by reqwest
            if name == header::HOST
                || name == header::CONTENT_LENGTH
                || name == header::AUTHORIZATION {
                continue;
            }

            backend_req = backend_req.header(name, value);
        }

        // Add Authorization header if api_key is configured
        if let Some(ref api_key) = self.state.config.backend.api_key {
            backend_req = backend_req.header(header::AUTHORIZATION, format!("Bearer {}", api_key));
        }

        // ALWAYS force stream: false for backend request
        let body_bytes = if let Some(mut json) = request_json.clone() {
            json["stream"] = serde_json::Value::Bool(false);
            // Override model if configured
            if let Some(ref model) = self.state.config.backend.model {
                json["model"] = serde_json::Value::String(model.clone());
            }
            if client_wants_streaming {
                tracing::debug!(
                    "Forcing non-streaming backend request (will synthesize streaming response)"
                );
            }
            serde_json::to_vec(&json).unwrap_or_else(|_| body_bytes.to_vec()).into()
        } else {
            body_bytes
        };

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

        // Log backend response status and headers for debugging
        let backend_status = backend_response.status();
        tracing::debug!(
            status = %backend_status,
            headers = ?backend_response.headers(),
            "Received response from backend"
        );

        // Check if streaming response
        let is_streaming_response = backend_response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|ct| ct.to_str().ok())
            .map(|ct| ct.contains("text/event-stream"))
            .unwrap_or(false);

        if is_streaming_response {
            // Unexpected! We forced stream:false but got streaming response
            tracing::warn!("Backend returned streaming response despite stream:false request");
            // Fall back to old streaming handler
            handle_streaming_response(
                backend_response,
                self.state.fix_registry.clone(),
                self.state.config.stats.enabled,
                self.state.config.stats.format,
                self.state.exporter_manager.clone(),
                request_json,
                start,
                self.state.http_client.clone(),
                self.state.config.backend.base_url().to_string(),
            )
            .await
        } else {
            // Handle non-streaming response (expected path)
            self.handle_non_streaming_response(
                backend_response,
                request_json,
                client_wants_streaming,
                is_anthropic_api,
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
        client_wants_streaming: bool,
        is_anthropic_api: bool,
        start: Instant,
    ) -> Response {
        let status = backend_response.status();
        let headers = backend_response.headers().clone();

        // Read response body
        let raw_body_bytes = match backend_response.bytes().await {
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

        // Check for Content-Encoding and decompress if needed
        let content_encoding = headers
            .get(header::CONTENT_ENCODING)
            .and_then(|ce| ce.to_str().ok());

        let body_bytes = match decompress_body(&raw_body_bytes, content_encoding) {
            Ok(decompressed) => decompressed,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    content_encoding = ?content_encoding,
                    "Failed to decompress response body"
                );
                return (
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to decompress response: {}", e),
                )
                    .into_response();
            }
        };

        // If error status, log the full response body for debugging
        if status.is_client_error() || status.is_server_error() {
            let error_body = String::from_utf8_lossy(&body_bytes);
            tracing::error!(
                status = %status,
                error_body = %error_body,
                "Backend returned error response"
            );
        }

        // Debug: Log received response details
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|ct| ct.to_str().ok())
            .unwrap_or("unknown");
        let body_preview = String::from_utf8_lossy(&body_bytes[..body_bytes.len().min(500)]);
        tracing::debug!(
            backend_status = %status,
            body_size = body_bytes.len(),
            content_type = %content_type,
            content_encoding = ?content_encoding,
            body_preview = %body_preview,
            "Received non-streaming response from backend"
        );

        // Only try to parse as JSON if Content-Type indicates JSON
        let is_json_response = Self::is_json_content_type(content_type);

        // Try to parse as JSON and apply fixes (only if Content-Type is JSON)
        let (json_value, metrics) = if is_json_response {
            if let Ok(mut json) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
                tracing::debug!("Response parsed as JSON successfully");
                // Apply fixes with request context if available
                let original_json = json.clone();
                json = if let Some(ref req_json) = request_json {
                    self.state.fix_registry.apply_fixes_with_context(json, req_json)
                } else {
                    self.state.fix_registry.apply_fixes(json)
                };
                if json != original_json {
                    tracing::debug!("Fixes applied to non-streaming response");
                } else {
                    tracing::debug!("No fixes applied to response");
                }

                // Collect stats if enabled
                let mut metrics = if self.state.config.stats.enabled {
                    if let Some(ref req_json) = request_json {
                        Some(RequestMetrics::from_response(
                            &json,
                            req_json,
                            false, // We forced non-streaming
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
                        &self.state.config.backend.base_url(),
                    )
                    .await
                    {
                        m.context_total = Some(ctx_total);
                        m.calculate_context_percent();
                    }
                }

                (Some(json), metrics)
            } else {
                // Content-Type says JSON but parsing failed - log warning
                tracing::warn!(
                    body_size = body_bytes.len(),
                    body_preview = %String::from_utf8_lossy(&body_bytes[..body_bytes.len().min(200)]),
                    "Content-Type indicates JSON but parsing failed - returning original body unchanged"
                );
                (None, None)
            }
        } else {
            // Content-Type is not JSON - this is expected, just passthrough
            tracing::debug!(
                content_type = %content_type,
                "Non-JSON content type, passing through unchanged"
            );
            (None, None)
        };

        // Log stats
        if let Some(ref m) = metrics {
            let formatted = format_metrics(m, self.state.config.stats.format);
            if self.state.config.stats.format == StatsFormat::Compact {
                tracing::info!("{}", formatted);
            } else {
                tracing::info!("\n{}", formatted);
            }

            // Export to remote systems
            let exporters = self.state.exporter_manager.clone();
            let metrics_clone = m.clone();
            tokio::spawn(async move {
                exporters.export_all(&metrics_clone).await;
            });
        } else {
            // Debug: Log why stats weren't collected
            tracing::debug!(
                stats_enabled = self.state.config.stats.enabled,
                has_request_json = request_json.is_some(),
                "No metrics collected for non-streaming response"
            );
        }

        // If client wants streaming, synthesize it from complete JSON
        if client_wants_streaming {
            if let Some(ref json) = json_value {
                if is_anthropic_api {
                    // Anthropic API: try parsing as Anthropic format first
                    match serde_json::from_value::<AnthropicMessage>(json.clone()) {
                        Ok(anthropic_msg) => {
                            tracing::debug!("Backend returned Anthropic format, synthesizing streaming response");
                            match synthesize_anthropic_streaming_response(anthropic_msg).await {
                                Ok(response) => return response,
                                Err(e) => {
                                    tracing::error!(error = %e, "Failed to synthesize Anthropic streaming response");
                                    // Fall through to return JSON
                                }
                            }
                        }
                        Err(_) => {
                            // Backend returned OpenAI format - convert to Anthropic and synthesize
                            tracing::debug!("Backend returned OpenAI format, converting to Anthropic for streaming synthesis");
                            match serde_json::from_value::<ChatCompletionResponse>(json.clone()) {
                                Ok(openai_response) => {
                                    // Convert OpenAI → Anthropic format
                                    let anthropic_msg = AnthropicMessage::from(openai_response);
                                    tracing::debug!(
                                        converted_tokens = anthropic_msg.usage.input_tokens + anthropic_msg.usage.output_tokens,
                                        content_blocks = anthropic_msg.content.len(),
                                        "Converted OpenAI response to Anthropic format"
                                    );
                                    match synthesize_anthropic_streaming_response(anthropic_msg).await {
                                        Ok(response) => return response,
                                        Err(e) => {
                                            tracing::error!(error = %e, "Failed to synthesize after OpenAI→Anthropic conversion");
                                            // Fall through to return JSON
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(
                                        error = %e,
                                        response_json = %json_preview(json),
                                        "Failed to parse backend response as either Anthropic or OpenAI format"
                                    );
                                    // Fall through to return JSON
                                }
                            }
                        }
                    }
                } else {
                    // OpenAI API: synthesize in OpenAI SSE format
                    match serde_json::from_value::<ChatCompletionResponse>(json.clone()) {
                        Ok(response) => {
                            tracing::debug!("Synthesizing OpenAI streaming response from complete JSON");
                            match synthesize_streaming_response(response).await {
                                Ok(response) => return response,
                                Err(e) => {
                                    tracing::error!(error = %e, "Failed to synthesize OpenAI streaming response");
                                    // Fall through to return JSON
                                }
                            }
                        }
                        Err(e) => {
                            // Log full response JSON for diagnosis
                            let json_preview = serde_json::to_string_pretty(&json)
                                .unwrap_or_else(|_| format!("{:?}", json));
                            tracing::warn!(
                                error = %e,
                                response_json = %json_preview,
                                "Cannot parse as ChatCompletionResponse for synthesis - dumping full response"
                            );
                        }
                    }
                }
            }
        }

        // Return complete JSON response (either client wants non-streaming, or synthesis failed)
        let final_body = if let Some(ref json) = json_value {
            serde_json::to_vec(json).unwrap_or_else(|_| body_bytes.to_vec())
        } else {
            body_bytes.to_vec()
        };

        let mut response = Response::builder().status(status);

        // Only set JSON content-type if we successfully parsed/modified as JSON
        // Otherwise preserve backend's content-type
        if json_value.is_some() {
            response = response.header(header::CONTENT_TYPE, "application/json");
        }

        for (name, value) in headers {
            if let Some(name) = name {
                // Skip headers that Axum will handle
                if name == header::CONTENT_LENGTH || name == header::TRANSFER_ENCODING {
                    continue;
                }
                // Skip content-type ONLY if we already set it (json_value.is_some())
                // Otherwise preserve backend's content-type
                if name == header::CONTENT_TYPE && json_value.is_some() {
                    continue;
                }
                response = response.header(name, value);
            }
        }

        response.body(Body::from(final_body)).unwrap().into_response()
    }

    /// Simple pass-through with no fix application or stats collection
    /// Used for monitoring endpoints like /props, /slots, /health
    async fn proxy_passthrough(&self, req: Request<Body>) -> Response {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let headers = req.headers().clone();
        let path = uri.path();
        let query = uri.query();

        tracing::debug!(method = %method, path = %path, query = ?query, "Pass-through request");

        // Read body
        let body_bytes = match to_bytes(req.into_body(), 1024 * 1024 * 10).await {
            Ok(bytes) => bytes,
            Err(e) => {
                return (StatusCode::BAD_REQUEST, format!("Failed to read body: {}", e))
                    .into_response();
            }
        };

        // Build complete URL with query string as-is (don't parse/re-encode)
        let backend_url = if let Some(q) = query {
            format!("{}{}?{}", self.state.config.backend.base_url(), path, q)
        } else {
            format!("{}{}", self.state.config.backend.base_url(), path)
        };

        tracing::debug!(
            backend_url = %backend_url,
            has_query = query.is_some(),
            "Building pass-through request"
        );

        // Create request with complete URL (query string included)
        let mut backend_req = self.state.http_client.request(
            Method::from_bytes(method.as_str().as_bytes()).unwrap(),
            &backend_url,
        );

        // Copy headers (skip Host and Authorization as we'll set those explicitly)
        for (name, value) in headers.iter() {
            // Skip headers that will be set explicitly or handled by reqwest
            if name == header::HOST || name == header::AUTHORIZATION {
                continue;
            }

            backend_req = backend_req.header(name, value);
        }

        // Add Authorization header if api_key is configured
        if let Some(ref api_key) = self.state.config.backend.api_key {
            backend_req = backend_req.header(header::AUTHORIZATION, format!("Bearer {}", api_key));
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
        let raw_body = match backend_response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return (StatusCode::BAD_GATEWAY, format!("Failed to read response: {}", e))
                    .into_response();
            }
        };

        // Check for Content-Encoding and decompress if needed
        let content_encoding = headers
            .get(header::CONTENT_ENCODING)
            .and_then(|ce| ce.to_str().ok());

        let body = match decompress_body(&raw_body, content_encoding) {
            Ok(decompressed) => decompressed,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    content_encoding = ?content_encoding,
                    "Failed to decompress pass-through response"
                );
                return (
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to decompress response: {}", e),
                )
                    .into_response();
            }
        };

        let mut response = Response::builder().status(status);
        for (name, value) in headers {
            if let Some(name) = name {
                // Skip Content-Length and Transfer-Encoding - Axum will handle these
                // This ensures consistent behavior with handle_non_streaming_response
                if name == header::CONTENT_LENGTH || name == header::TRANSFER_ENCODING {
                    continue;
                }
                response = response.header(name, value);
            }
        }
        response.body(Body::from(body)).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, BackendConfig, StreamingConfig};
    use crate::fixes::FixRegistry;
    use crate::exporters::ExporterManager;
    use std::collections::HashMap;

    fn create_test_handler_with_streaming(streaming_config: StreamingConfig) -> ProxyHandler {
        let config = AppConfig {
            server: crate::config::ServerConfig {
                port: 8066,
                host: "0.0.0.0".to_string(),
            },
            backend: BackendConfig::default(),
            fixes: crate::config::FixesConfig {
                enabled: false,
                modules: HashMap::new(),
            },
            stats: crate::config::StatsConfig {
                enabled: false,
                format: crate::config::StatsFormat::Pretty,
                log_interval: 1,
            },
            exporters: crate::config::ExportersConfig {
                influxdb: crate::config::InfluxDbConfig {
                    enabled: false,
                    url: "http://localhost:8086".to_string(),
                    org: "test".to_string(),
                    bucket: "test".to_string(),
                    token: "test".to_string(),
                    batch_size: 1,
                    flush_interval_seconds: 1,
                },
            },
            detection: crate::config::DetectionConfig::default(),
            streaming: streaming_config,
        };

        let http_client = reqwest::Client::new();
        let fix_registry = FixRegistry::new();
        let exporter_manager = ExporterManager::new();

        ProxyHandler::new(ProxyState {
            config: std::sync::Arc::new(config),
            http_client,
            fix_registry: std::sync::Arc::new(fix_registry),
            exporter_manager: std::sync::Arc::new(exporter_manager),
        })
    }

    // REMOVED: Tests for should_stream() method
    // The method has been removed since we now always force non-streaming backend requests
    // and synthesize streaming responses when clients request them.
    //
    // New architecture:
    // - All backend requests: stream = false
    // - Client wants streaming: synthesize from complete JSON
    // - Client wants non-streaming: return complete JSON as-is

    #[test]
    fn test_is_json_content_type() {
        // Standard JSON content types
        assert!(ProxyHandler::is_json_content_type("application/json"));
        assert!(ProxyHandler::is_json_content_type("application/json; charset=utf-8"));
        assert!(ProxyHandler::is_json_content_type("APPLICATION/JSON"));

        // Vendor-specific JSON types
        assert!(ProxyHandler::is_json_content_type("application/vnd.api+json"));
        assert!(ProxyHandler::is_json_content_type("application/vnd.github.v3+json"));
        assert!(ProxyHandler::is_json_content_type("application/vnd.custom+json; charset=utf-8"));

        // Non-JSON types
        assert!(!ProxyHandler::is_json_content_type("text/html"));
        assert!(!ProxyHandler::is_json_content_type("text/plain"));
        assert!(!ProxyHandler::is_json_content_type("text/javascript"));
        assert!(!ProxyHandler::is_json_content_type("application/javascript"));
        assert!(!ProxyHandler::is_json_content_type("image/png"));
        assert!(!ProxyHandler::is_json_content_type("image/jpeg"));
        assert!(!ProxyHandler::is_json_content_type("text/css"));
        assert!(!ProxyHandler::is_json_content_type("application/octet-stream"));
    }
}
