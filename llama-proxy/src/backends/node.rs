//! Runtime handle for a single backend node

use std::sync::atomic::AtomicUsize;
use std::time::Duration;

use crate::config::TlsConfig;

/// A single backend node with its own HTTP client
pub struct BackendNode {
    pub url: String,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub timeout_seconds: u64,
    pub http_client: reqwest::Client,
    pub active_requests: AtomicUsize,
    /// Model names this backend handles (empty = handles all models)
    pub mapping: Vec<String>,
}

impl BackendNode {
    /// Construct a BackendNode from configuration parameters
    pub fn from_config(
        url: String,
        timeout_seconds: u64,
        tls: Option<&TlsConfig>,
        model: Option<String>,
        api_key: Option<String>,
        mapping: Vec<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let http_client = build_node_client(timeout_seconds, tls)?;
        Ok(Self {
            url,
            model,
            api_key,
            timeout_seconds,
            http_client,
            active_requests: AtomicUsize::new(0),
            mapping,
        })
    }

    /// Returns the base URL with trailing slash stripped
    pub fn base_url(&self) -> &str {
        self.url.trim_end_matches('/')
    }

    /// Check if this node can handle the given model
    /// - If model is None: node handles it (no model specified in request)
    /// - If mapping is empty: node handles all models (catch-all)
    /// - If mapping contains the model: node handles it
    pub fn handles_model(&self, model: Option<&str>) -> bool {
        match model {
            None => true,  // No model specified, any node can handle
            Some(m) => self.mapping.is_empty() || self.mapping.contains(&m.to_string()),
        }
    }
}

/// Build an HTTP client for a single backend node
fn build_node_client(timeout_seconds: u64, tls: Option<&TlsConfig>) -> Result<reqwest::Client, Box<dyn std::error::Error>> {
    let mut client_builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .pool_max_idle_per_host(10);

    if let Some(tls) = tls {
        if tls.accept_invalid_certs {
            client_builder = client_builder.danger_accept_invalid_certs(true);
            tracing::warn!("TLS: Accepting invalid certificates (use only for development/testing)");
        }

        if let Some(ref ca_path) = tls.ca_cert_path {
            let ca_cert = std::fs::read(ca_path)?;
            let ca_cert = reqwest::Certificate::from_pem(&ca_cert)?;
            client_builder = client_builder.add_root_certificate(ca_cert);
            tracing::info!("TLS: Loaded custom CA certificate from {}", ca_path);
        }

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
