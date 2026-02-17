//! Main proxy server implementation

use axum::{
    extract::State,
    routing::{any, get},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use super::handler::ProxyHandler;
use crate::config::AppConfig;
use crate::exporters::ExporterManager;
use crate::fixes::FixRegistry;

/// Shared state for the proxy
#[derive(Clone)]
pub struct ProxyState {
    pub config: Arc<AppConfig>,
    pub http_client: reqwest::Client,
    pub fix_registry: Arc<FixRegistry>,
    pub exporter_manager: Arc<ExporterManager>,
}

/// Build an HTTP client with TLS configuration
fn build_http_client(config: &AppConfig) -> Result<reqwest::Client, Box<dyn std::error::Error>> {
    let mut client_builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.backend.timeout_seconds))
        .pool_max_idle_per_host(10);

    // Apply TLS configuration if present
    if let Some(ref tls) = config.backend.tls {
        if tls.accept_invalid_certs {
            client_builder = client_builder.danger_accept_invalid_certs(true);
            tracing::warn!("TLS: Accepting invalid certificates (use only for development/testing)");
        }

        // Load custom CA certificate if provided
        if let Some(ref ca_path) = tls.ca_cert_path {
            let ca_cert = std::fs::read(ca_path)?;
            let ca_cert = reqwest::Certificate::from_pem(&ca_cert)?;
            client_builder = client_builder.add_root_certificate(ca_cert);
            tracing::info!("TLS: Loaded custom CA certificate from {}", ca_path);
        }

        // Load client certificate for mTLS if both cert and key are provided
        if let (Some(cert_path), Some(key_path)) = (&tls.client_cert_path, &tls.client_key_path) {
            let cert_pem = std::fs::read(cert_path)?;
            let key_pem = std::fs::read(key_path)?;

            let identity = reqwest::Identity::from_pem(&[cert_pem, key_pem].concat())?;
            client_builder = client_builder.identity(identity);
            tracing::info!("TLS: Loaded client certificate from {} for mTLS", cert_path);
        }
    }

    Ok(client_builder.build()?)
}

/// Run the proxy server
pub async fn run_server(
    config: AppConfig,
    fix_registry: FixRegistry,
    exporter_manager: ExporterManager,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create HTTP client for backend connections with TLS config
    let http_client = build_http_client(&config)?;

    let state = ProxyState {
        config: Arc::new(config.clone()),
        http_client,
        fix_registry: Arc::new(fix_registry),
        exporter_manager: Arc::new(exporter_manager),
    };

    // Build the router
    let app = Router::new()
        // Health check
        .route("/health", get(health_handler))
        // Catch-all proxy routes
        .route("/v1/*path", any(proxy_handler))
        .route("/*path", any(proxy_handler))
        .fallback(proxy_handler_fallback)
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("llama-proxy listening on {}", addr);
    tracing::info!("Proxying to {}", config.backend.base_url());

    Ok(axum::serve(listener, app).await?)
}

/// Health check endpoint
async fn health_handler() -> &'static str {
    "OK"
}

/// Main proxy handler for matched routes
async fn proxy_handler(State(state): State<ProxyState>, req: axum::extract::Request) -> axum::response::Response {
    let handler = ProxyHandler::new(state);
    handler.handle(req).await
}

/// Fallback handler for unmatched routes
async fn proxy_handler_fallback(State(state): State<ProxyState>, req: axum::extract::Request) -> axum::response::Response {
    let handler = ProxyHandler::new(state);
    handler.handle(req).await
}
