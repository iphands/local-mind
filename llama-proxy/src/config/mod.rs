mod loader;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

pub use loader::load_config;

/// Main application configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub backend: BackendConfig,
    pub fixes: FixesConfig,
    pub stats: StatsConfig,
    pub exporters: ExportersConfig,
}

/// Proxy server configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
}

/// Backend llama-server configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BackendConfig {
    pub host: String,
    pub port: u16,
    pub timeout_seconds: u64,
}

impl BackendConfig {
    pub fn url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// Response fix modules configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FixesConfig {
    pub enabled: bool,
    pub modules: HashMap<String, FixModuleConfig>,
}

/// Individual fix module configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FixModuleConfig {
    pub enabled: bool,
    #[serde(flatten)]
    pub options: HashMap<String, serde_yaml::Value>,
}

/// Stats logging configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StatsConfig {
    pub enabled: bool,
    pub format: StatsFormat,
    pub log_interval: u32,
}

/// Stats output format
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StatsFormat {
    #[default]
    Pretty,
    Json,
    Compact,
}

/// Exporters configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExportersConfig {
    pub influxdb: InfluxDbConfig,
}

/// InfluxDB exporter configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InfluxDbConfig {
    pub enabled: bool,
    pub url: String,
    pub org: String,
    pub bucket: String,
    pub token: String,
    pub batch_size: usize,
    pub flush_interval_seconds: u64,
}

impl AppConfig {
    /// Load configuration from a YAML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        load_config(path)
    }

    /// Load configuration with fallback to default path
    pub fn load_or_default(config_path: Option<&Path>) -> Result<Self, ConfigError> {
        match config_path {
            Some(path) => Self::from_file(path),
            None => {
                // Try default locations
                let default_paths = ["config.yaml", "config.yml", "./config/config.yaml"];
                for p in default_paths {
                    let path = Path::new(p);
                    if path.exists() {
                        return Self::from_file(path);
                    }
                }
                Err(ConfigError::NotFound(
                    "No config file found. Tried: config.yaml, config.yml, ./config/config.yaml"
                        .to_string(),
                ))
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Configuration file not found: {0}")]
    NotFound(String),

    #[error("Failed to read configuration file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse configuration: {0}")]
    Parse(#[from] serde_yaml::Error),
}
