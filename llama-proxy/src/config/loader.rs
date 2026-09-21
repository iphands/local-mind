use std::collections::HashMap;
use std::path::Path;

use super::validate::{
    require_nonempty, validate_allowed_origins, validate_bind_host, validate_http_url, validate_temperature,
};
use super::{AppConfig, ConfigError};

/// Load configuration from a YAML file.
///
/// This loader is the SINGLE validation home: everything a YAML file can
/// express is checked here once, and consumers downstream trust the result.
/// Task 72's `validate_final` covers only what CLI overrides can change.
pub fn load_config<P: AsRef<Path>>(path: P) -> Result<AppConfig, ConfigError> {
    let path = path.as_ref();

    if !path.exists() {
        return Err(ConfigError::NotFound(path.display().to_string()));
    }

    let content = std::fs::read_to_string(path)?;
    let config: AppConfig = serde_yaml::from_str(&content)?;

    validate_server_config(&config.server)?;
    validate_reprompt(config.reprompt.as_ref())?;
    validate_backends(&config)?;
    if let Some(ref augment) = config.augment_backend {
        validate_http_url(&augment.url, "Augment backend")?;
        require_nonempty(&augment.model, "augment-backend model")?;
    }
    if config.dump.enabled {
        require_nonempty(config.dump.path.trim(), "dump path while dump is enabled")?;
    }

    Ok(config)
}

fn validate_reprompt(rep: Option<&super::RepromptConfig>) -> Result<(), ConfigError> {
    let Some(r) = rep else {
        return Ok(());
    };
    if r.enabled && r.prompt_file.is_none() && r.prompt.is_none() {
        return Err(ConfigError::Validation(
            "reprompt: enabled but neither 'prompt_file' nor 'prompt' is set".to_string(),
        ));
    }
    if let Some(file) = r.prompt_file.as_deref() {
        require_nonempty(file, "reprompt prompt_file")?;
        if !Path::new(file).exists() {
            return Err(ConfigError::Validation(format!(
                "reprompt: prompt_file '{file}' does not exist"
            )));
        }
    }
    for sentinel in &r.done_sentinels {
        require_nonempty(sentinel, "reprompt done_sentinels entry")?;
    }
    Ok(())
}

fn validate_backends(config: &AppConfig) -> Result<(), ConfigError> {
    let Some(backends) = config.backends.as_ref() else {
        if let Some(ref backend) = config.backend {
            validate_backend_config(backend)?;
            validate_backend_url(&backend.url)?;
        }
        return Ok(());
    };

    if backends.is_empty() {
        return Err(ConfigError::Validation("No backend groups configured".to_string()));
    }

    let mut catch_alls = 0;
    let mut model_owner: HashMap<&str, &str> = HashMap::new();
    for (name, group) in backends {
        require_nonempty(name, "backend group name")?;
        if group.mappings.is_empty() {
            catch_alls += 1;
            if catch_alls > 1 {
                return Err(ConfigError::Validation(
                    "Only one catch-all group (empty 'mappings: []') may be configured".to_string(),
                ));
            }
        }
        for model in &group.mappings {
            require_nonempty(model, "backend group mapping entry")?;
            if let Some(prev) = model_owner.insert(model.as_str(), name.as_str()) {
                return Err(ConfigError::Validation(format!(
                    "Model '{model}' is mapped in both group '{prev}' and group '{name}'"
                )));
            }
        }
        if group.nodes.is_empty() {
            return Err(ConfigError::Validation(format!(
                "Backend group '{name}' has no nodes configured"
            )));
        }
        if group.strategy != "round_robin" && group.strategy != "priority_free" {
            return Err(ConfigError::Validation(format!(
                "Backend group '{name}' has invalid strategy '{}'. Use 'round_robin' or 'priority_free'",
                group.strategy
            )));
        }
        for node in &group.nodes {
            validate_backend_url(&node.url)?;
            if node.timeout_seconds == 0 {
                return Err(ConfigError::Validation(format!(
                    "Node in group '{name}' has timeout_seconds of 0"
                )));
            }
            if let Some(t) = node.temperature {
                validate_temperature(t, &format!("Node in group '{name}'"))?;
            }
        }
    }
    Ok(())
}

/// Validate that the backend URL is properly formatted
fn validate_backend_url(url: &str) -> Result<(), ConfigError> {
    validate_http_url(url, "Backend")
}

/// Validate server configuration
fn validate_server_config(config: &super::ServerConfig) -> Result<(), ConfigError> {
    if config.port == 0 {
        return Err(ConfigError::Validation(format!(
            "Server port must be between 1-65535, got {}",
            config.port
        )));
    }
    if let Some(origins) = &config.allowed_origins {
        validate_allowed_origins(origins)?;
    }
    validate_bind_host(&config.host)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_missing_config() {
        let result = load_config("/nonexistent/config.yaml");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::NotFound(_)));
    }

    /// Process-unique scratch path (big-fix 96): fixed /tmp names collide between
    /// concurrent test processes (and leak between runs); pid+seq+stem keeps every
    /// write private. The stem keeps its extension - validators may inspect it.
    fn scratch(stem: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("llama-proxy-{}-{}-{}", std::process::id(), n, stem))
    }

    #[test]
    fn test_load_config_invalid_yaml() {
        // Create a temp file with invalid YAML
        let temp_file = scratch("test_invalid_config.yaml");
        std::fs::write(&temp_file, "invalid: yaml: content: [").unwrap();

        let result = load_config(&temp_file);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::Parse(_)));

        // Cleanup
        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_load_config_valid() {
        let temp_file = scratch("test_valid_config.yaml");

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

exporters:
  influxdb:
    enabled: false
    url: "http://localhost:8086"
    org: "my-org"
    bucket: "llama-metrics"
    token: "test-token"
"#;
        std::fs::write(&temp_file, config_content).unwrap();

        let result = load_config(&temp_file);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.server.port, 8066);
        assert_eq!(config.server.host, "0.0.0.0");
        let backend = config.backend.expect("single backend section");
        assert_eq!(backend.url, "http://localhost:8080");
        assert!(config.fixes.enabled);
        assert!(config.stats.enabled);

        // Cleanup
        let _ = std::fs::remove_file(&temp_file);
    }

    /// D2 (task 84) acceptance: the proxy never batched - a sample is a single
    /// write through the bounded queue (task 81). The legacy keys must be
    /// REJECTED, not silently ignored, so a config claiming batching fails
    /// loudly instead of configuring nothing.
    #[test]
    fn test_influxdb_batch_keys_are_rejected() {
        for key in ["batch_size: 10", "flush_interval_seconds: 5"] {
            let temp_file = scratch(&format!("test_influxdb_{key}.yaml").replace(|c: char| !c.is_alphanumeric(), "_"));
            std::fs::write(
                &temp_file,
                format!(
                    r#"
server:
  port: 8066
  host: "127.0.0.1"

exporters:
  influxdb:
    enabled: true
    url: "http://localhost:8086"
    org: "my-org"
    bucket: "llama-metrics"
    token: "t"
    {key}
"#
                ),
            )
            .unwrap();

            let err = load_config(&temp_file).expect_err(&format!("{key} must be rejected, not ignored"));
            let msg = err.to_string();
            assert!(msg.contains("unknown field"), "{key}: {msg}");
            assert!(
                msg.contains("batch_size") || msg.contains("flush_interval_seconds"),
                "{key}: {msg}"
            );

            let _ = std::fs::remove_file(&temp_file);
        }
    }

    #[test]
    fn test_load_config_https() {
        let temp_file = scratch("test_https_config.yaml");

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

exporters:
  influxdb:
    enabled: false
    url: ""
    org: ""
    bucket: ""
    token: ""
"#;
        std::fs::write(&temp_file, config_content).unwrap();

        let result = load_config(&temp_file);
        assert!(result.is_ok());

        let config = result.unwrap();
        let backend = config.backend.expect("single backend section");
        assert_eq!(backend.url, "https://example.com:4234");
        assert!(backend.is_tls());
        let tls = backend.tls.expect("tls section");
        assert!(tls.accept_invalid_certs);

        // Cleanup
        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_load_config_minimal() {
        let temp_file = scratch("test_minimal_config.yaml");

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

exporters:
  influxdb:
    enabled: false
    url: ""
    org: ""
    bucket: ""
    token: ""
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
            max_concurrent_requests: super::super::default_max_concurrent(),
            allowed_origins: None,
        };
        assert!(validate_server_config(&config).is_ok());
    }

    #[test]
    fn test_server_config_omitted_max_concurrent_is_unlimited() {
        // Configs written before this setting existed must keep behaving as unlimited;
        // defaulting to a finite cap would start shedding traffic on upgrade.
        let yaml = "port: 8066\nhost: \"0.0.0.0\"\n";
        let config: super::super::ServerConfig = serde_yaml::from_str(yaml).expect("should parse without the key");
        assert_eq!(config.max_concurrent_requests, 0);
    }

    #[test]
    fn test_server_config_explicit_max_concurrent_is_honored() {
        let yaml = "port: 8066\nhost: \"0.0.0.0\"\nmax_concurrent_requests: 25\n";
        let config: super::super::ServerConfig = serde_yaml::from_str(yaml).expect("should parse with the key");
        assert_eq!(config.max_concurrent_requests, 25);
    }

    #[test]
    fn test_server_config_allowed_origins_absent_is_none_list_present_is_exact() {
        let absent: super::super::ServerConfig =
            serde_yaml::from_str("port: 8066\nhost: \"0.0.0.0\"\n").expect("absent key must parse");
        assert_eq!(absent.allowed_origins, None, "absent = permissive default");

        let present: super::super::ServerConfig = serde_yaml::from_str(
            "port: 8066\nhost: \"0.0.0.0\"\nallowed_origins: [\"https://app.one\", \"http://localhost:3000\"]\n",
        )
        .expect("list must parse");
        assert_eq!(
            present.allowed_origins,
            Some(vec!["https://app.one".to_string(), "http://localhost:3000".to_string()]),
            "entries must be stored verbatim, unnormalized"
        );
    }

    #[test]
    fn test_validate_server_config_rejects_wildcard_in_allowed_origins() {
        // '*' in the list is a config error, not a silent permissive switch:
        // it must never reach AllowOrigin::list (which panics on '*').
        let config = super::super::ServerConfig {
            port: 8066,
            host: "0.0.0.0".to_string(),
            max_concurrent_requests: super::super::default_max_concurrent(),
            allowed_origins: Some(vec!["https://app.one".to_string(), "*".to_string()]),
        };
        let err = validate_server_config(&config).expect_err("wildcard entry must be rejected");
        assert!(err.to_string().contains("allowed_origins"), "got {err}");
    }

    #[test]
    fn test_validate_server_config_zero_port() {
        let config = super::super::ServerConfig {
            port: 0,
            host: "0.0.0.0".to_string(),
            max_concurrent_requests: super::super::default_max_concurrent(),
            allowed_origins: None,
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
    fn test_invalid_timeout() {
        let temp_file = scratch("test_invalid_timeout.yaml");

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

exporters:
  influxdb:
    enabled: false
    url: ""
    org: ""
    bucket: ""
    token: ""
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

    fn streaming_yaml(mode_line: &str) -> std::path::PathBuf {
        let temp_file = scratch(&format!("test_streaming_{}.yaml", mode_line.replace([':', ' '], "_")));
        std::fs::write(
            &temp_file,
            format!(
                r#"
server:
  port: 8066
  host: "0.0.0.0"
backend:
  url: "http://localhost:8080"
  timeout_seconds: 300
fixes:
  enabled: true
  modules: {{}}
stats:
  enabled: true
  format: "json"
exporters:
  influxdb:
    enabled: false
    url: ""
    org: ""
    bucket: ""
    token: ""
streaming: {}
"#,
                mode_line
            ),
        )
        .unwrap();
        temp_file
    }

    #[test]
    fn test_load_config_accepts_unimplemented_streaming_mode() {
        // The loader validates SHAPE only. Rejecting a mode here would
        // pre-empt the CLI switch, so `--streaming-mode fake` could not
        // override `streaming: passthrough` in the file. run_proxy enforces
        // startability AFTER resolve() applies the precedence.
        let temp_file = streaming_yaml("passthrough");
        let config = load_config(&temp_file).expect("passthrough must load");
        assert_eq!(config.streaming, super::super::StreamingMode::Passthrough);
        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_load_config_rejects_unknown_streaming_mode() {
        let temp_file = streaming_yaml("teleport");
        let err = load_config(&temp_file).expect_err("unknown mode must not load");
        assert!(matches!(err, ConfigError::Parse(_)), "got {:?}", err);
        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_load_config_with_invalid_port() {
        let temp_file = scratch("test_invalid_port_config.yaml");

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

exporters:
  influxdb:
    enabled: false
    url: ""
    org: ""
    bucket: ""
    token: ""
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
        let temp_file = scratch("test_invalid_timeout_config.yaml");

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

exporters:
  influxdb:
    enabled: false
    url: ""
    org: ""
    bucket: ""
    token: ""
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

    #[test]
    fn test_load_config_nodes_only_without_backend_section() {
        // F-H1: a config with ONLY `backends:` groups must load; `backend:` is optional.
        // With no single backend, startup proceeds and requests take the task-4 503 path.
        let temp_file = scratch("test_t69_nodes_only.yaml");
        let config_content = r#"
server:
  port: 8066
  host: "0.0.0.0"

backends:
  main:
    mappings: []
    nodes:
      - url: "http://localhost:8080"
"#;
        std::fs::write(&temp_file, config_content).unwrap();

        let config = load_config(&temp_file).expect("backends-only config must load");
        assert!(config.backend.is_none(), "`backend:` absent must deserialize to None");
        assert!(config.backends.is_some());

        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_load_config_no_backend_at_all_loads_for_503_path() {
        // F-H1: neither `backend:` nor `backends:` must load — the proxy starts and
        // every completion request answers the task-4 503 envelope (NoMatchingBackend).
        let temp_file = scratch("test_t69_no_backend.yaml");
        std::fs::write(&temp_file, "server:\n  port: 8066\n  host: \"0.0.0.0\"\n").unwrap();

        let config = load_config(&temp_file).expect("backend-less config must load (503 path)");
        assert!(config.backend.is_none());
        assert!(config.backends.is_none());

        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_augment_backend_absent_enabled_defaults_false_opt_in() {
        let temp_file = scratch("test_augment_opt_in_config.yaml");
        let config_content = r#"
server:
  port: 8066
  host: "0.0.0.0"

backend:
  url: "http://localhost:8080"
  timeout_seconds: 300

augment-backend:
  url: "http://localhost:8701"
  model: "fast-model"
"#;
        std::fs::write(&temp_file, config_content).unwrap();

        let config = load_config(&temp_file).unwrap();
        let augment = config.augment_backend.expect("section must parse");
        assert!(
            !augment.enabled,
            "F-M13: augment-backend is opt-in - an absent `enabled` key must default to false"
        );
        assert_eq!(augment.url, "http://localhost:8701");
        assert_eq!(augment.model, "fast-model");

        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_augment_backend_explicit_enabled_true_still_honored() {
        let temp_file = scratch("test_augment_opt_in_true_config.yaml");
        let config_content = r#"
server:
  port: 8066
  host: "0.0.0.0"

backend:
  url: "http://localhost:8080"
  timeout_seconds: 300

augment-backend:
  enabled: true
  url: "http://localhost:8701"
  model: "fast-model"
"#;
        std::fs::write(&temp_file, config_content).unwrap();

        let config = load_config(&temp_file).unwrap();
        assert!(
            config.augment_backend.expect("section parsed").enabled,
            "explicit `enabled: true` must still opt in"
        );

        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_augment_backend_timeout_secs_absent_defaults_15_via_loader() {
        let temp_file = scratch("test_augment_timeout_default_config.yaml");
        let config_content = r#"
server:
  port: 8066
  host: "0.0.0.0"

backend:
  url: "http://localhost:8080"
  timeout_seconds: 300

augment-backend:
  enabled: true
  url: "http://localhost:8701"
  model: "fast-model"
"#;
        std::fs::write(&temp_file, config_content).unwrap();

        let config = load_config(&temp_file).unwrap();
        let augment = config.augment_backend.expect("section parsed");
        assert_eq!(
            augment.timeout_secs, 15,
            "F-L8: a real config file without timeout_secs must get 15s, not the old hard-coded 60"
        );

        let _ = std::fs::remove_file(&temp_file);
    }

    fn loaded(yaml: &str, tag: &str) -> Result<super::super::AppConfig, ConfigError> {
        let f = scratch(&format!("test_t71_{tag}.yaml"));
        std::fs::write(&f, yaml).unwrap();
        let r = load_config(&f);
        let _ = std::fs::remove_file(&f);
        r
    }

    const BASE: &str = "server:\n  port: 8066\n  host: \"0.0.0.0\"\nbackend:\n  url: \"http://localhost:8080\"\n";

    #[test]
    fn t71_rejects_junk_server_host_and_accepts_ip_forms() {
        let err = loaded(&BASE.replace("host: \"0.0.0.0\"", "host: \"not a host!!\""), "junk_host")
            .expect_err("junk host must be rejected");
        assert!(err.to_string().contains("RFC-1123"), "got {err}");
        for host in ["\"0.0.0.0\"", "\"::1\"", "\"[::1]\"", "\"localhost\"", "\"cosmo.lan\""] {
            let yaml = BASE.replace("host: \"0.0.0.0\"", &format!("host: {host}"));
            assert!(loaded(&yaml, "host_ok").is_ok(), "{host} must load");
        }
    }

    #[test]
    fn t71_rejects_reprompt_prompt_file_that_does_not_exist() {
        let yaml = format!("{BASE}reprompt:\n  enabled: true\n  prompt_file: \"/nonexistent/does-not-exist.md\"\n");
        let err = loaded(&yaml, "reprompt_missing").expect_err("missing prompt_file must be rejected");
        assert!(err.to_string().contains("does not exist"), "got {err}");

        let prompt = scratch("test_t71_prompt_exists.md");
        std::fs::write(&prompt, "continue please").unwrap();
        let yaml = format!("{BASE}reprompt:\n  enabled: true\n  prompt_file: \"{}\"\n", prompt.display());
        assert!(loaded(&yaml, "reprompt_present").is_ok());
        let _ = std::fs::remove_file(&prompt);
    }

    #[test]
    fn t71_rejects_dump_enabled_with_empty_path() {
        let err = loaded(&format!("{BASE}dump:\n  enabled: true\n  path: \"\"\n"), "dump_empty")
            .expect_err("dump enabled with empty path must be rejected");
        assert!(err.to_string().contains("dump path"), "got {err}");
        assert!(loaded(
            &format!("{BASE}dump:\n  enabled: true\n  path: \"{}\"\n", scratch("t71-dumps").display()),
            "dump_ok"
        )
        .is_ok());
    }

    #[test]
    fn t71_rejects_non_https_augment_url_and_empty_model() {
        let err = loaded(
            &format!("{BASE}augment-backend:\n  enabled: true\n  url: \"ftp://nope\"\n  model: \"fast\"\n"),
            "aug_ftp",
        )
        .expect_err("ftp augment url must be rejected");
        assert!(err.to_string().contains("http"), "got {err}");

        let err = loaded(
            &format!("{BASE}augment-backend:\n  enabled: true\n  url: \"http://x.test:1\"\n  model: \"\"\n"),
            "aug_empty_model",
        )
        .expect_err("empty augment model must be rejected");
        assert!(err.to_string().contains("model"), "got {err}");

        assert!(loaded(
            &format!("{BASE}augment-backend:\n  enabled: true\n  url: \"https://x.test:1\"\n  model: \"fast\"\n"),
            "aug_ok",
        )
        .is_ok());
    }

    #[test]
    fn t71_node_temperature_pin_table() {
        for (lit, ok) in [
            ("0.0", true),
            ("2.0", true),
            ("1.5", true),
            ("-0.1", false),
            ("2.5", false),
            (".inf", false),
            ("-.inf", false),
            (".nan", false),
        ] {
            let yaml = format!(
                "server:\n  port: 8066\n  host: \"0.0.0.0\"\nbackends:\n  g:\n    mappings: []\n    nodes:\n      - url: \"http://localhost:8080\"\n        temperature: {lit}\n"
            );
            let got = loaded(&yaml, "temp");
            assert_eq!(
                got.is_ok(),
                ok,
                "temperature {lit} ok={ok}, got {:?}",
                got.err().map(|e| e.to_string())
            );
        }
    }

    #[test]
    fn t71_rejects_duplicate_model_mapping_across_groups() {
        let yaml = "server:\n  port: 8066\n  host: \"0.0.0.0\"\nbackends:\n  g1:\n    mappings: [\"qwen3\"]\n    nodes:\n      - url: \"http://localhost:8080\"\n  g2:\n    mappings: [\"qwen3\", \"llama\"]\n    nodes:\n      - url: \"http://localhost:8081\"\n";
        let err = loaded(yaml, "dup_map").expect_err("duplicate mapping must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("qwen3") && msg.contains("g1") && msg.contains("g2"), "got {msg}");
    }

    #[test]
    fn t71_rejects_second_catch_all_group() {
        let yaml = "server:\n  port: 8066\n  host: \"0.0.0.0\"\nbackends:\n  g1:\n    mappings: []\n    nodes:\n      - url: \"http://localhost:8080\"\n  g2:\n    mappings: []\n    nodes:\n      - url: \"http://localhost:8081\"\n";
        let err = loaded(yaml, "two_ca").expect_err("two catch-alls must be rejected");
        assert!(err.to_string().contains("catch-all"), "got {err}");
    }

    #[test]
    fn t71_rejects_empty_backends_map_and_empty_group_name() {
        let err = loaded("server:\n  port: 8066\n  host: \"0.0.0.0\"\nbackends: {}\n", "empty_groups")
            .expect_err("empty groups must be rejected");
        assert!(err.to_string().contains("No backend groups"), "got {err}");

        let err = loaded(
            "server:\n  port: 8066\n  host: \"0.0.0.0\"\nbackends:\n  \"\":\n    mappings: [\"m\"]\n    nodes:\n      - url: \"http://localhost:8080\"\n",
            "empty_name",
        )
        .expect_err("empty group name must be rejected");
        assert!(err.to_string().contains("group name"), "got {err}");
    }

    #[test]
    fn t71_rejects_empty_done_sentinel_entry() {
        let yaml = format!("{BASE}reprompt:\n  enabled: true\n  prompt: \"go\"\n  done_sentinels: [\"DONE\", \"\"]\n");
        let err = loaded(&yaml, "empty_sentinel").expect_err("empty sentinel must be rejected");
        assert!(err.to_string().contains("done_sentinels"), "got {err}");
    }
}
