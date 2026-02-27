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
use crate::backends::{build_balancer, BackendNode, LoadBalancer};
use crate::config::{resolve_backend_nodes, resolve_strategy, AppConfig};
use crate::exporters::ExporterManager;
use crate::fixes::FixRegistry;

/// Shared state for the proxy
#[derive(Clone)]
pub struct ProxyState {
    pub config: Arc<AppConfig>,
    pub load_balancer: Arc<dyn LoadBalancer>,
    pub fix_registry: Arc<FixRegistry>,
    pub exporter_manager: Arc<ExporterManager>,
}

/// Run the proxy server
pub async fn run_server(
    config: AppConfig,
    fix_registry: FixRegistry,
    exporter_manager: ExporterManager,
) -> Result<(), Box<dyn std::error::Error>> {
    // Resolve backend nodes and strategy
    let node_configs = resolve_backend_nodes(&config);
    let strategy = resolve_strategy(&config);

    // Build BackendNode instances (each owns its own HTTP client)
    let mut nodes = Vec::with_capacity(node_configs.len());
    for node_cfg in &node_configs {
        let node = BackendNode::from_config(
            node_cfg.url.clone(),
            node_cfg.timeout_seconds,
            node_cfg.tls.as_ref(),
            node_cfg.model.clone(),
            node_cfg.api_key.clone(),
        )?;
        nodes.push(node);
    }

    // Build load balancer
    let load_balancer = build_balancer(nodes, &strategy)?;

    // Log backend configuration
    for node in load_balancer.all_nodes() {
        tracing::info!(url = %node.base_url(), "Backend node registered");
    }
    tracing::info!(strategy = %load_balancer.strategy_name(), "Load balancing strategy");

    let state = ProxyState {
        config: Arc::new(config.clone()),
        load_balancer,
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
