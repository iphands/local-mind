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
    backends::BackendNode,
    config::{resolve_backend_nodes, resolve_strategy, AppConfig},
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
        /// Override streaming mode (disabled, fake, accumulator)
        #[arg(long, value_name = "MODE")]
        streaming_mode: Option<String>,
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
            streaming_mode,
        } => {
            run_proxy(cli.config, port, backend_url, streaming_mode).await?;
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
    streaming_mode_override: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let mut config = load_config_or_exit(&config_path);

    // Apply CLI overrides
    if let Some(port) = port_override {
        config.server.port = port;
    }
    if let Some(url) = backend_url_override {
        if config.backends.is_some() {
            tracing::warn!("--backend-url ignored: multi-backend 'backends:' config is active");
        } else {
            config.backend.url = url;
        }
    }
    if let Some(mode_str) = streaming_mode_override {
        use llama_proxy::config::StreamingMode;
        config.streaming = match mode_str.to_lowercase().as_str() {
            "disabled" => StreamingMode::Disabled,
            "fake" => StreamingMode::Fake,
            "accumulator" => StreamingMode::Accumulator,
            _ => {
                eprintln!(
                    "Invalid streaming mode: {}. Use 'disabled', 'fake', or 'accumulator'.",
                    mode_str
                );
                std::process::exit(1);
            }
        };
    }

    // Validate streaming mode is implemented
    if !config.streaming.is_implemented() {
        eprintln!("Error: Streaming mode '{:?}' is not yet implemented.", config.streaming);
        eprintln!("Available modes:");
        eprintln!("  - disabled: Forces streaming off completely");
        eprintln!("  - fake: Forces non-streaming to backend, synthesizes streaming to frontend (default)");
        eprintln!("  - accumulator: NOT IMPLEMENTED");
        std::process::exit(1);
    }

    tracing::info!("Loading configuration from {:?}", config_path);

    // Log all configuration settings
    log_config_settings(&config);

    // Create fix registry
    let mut fix_registry = create_default_registry();

    // Configure fixes from config
    if !config.fixes.enabled {
        // Disable all fixes - collect names first to avoid borrow issues
        let fix_names: Vec<String> = fix_registry.list_fixes().iter().map(|f| f.name().to_string()).collect();
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

/// Log all configuration settings at startup (masks sensitive values)
fn log_config_settings(config: &AppConfig) {
    tracing::info!("=== Configuration ===");

    // Server
    tracing::info!(
        host = %config.server.host,
        port = config.server.port,
        "Server"
    );

    // Backend(s)
    let nodes = resolve_backend_nodes(config);
    let strategy = resolve_strategy(config);
    if config.backends.is_some() {
        tracing::info!(strategy = %strategy, node_count = nodes.len(), "Backends (multi-node mode)");
        for (i, node) in nodes.iter().enumerate() {
            tracing::info!(
                index = i,
                url = %node.url.trim_end_matches('/'),
                timeout_seconds = node.timeout_seconds,
                "Backend node"
            );
        }
    } else {
        tracing::info!(
            url = %config.backend.base_url(),
            timeout_seconds = config.backend.timeout_seconds,
            "Backend"
        );
        if let Some(ref tls) = config.backend.tls {
            tracing::info!(
                accept_invalid_certs = tls.accept_invalid_certs,
                ca_cert = tls.ca_cert_path.as_deref().unwrap_or("none"),
                client_cert = tls.client_cert_path.as_deref().unwrap_or("none"),
                "Backend TLS"
            );
        }
    }

    // Fixes
    tracing::info!(
        enabled = config.fixes.enabled,
        module_count = config.fixes.modules.len(),
        "Fixes"
    );
    for (name, module) in &config.fixes.modules {
        tracing::info!(
            module = %name,
            enabled = module.enabled,
            "Fix module"
        );
    }

    // Detection
    tracing::info!(
        enabled = config.detection.enabled,
        log_level = %config.detection.log_level,
        "Detection"
    );

    // Streaming
    tracing::info!(
        mode = ?config.streaming,
        "Streaming"
    );

    // Stats
    tracing::info!(
        enabled = config.stats.enabled,
        format = ?config.stats.format,
        log_interval = config.stats.log_interval,
        "Stats"
    );

    // Exporters
    tracing::info!(
        enabled = config.exporters.influxdb.enabled,
        url = %config.exporters.influxdb.url,
        org = %config.exporters.influxdb.org,
        bucket = %config.exporters.influxdb.bucket,
        batch_size = config.exporters.influxdb.batch_size,
        flush_interval_seconds = config.exporters.influxdb.flush_interval_seconds,
        "InfluxDB exporter"
        // Note: token is intentionally NOT logged
    );

    tracing::info!("=== End Configuration ===");
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
            let nodes = resolve_backend_nodes(&config);
            let strategy = resolve_strategy(&config);
            println!("\nBackend(s):");
            println!("  Strategy: {}", strategy);
            println!("  Node count: {}", nodes.len());
            for (i, node) in nodes.iter().enumerate() {
                println!("  Node [{}]: {}", i, node.url.trim_end_matches('/'));
                println!("    Timeout: {}s", node.timeout_seconds);
            }
            println!("\nFixes:");
            println!("  Global: {}", config.fixes.enabled);
            for (name, module) in &config.fixes.modules {
                println!("  {} : {}", name, module.enabled);
            }
            println!("\nStats:");
            println!("  Enabled: {}", config.stats.enabled);
            println!("  Format: {:?}", config.stats.format);
            println!("\nStreaming:");
            println!("  Mode: {:?}", config.streaming);
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
    let node_configs = resolve_backend_nodes(&config);

    println!("Testing {} backend node(s)...\n", node_configs.len());

    for (i, node_cfg) in node_configs.iter().enumerate() {
        let node = BackendNode::from_config(
            node_cfg.url.clone(),
            5, // short timeout for test
            node_cfg.tls.as_ref(),
            node_cfg.model.clone(),
            node_cfg.api_key.clone(),
        )?;

        let base_url = node.base_url().to_string();
        println!("Node [{}]: {}", i, base_url);

        let health_url = format!("{}/health", base_url);
        println!("  Testing /health: {}", health_url);

        match node.http_client.get(&health_url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    println!("  ✓ Reachable ({})", resp.status());
                    if let Ok(body) = resp.text().await {
                        println!("    Response: {}", body.trim());
                    }
                } else {
                    println!("  ✗ Error status: {}", resp.status());
                }
            }
            Err(e) => {
                println!("  ✗ Failed to connect: {}", e);
            }
        }

        let models_url = format!("{}/v1/models", base_url);
        println!("  Testing /v1/models: {}", models_url);

        match node.http_client.get(&models_url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    println!("  ✓ /v1/models available");
                    if let Ok(body) = resp.text().await {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                            if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
                                println!("    Available models: {}", data.len());
                                for model in data.iter().take(5) {
                                    if let Some(id) = model.get("id").and_then(|i| i.as_str()) {
                                        println!("      - {}", id);
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

        println!();
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
