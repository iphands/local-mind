use std::path::Path;

use super::{AppConfig, ConfigError};

/// Load configuration from a YAML file
pub fn load_config<P: AsRef<Path>>(path: P) -> Result<AppConfig, ConfigError> {
    let path = path.as_ref();

    if !path.exists() {
        return Err(ConfigError::NotFound(path.display().to_string()));
    }

    let content = std::fs::read_to_string(path)?;
    let config: AppConfig = serde_yaml::from_str(&content)?;

    // Validate server configuration
    validate_server_config(&config.server)?;

    // Validate streaming configuration
    validate_streaming_config(&config.streaming)?;

    // Validate backend configuration (single or multi-backend)
    if let Some(ref backends) = config.backends {
        // Multi-backend mode: validate all groups and their nodes
        if backends.is_empty() {
            return Err(ConfigError::Validation("No backend groups configured".to_string()));
        }
        for (name, group) in backends {
            if group.nodes.is_empty() {
                return Err(ConfigError::Validation(format!(
                    "Backend group '{}' has no nodes configured",
                    name
                )));
            }
            // Validate strategy
            if group.strategy != "round_robin" && group.strategy != "priority_free" {
                return Err(ConfigError::Validation(format!(
                    "Backend group '{}' has invalid strategy '{}'. Use 'round_robin' or 'priority_free'",
                    name, group.strategy
                )));
            }
            // Validate all node URLs
            for node in &group.nodes {
                validate_backend_url(&node.url)?;
                if node.timeout_seconds == 0 {
                    return Err(ConfigError::Validation(format!(
                        "Node in group '{}' has timeout_seconds of 0",
                        name
                    )));
                }
            }
        }
    } else {
        // Single backend mode
        validate_backend_config(&config.backend)?;
        validate_backend_url(&config.backend.url)?;
    }

    Ok(config)
}

/// Validate that the backend URL is properly formatted
fn validate_backend_url(url: &str) -> Result<(), ConfigError> {
    let parsed = url::Url::parse(url).map_err(|e| ConfigError::Validation(format!("Invalid backend URL '{}': {}", url, e)))?;

    // Ensure scheme is http or https
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(ConfigError::Validation(format!(
            "Backend URL must use http:// or https://, got '{}'",
            scheme
        )));
    }

    // Ensure there's a host
    if parsed.host_str().is_none() {
        return Err(ConfigError::Validation(format!("Backend URL must include a host: '{}'", url)));
    }

    Ok(())
}

/// Validate server configuration
fn validate_server_config(config: &super::ServerConfig) -> Result<(), ConfigError> {
    // Validate port range (1-65535)
    if config.port == 0 {
        return Err(ConfigError::Validation(format!(
            "Server port must be between 1-65535, got {}",
            config.port
        )));
    }

    Ok(())
}

/// Validate backend configuration
fn validate_backend_config(config: &super::BackendConfig) -> Result<(), ConfigError> {
    // Validate timeout_seconds > 0
    if config.timeout_seconds == 0 {
        return Err(ConfigError::Validation(
            "Backend timeout_seconds must be greater than 0".to_string(),
        ));
    }

    Ok(())
}

/// Validate streaming configuration
fn validate_streaming_config(config: &super::StreamingMode) -> Result<(), ConfigError> {
    // Validate streaming mode is implemented
    if !config.is_implemented() {
        return Err(ConfigError::Validation(format!(
            "Streaming mode '{}' is not yet implemented. Use 'disabled' or 'fake'",
            match config {
                super::StreamingMode::Disabled => "disabled",
                super::StreamingMode::Fake => "fake",
                super::StreamingMode::Accumulator => "accumulator",
            }
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_missing_config() {
        let result = load_config("/nonexistent/config.yaml");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::NotFound(_)));
    }

    #[test]
    fn test_load_config_invalid_yaml() {
        // Create a temp file with invalid YAML
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_invalid_config.yaml");
        std::fs::write(&temp_file, "invalid: yaml: content: [").unwrap();

        let result = load_config(&temp_file);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::Parse(_)));

        // Cleanup
        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_load_config_valid() {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_valid_config.yaml");

        let config_content = r#"
server:
  port: 8066
  host: "0.0.0.0"

backend:
  url: "http://localhost:8080"
  timeout_seconds: 300

fixes:
  enabled: true
  modules:
    toolcall_bad_filepath:
      enabled: true
      remove_duplicate: true

stats:
  enabled: true
  format: "pretty"
  log_interval: 1

exporters:
  influxdb:
    enabled: false
    url: "http://localhost:8086"
    org: "my-org"
    bucket: "llama-metrics"
    token: "test-token"
    batch_size: 10
    flush_interval_seconds: 5
"#;
        std::fs::write(&temp_file, config_content).unwrap();

        let result = load_config(&temp_file);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.server.port, 8066);
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.backend.url, "http://localhost:8080");
        assert!(config.fixes.enabled);
        assert!(config.stats.enabled);

        // Cleanup
        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_load_config_https() {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_https_config.yaml");

        let config_content = r#"
server:
  port: 8066
  host: "0.0.0.0"

backend:
  url: "https://example.com:4234"
  timeout_seconds: 300
  tls:
    accept_invalid_certs: true

fixes:
  enabled: true
  modules: {}

stats:
  enabled: true
  format: "json"
  log_interval: 1

exporters:
  influxdb:
    enabled: false
    url: ""
    org: ""
    bucket: ""
    token: ""
    batch_size: 0
    flush_interval_seconds: 0
"#;
        std::fs::write(&temp_file, config_content).unwrap();

        let result = load_config(&temp_file);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.backend.url, "https://example.com:4234");
        assert!(config.backend.is_tls());
        assert!(config.backend.tls.is_some());
        assert!(config.backend.tls.unwrap().accept_invalid_certs);

        // Cleanup
        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_load_config_minimal() {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_minimal_config.yaml");

        // Minimal config with required fields only
        let config_content = r#"
server:
  port: 8066
  host: "0.0.0.0"

backend:
  url: "http://localhost:8080"
  timeout_seconds: 300

fixes:
  enabled: true
  modules: {}

stats:
  enabled: true
  format: "json"
  log_interval: 1

exporters:
  influxdb:
    enabled: false
    url: ""
    org: ""
    bucket: ""
    token: ""
    batch_size: 0
    flush_interval_seconds: 0
"#;
        std::fs::write(&temp_file, config_content).unwrap();

        let result = load_config(&temp_file);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert!(config.fixes.modules.is_empty());

        // Cleanup
        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_config_from_file() {
        let result = AppConfig::from_file("/nonexistent/path.yaml");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_backend_url_valid() {
        assert!(validate_backend_url("http://localhost:8080").is_ok());
        assert!(validate_backend_url("https://example.com:4234").is_ok());
        assert!(validate_backend_url("http://192.168.1.100:9000").is_ok());
    }

    #[test]
    fn test_validate_backend_url_invalid() {
        // Invalid URL format
        assert!(validate_backend_url("not-a-url").is_err());

        // Wrong scheme
        assert!(validate_backend_url("ftp://example.com").is_err());

        // Missing host
        assert!(validate_backend_url("http://").is_err());
    }

    #[test]
    fn test_validate_server_config_valid_port() {
        let config = super::super::ServerConfig {
            port: 8066,
            host: "0.0.0.0".to_string(),
        };
        assert!(validate_server_config(&config).is_ok());
    }

    #[test]
    fn test_validate_server_config_zero_port() {
        let config = super::super::ServerConfig {
            port: 0,
            host: "0.0.0.0".to_string(),
        };
        let result = validate_server_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
        assert!(err.to_string().contains("port"));
        assert!(err.to_string().contains("1-65535"));
    }

    #[test]
    fn test_validate_backend_config_valid_timeout() {
        let config = super::super::BackendConfig {
            url: "http://localhost:8080".to_string(),
            timeout_seconds: 300,
            tls: None,
            model: None,
            api_key: None,
            strip_path_prefix: None,
        };
        assert!(validate_backend_config(&config).is_ok());
    }

    #[test]
    fn test_validate_backend_config_zero_timeout() {
        let config = super::super::BackendConfig {
            url: "http://localhost:8080".to_string(),
            timeout_seconds: 0,
            tls: None,
            model: None,
            api_key: None,
            strip_path_prefix: None,
        };
        let result = validate_backend_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
        assert!(err.to_string().contains("timeout"));
    }

    #[test]
    fn test_validate_streaming_config_valid_modes() {
        // Test disabled mode
        let config = super::super::StreamingMode::Disabled;
        assert!(validate_streaming_config(&config).is_ok());

        // Test fake mode
        let config = super::super::StreamingMode::Fake;
        assert!(validate_streaming_config(&config).is_ok());
    }

    #[test]
    fn test_validate_streaming_config_unimplemented_mode() {
        let config = super::super::StreamingMode::Accumulator;
        let result = validate_streaming_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
        assert!(err.to_string().contains("not yet implemented"));
    }
    #[test]
    fn test_invalid_timeout() {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_invalid_timeout.yaml");

        let config_content = r#"
server:
  port: 8066
  host: "0.0.0.0"

backend:
  url: "http://localhost:8080"
  timeout_seconds: 0

fixes:
  enabled: true
  modules: {}

stats:
  enabled: true
  format: "json"
  log_interval: 1

exporters:
  influxdb:
    enabled: false
    url: ""
    org: ""
    bucket: ""
    token: ""
    batch_size: 0
    flush_interval_seconds: 0
"#;
        std::fs::write(&temp_file, config_content).unwrap();

        let result = load_config(&temp_file);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
        assert!(err.to_string().contains("timeout_seconds must be greater than 0"));

        // Cleanup
        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_load_config_with_invalid_port() {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_invalid_port_config.yaml");

        let config_content = r#"
server:
  port: 0
  host: "0.0.0.0"

backend:
  url: "http://localhost:8080"
  timeout_seconds: 300

fixes:
  enabled: true
  modules: {}

stats:
  enabled: true
  format: "json"
  log_interval: 1

exporters:
  influxdb:
    enabled: false
    url: ""
    org: ""
    bucket: ""
    token: ""
    batch_size: 0
    flush_interval_seconds: 0
"#;
        std::fs::write(&temp_file, config_content).unwrap();

        let result = load_config(&temp_file);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
        assert!(err.to_string().contains("port"));

        // Cleanup
        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_load_config_with_invalid_timeout() {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_invalid_timeout_config.yaml");

        let config_content = r#"
server:
  port: 8066
  host: "0.0.0.0"

backend:
  url: "http://localhost:8080"
  timeout_seconds: 0

fixes:
  enabled: true
  modules: {}

stats:
  enabled: true
  format: "json"
  log_interval: 1

exporters:
  influxdb:
    enabled: false
    url: ""
    org: ""
    bucket: ""
    token: ""
    batch_size: 0
    flush_interval_seconds: 0
"#;
        std::fs::write(&temp_file, config_content).unwrap();

        let result = load_config(&temp_file);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
        assert!(err.to_string().contains("timeout"));

        // Cleanup
        let _ = std::fs::remove_file(&temp_file);
    }
}
