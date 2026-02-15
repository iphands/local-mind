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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_config_url() {
        let config = BackendConfig {
            host: "localhost".to_string(),
            port: 8080,
            timeout_seconds: 300,
        };
        assert_eq!(config.url(), "http://localhost:8080");
    }

    #[test]
    fn test_backend_config_remote_host() {
        let config = BackendConfig {
            host: "192.168.1.100".to_string(),
            port: 9000,
            timeout_seconds: 60,
        };
        assert_eq!(config.url(), "http://192.168.1.100:9000");
    }

    #[test]
    fn test_stats_format_default() {
        let format = StatsFormat::default();
        assert!(matches!(format, StatsFormat::Pretty));
    }

    #[test]
    fn test_stats_format_serde() {
        // Test serialization
        let pretty = StatsFormat::Pretty;
        let json = StatsFormat::Json;
        let compact = StatsFormat::Compact;

        assert_eq!(serde_json::to_string(&pretty).unwrap(), "\"pretty\"");
        assert_eq!(serde_json::to_string(&json).unwrap(), "\"json\"");
        assert_eq!(serde_json::to_string(&compact).unwrap(), "\"compact\"");
    }

    #[test]
    fn test_stats_format_deserialize() {
        let pretty: StatsFormat = serde_json::from_str("\"pretty\"").unwrap();
        let json: StatsFormat = serde_json::from_str("\"json\"").unwrap();
        let compact: StatsFormat = serde_json::from_str("\"compact\"").unwrap();

        assert!(matches!(pretty, StatsFormat::Pretty));
        assert!(matches!(json, StatsFormat::Json));
        assert!(matches!(compact, StatsFormat::Compact));
    }

    #[test]
    fn test_fix_module_config() {
        let config = FixModuleConfig {
            enabled: true,
            options: HashMap::new(),
        };
        assert!(config.enabled);
        assert!(config.options.is_empty());
    }

    #[test]
    fn test_fix_module_config_with_options() {
        let mut options = HashMap::new();
        options.insert("remove_duplicate".to_string(), serde_yaml::Value::Bool(true));

        let config = FixModuleConfig {
            enabled: false,
            options,
        };
        assert!(!config.enabled);
        assert!(config.options.contains_key("remove_duplicate"));
    }

    #[test]
    fn test_config_error_display() {
        let err = ConfigError::NotFound("test.yaml".to_string());
        assert!(err.to_string().contains("test.yaml"));

        let err = ConfigError::Parse(serde_yaml::from_str::<AppConfig>("invalid").unwrap_err());
        assert!(err.to_string().contains("parse"));
    }

    #[test]
    fn test_load_or_default_none() {
        let result = AppConfig::load_or_default(None);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::NotFound(_)));
    }

    #[test]
    fn test_load_or_default_with_path() {
        let result = AppConfig::load_or_default(Some(Path::new("/nonexistent/config.yaml")));
        assert!(result.is_err());
    }

    #[test]
    fn test_server_config() {
        let config = ServerConfig {
            port: 8066,
            host: "0.0.0.0".to_string(),
        };
        assert_eq!(config.port, 8066);
        assert_eq!(config.host, "0.0.0.0");
    }

    #[test]
    fn test_stats_config() {
        let config = StatsConfig {
            enabled: true,
            format: StatsFormat::Json,
            log_interval: 5,
        };
        assert!(config.enabled);
        assert!(matches!(config.format, StatsFormat::Json));
        assert_eq!(config.log_interval, 5);
    }

    #[test]
    fn test_influxdb_config() {
        let config = InfluxDbConfig {
            enabled: true,
            url: "http://localhost:8086".to_string(),
            org: "my-org".to_string(),
            bucket: "metrics".to_string(),
            token: "secret".to_string(),
            batch_size: 10,
            flush_interval_seconds: 5,
        };
        assert!(config.enabled);
        assert_eq!(config.url, "http://localhost:8086");
        assert_eq!(config.batch_size, 10);
    }
}
