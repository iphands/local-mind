//! Main proxy server implementation

use axum::{
    extract::State,
    routing::{any, get},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
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

/// Run the proxy server
pub async fn run_server(
    config: AppConfig,
    fix_registry: FixRegistry,
    exporter_manager: ExporterManager,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create HTTP client for backend connections
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(config.backend.timeout_seconds))
        .pool_max_idle_per_host(10)
        .build()?;

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
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("llama-proxy listening on {}", addr);
    tracing::info!("Proxying to {}", config.backend.url());

    axum::serve(listener, app).await?;

    Ok(())
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
