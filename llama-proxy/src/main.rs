//! llama-proxy: HTTP reverse proxy for llama.cpp server
//!
//! A Rust-based reverse proxy that sits in front of llama.cpp's llama-server
//! and provides:
//! - Response fixing for malformed tool calls
//! - Performance metrics logging
//! - Remote metrics export (InfluxDB, etc.)

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Trace => write!(f, "trace"),
            LogLevel::Debug => write!(f, "debug"),
            LogLevel::Info => write!(f, "info"),
            LogLevel::Warn => write!(f, "warn"),
            LogLevel::Error => write!(f, "error"),
        }
    }
}

use llama_proxy::{
    config::AppConfig,
    create_default_registry,
    exporters::{ExporterManager, InfluxDbExporter},
    run_server,
};

#[derive(Parser)]
#[command(name = "llama-proxy")]
#[command(version = "0.1.0")]
#[command(about = "HTTP reverse proxy for llama.cpp server")]
#[command(long_about = "
llama-proxy is a reverse proxy for llama.cpp's llama-server that provides:
  - Response fixing for malformed tool calls (e.g., Qwen3-Coder)
  - Performance metrics logging (tokens/sec, timing, context usage)
  - Remote metrics export to InfluxDB and other systems

Example usage:
  llama-proxy run --config config.yaml
  llama-proxy list-fixes --verbose
")]
struct Cli {
    /// Path to config file
    #[arg(short, long, global = true, default_value = "config.yaml")]
    config: PathBuf,

    /// Set logging level (trace, debug, info, warn, error)
    #[arg(long, global = true, value_name = "LEVEL")]
    log_level: Option<LogLevel>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the proxy server
    Run {
        /// Override listen port
        #[arg(short, long)]
        port: Option<u16>,
        /// Override backend URL (e.g., "https://example.com:4234")
        #[arg(long)]
        backend_url: Option<String>,
    },

    /// List all available response fix modules
    ListFixes {
        /// Show detailed information
        #[arg(short, long)]
        verbose: bool,
    },

    /// Validate configuration file
    CheckConfig,

    /// Test connection to backend llama-server
    TestBackend,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let level_filter = if let Some(level) = cli.log_level {
        level.to_string()
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
            .to_string()
    };

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(&level_filter))
        .init();

    match cli.command {
        Commands::Run {
            port,
            backend_url,
        } => {
            run_proxy(cli.config, port, backend_url).await?;
        }
        Commands::ListFixes { verbose } => {
            list_fixes(verbose);
        }
        Commands::CheckConfig => {
            check_config(cli.config)?;
        }
        Commands::TestBackend => {
            test_backend(cli.config).await?;
        }
    }

    Ok(())
}

/// Run the proxy server
async fn run_proxy(
    config_path: PathBuf,
    port_override: Option<u16>,
    backend_url_override: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let mut config = load_config_or_exit(&config_path);

    // Apply CLI overrides
    if let Some(port) = port_override {
        config.server.port = port;
    }
    if let Some(url) = backend_url_override {
        config.backend.url = url;
    }

    tracing::info!("Loading configuration from {:?}", config_path);

    // Create fix registry
    let mut fix_registry = create_default_registry();

    // Configure fixes from config
    if !config.fixes.enabled {
        // Disable all fixes - collect names first to avoid borrow issues
        let fix_names: Vec<String> = fix_registry
            .list_fixes()
            .iter()
            .map(|f| f.name().to_string())
            .collect();
        for name in fix_names {
            fix_registry.set_enabled(&name, false);
        }
    } else {
        // Apply individual fix settings
        fix_registry.configure(&config.fixes.modules);
    }

    let enabled_fixes: Vec<&str> = fix_registry
        .list_fixes()
        .iter()
        .filter(|f| fix_registry.is_enabled(f.name()))
        .map(|f| f.name())
        .collect();

    tracing::info!(
        enabled_fixes = ?enabled_fixes,
        "Fix modules configured"
    );

    // Create exporter manager
    let mut exporter_manager = ExporterManager::new();

    // Add InfluxDB exporter if enabled
    if config.exporters.influxdb.enabled {
        match InfluxDbExporter::from_config(&config.exporters.influxdb) {
            Ok(exporter) => {
                exporter_manager.add(Arc::new(exporter));
                tracing::info!("InfluxDB exporter enabled");
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to initialize InfluxDB exporter");
            }
        }
    }

    // Run the server
    run_server(config, fix_registry, exporter_manager).await?;

    Ok(())
}

/// List all available fix modules
fn list_fixes(verbose: bool) {
    let registry = create_default_registry();

    println!("Available response fix modules:\n");

    for fix in registry.list_fixes() {
        if verbose {
            println!("  {}:", fix.name());
            println!("    {}", fix.description());
            println!("    Enabled: {}", registry.is_enabled(fix.name()));
            println!();
        } else {
            let status = if registry.is_enabled(fix.name()) {
                "[enabled]"
            } else {
                "[disabled]"
            };
            println!("  {:30} {} - {}", fix.name(), status, fix.description());
        }
    }

    if verbose {
        println!("\nTo enable/disable fixes, edit your config.yaml:");
        println!("\nfixes:");
        println!("  enabled: true");
        println!("  modules:");
        println!("    toolcall_bad_filepath:");
        println!("      enabled: true");
        println!("      remove_duplicate: true");
    }
}

/// Validate configuration file
fn check_config(config_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    match AppConfig::from_file(&config_path) {
        Ok(config) => {
            println!("✓ Configuration file is valid\n");
            println!("Server:");
            println!("  Listen: {}:{}", config.server.host, config.server.port);
            println!("\nBackend:");
            println!("  URL: {}", config.backend.url);
            println!("  TLS: {}", if config.backend.is_tls() { "enabled" } else { "disabled" });
            if let Some(ref tls) = config.backend.tls {
                if tls.accept_invalid_certs {
                    println!("  TLS: Accepting invalid certificates");
                }
                if let Some(ref ca) = tls.ca_cert_path {
                    println!("  TLS CA: {}", ca);
                }
                if let Some(ref cert) = tls.client_cert_path {
                    println!("  TLS Client Cert: {}", cert);
                }
            }
            println!("  Timeout: {}s", config.backend.timeout_seconds);
            println!("\nFixes:");
            println!("  Global: {}", config.fixes.enabled);
            for (name, module) in &config.fixes.modules {
                println!("  {} : {}", name, module.enabled);
            }
            println!("\nStats:");
            println!("  Enabled: {}", config.stats.enabled);
            println!("  Format: {:?}", config.stats.format);
            println!("\nExporters:");
            println!("  InfluxDB: {}", config.exporters.influxdb.enabled);
            Ok(())
        }
        Err(e) => {
            eprintln!("✗ Configuration error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Test connection to backend
async fn test_backend(config_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config_or_exit(&config_path);
    let base_url = config.backend.base_url();
    let health_url = format!("{}/health", base_url);

    println!("Testing connection to backend: {}", health_url);

    let mut client_builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5));

    // Apply TLS settings for test
    if let Some(ref tls) = config.backend.tls {
        if tls.accept_invalid_certs {
            client_builder = client_builder.danger_accept_invalid_certs(true);
        }
        if let Some(ref ca_path) = tls.ca_cert_path {
            let ca_cert = std::fs::read(ca_path)?;
            let ca_cert = reqwest::Certificate::from_pem(&ca_cert)?;
            client_builder = client_builder.add_root_certificate(ca_cert);
        }
        if let (Some(cert_path), Some(key_path)) =
            (&tls.client_cert_path, &tls.client_key_path)
        {
            let cert_pem = std::fs::read(cert_path)?;
            let key_pem = std::fs::read(key_path)?;
            let identity = reqwest::Identity::from_pem(&[cert_pem, key_pem].concat())?;
            client_builder = client_builder.identity(identity);
        }
    }

    let client = client_builder.build()?;

    match client.get(&health_url).send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                println!("✓ Backend is reachable");
                println!("  Status: {}", resp.status());

                if let Ok(body) = resp.text().await {
                    println!("  Response: {}", body.trim());
                }
            } else {
                println!("✗ Backend returned error status: {}", resp.status());
            }
        }
        Err(e) => {
            println!("✗ Failed to connect to backend: {}", e);
            std::process::exit(1);
        }
    }

    // Also try /v1/models
    let models_url = format!("{}/v1/models", base_url);
    println!("\nTesting /v1/models endpoint: {}", models_url);

    match client.get(&models_url).send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                println!("✓ /v1/models endpoint available");
                if let Ok(body) = resp.text().await {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                        if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
                            println!("  Available models: {}", data.len());
                            for model in data.iter().take(5) {
                                if let Some(id) = model.get("id").and_then(|i| i.as_str()) {
                                    println!("    - {}", id);
                                }
                            }
                        }
                    }
                }
            } else {
                println!("  /v1/models returned: {}", resp.status());
            }
        }
        Err(e) => {
            println!("  /v1/models error: {}", e);
        }
    }

    Ok(())
}

/// Load configuration or exit with error
fn load_config_or_exit(config_path: &PathBuf) -> AppConfig {
    match AppConfig::from_file(config_path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error loading configuration: {}", e);
            eprintln!("\nMake sure you have a config.yaml file.");
            eprintln!("You can copy config.yaml.default and modify it:");
            eprintln!("  cp config.yaml.default config.yaml");
            std::process::exit(1);
        }
    }
}
