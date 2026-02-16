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
    #[serde(default)]
    pub detection: DetectionConfig,
    #[serde(default)]
    pub streaming: StreamingConfig,
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
    /// Full backend URL (e.g., "https://example.com:4234" or "http://localhost:8080")
    pub url: String,
    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    /// TLS configuration options
    #[serde(default)]
    pub tls: Option<TlsConfig>,
}

/// TLS configuration for backend connections
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TlsConfig {
    /// Accept invalid certificates (self-signed, expired)
    #[serde(default)]
    pub accept_invalid_certs: bool,
    /// Path to custom CA certificate (PEM format)
    pub ca_cert_path: Option<String>,
    /// Path to client certificate for mTLS
    pub client_cert_path: Option<String>,
    /// Path to client private key for mTLS
    pub client_key_path: Option<String>,
}

fn default_timeout() -> u64 {
    300
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:8080".to_string(),
            timeout_seconds: default_timeout(),
            tls: None,
        }
    }
}

impl BackendConfig {
    /// Returns the base URL with trailing slash stripped
    pub fn base_url(&self) -> &str {
        self.url.trim_end_matches('/')
    }

    /// Returns true if the URL uses HTTPS
    pub fn is_tls(&self) -> bool {
        self.url.to_lowercase().starts_with("https://")
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
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StatsFormat {
    #[default]
    Pretty,
    Json,
    Compact,
}

/// Pre-parse detection configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DetectionConfig {
    /// Enable pre-parse malformed pattern detection
    /// This runs BEFORE JSON parsing and logs warnings immediately
    #[serde(default = "default_detection_enabled")]
    pub enabled: bool,

    /// Log level for detections: "warn" | "error" | "info"
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_detection_enabled() -> bool {
    true
}

fn default_log_level() -> String {
    "warn".to_string()
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            enabled: default_detection_enabled(),
            log_level: default_log_level(),
        }
    }
}

/// Streaming control configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StreamingConfig {
    /// Enable streaming responses globally (when false, all requests use non-streaming)
    #[serde(default = "default_streaming_enabled")]
    pub enabled: bool,

    /// Allow streaming when request contains tools (default: true)
    /// Set to false to disable streaming for tool requests
    #[serde(default = "default_streaming_on_tools")]
    pub streaming_on_tools: bool,

    /// Per-client streaming rules (User-Agent pattern -> enabled)
    #[serde(default)]
    pub client_rules: HashMap<String, bool>,
}

fn default_streaming_on_tools() -> bool {
    true
}

fn default_streaming_enabled() -> bool {
    true
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            enabled: default_streaming_enabled(),
            streaming_on_tools: default_streaming_on_tools(),
            client_rules: HashMap::new(),
        }
    }
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

    #[error("Configuration validation error: {0}")]
    Validation(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_config_base_url() {
        let config = BackendConfig {
            url: "http://localhost:8080".to_string(),
            timeout_seconds: 300,
            tls: None,
        };
        assert_eq!(config.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_backend_config_https() {
        let config = BackendConfig {
            url: "https://example.com:4234".to_string(),
            timeout_seconds: 300,
            tls: None,
        };
        assert_eq!(config.base_url(), "https://example.com:4234");
        assert!(config.is_tls());
    }

    #[test]
    fn test_backend_config_is_tls() {
        let http_config = BackendConfig {
            url: "http://localhost:8080".to_string(),
            timeout_seconds: 300,
            tls: None,
        };
        assert!(!http_config.is_tls());

        let https_config = BackendConfig {
            url: "https://secure.example.com".to_string(),
            timeout_seconds: 300,
            tls: None,
        };
        assert!(https_config.is_tls());
    }

    #[test]
    fn test_backend_config_trailing_slash() {
        let config = BackendConfig {
            url: "http://localhost:8080/".to_string(),
            timeout_seconds: 300,
            tls: None,
        };
        assert_eq!(config.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_backend_config_default() {
        let config = BackendConfig::default();
        assert_eq!(config.url, "http://localhost:8080");
        assert_eq!(config.timeout_seconds, 300);
        assert!(config.tls.is_none());
    }

    #[test]
    fn test_backend_config_tls_options() {
        let config = BackendConfig {
            url: "https://secure.example.com".to_string(),
            timeout_seconds: 300,
            tls: Some(TlsConfig {
                accept_invalid_certs: true,
                ca_cert_path: Some("/path/to/ca.pem".to_string()),
                client_cert_path: None,
                client_key_path: None,
            }),
        };
        assert!(config.tls.is_some());
        let tls = config.tls.unwrap();
        assert!(tls.accept_invalid_certs);
        assert_eq!(tls.ca_cert_path, Some("/path/to/ca.pem".to_string()));
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
        options.insert(
            "remove_duplicate".to_string(),
            serde_yaml::Value::Bool(true),
        );

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

        let err = ConfigError::Validation("invalid URL".to_string());
        assert!(err.to_string().contains("invalid URL"));
    }

    #[test]
    fn test_load_or_default_none() {
        // Create a temporary directory without config files to test error case
        let temp_dir = tempfile::TempDir::new().unwrap();
        let _ = std::env::set_current_dir(temp_dir.path());

        // When no path is provided and no default files exist, should return NotFound error
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

    #[test]
    fn test_streaming_config_default() {
        let config = StreamingConfig::default();
        assert!(config.enabled);
        assert!(config.streaming_on_tools);
        assert!(config.client_rules.is_empty());
    }

    #[test]
    fn test_streaming_config_with_rules() {
        let mut client_rules = HashMap::new();
        client_rules.insert("claude-code".to_string(), false);
        client_rules.insert("opencode".to_string(), true);

        let config = StreamingConfig {
            enabled: true,
            streaming_on_tools: false,
            client_rules,
        };
        assert!(config.enabled);
        assert!(!config.streaming_on_tools);
        assert_eq!(config.client_rules.len(), 2);
        assert!(!config.client_rules["claude-code"]);
        assert!(config.client_rules["opencode"]);
    }
}
