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
use crate::augment::AugmentBackend;
use crate::backends::{build_balancer_from_groups, build_balancer_from_single, preflight, LoadBalancer};
use crate::config::AppConfig;
use crate::exporters::ExporterManager;
use crate::fixes::FixRegistry;

/// Shared state for the proxy
#[derive(Clone)]
pub struct ProxyState {
    pub config: Arc<AppConfig>,
    pub load_balancer: Arc<dyn LoadBalancer>,
    pub fix_registry: Arc<FixRegistry>,
    pub exporter_manager: Arc<ExporterManager>,
    pub augment_backend: Option<Arc<AugmentBackend>>,
    pub hide_requests: bool,
    pub log_augmented_request_text: bool,
}

/// Run the proxy server
pub async fn run_server(
    config: AppConfig,
    fix_registry: FixRegistry,
    exporter_manager: ExporterManager,
    hide_requests: bool,
    log_augmented_request_text: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = config;

    // Build load balancer from configuration
    let load_balancer = if let Some(ref mut backends) = config.backends {
        // Multi-backend mode with named groups
        tracing::info!("Using multi-backend mode with {} group(s)", backends.len());

        for (name, group) in backends.iter() {
            let mapping_str = if group.mappings.is_empty() {
                "catch-all".to_string()
            } else {
                format!("{:?}", group.mappings)
            };
            tracing::info!(
                group = %name,
                mappings = %mapping_str,
                strategy = %group.strategy,
                node_count = group.nodes.len(),
                "Backend group configured"
            );
        }

        // Run preflight: discover models, auto-set overrides, warn on ambiguity
        preflight::run_preflight_multi(backends).await;

        build_balancer_from_groups(backends.clone())?
    } else {
        // Single backend mode (backward compatibility)
        tracing::info!(
            url = %config.backend.base_url(),
            timeout_seconds = config.backend.timeout_seconds,
            "Using single backend mode"
        );

        preflight::run_preflight_single(&config.backend).await;

        build_balancer_from_single(
            config.backend.url.clone(),
            config.backend.timeout_seconds,
            config.backend.tls.as_ref(),
            config.backend.model.clone(),
            config.backend.api_key.clone(),
            config.backend.strip_path_prefix.clone(),
        )?
    };

    tracing::info!(strategy = %load_balancer.strategy_name(), "Load balancing strategy");

    // Initialize augment backend if configured
    let augment_backend = if let Some(ref augment_config) = config.augment_backend {
        if augment_config.enabled {
            match AugmentBackend::from_config(augment_config) {
                Ok(backend) => {
                    tracing::info!(
                        url = %augment_config.url,
                        model = %augment_config.model,
                        prompt_file = %augment_config.prompt_file,
                        "Augment backend enabled"
                    );
                    Some(Arc::new(backend))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to initialize augment backend, will be disabled");
                    None
                }
            }
        } else {
            tracing::info!("Augment backend disabled via config");
            None
        }
    } else {
        None
    };

    let state = ProxyState {
        config: Arc::new(config.clone()),
        load_balancer,
        fix_registry: Arc::new(fix_registry),
        exporter_manager: Arc::new(exporter_manager),
        augment_backend,
        hide_requests,
        log_augmented_request_text,
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
