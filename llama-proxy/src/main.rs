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
    config::{AppConfig, BackendConfig, FixesConfig},
    create_default_registry,
    exporters::{ExporterManager, InfluxDbExporter},
    fixes::{create_registry_from_config, FixRegistry},
    run_server,
};
use std::path::Path;

#[derive(Parser)]
#[command(name = "llama-proxy")]
#[command(version = env!("CARGO_PKG_VERSION"))]
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
        /// Override streaming mode (disabled, fake, passthrough). Takes precedence over the config file.
        #[arg(long, value_name = "MODE")]
        streaming_mode: Option<String>,
        /// Hide request log lines (only show response stats)
        #[arg(long)]
        hide_requests: bool,
        /// Log the full augmented request text at INFO level after augmentation injection
        #[arg(long)]
        log_augmented_request_text: bool,
        /// Dump request/response pairs to directory for debugging
        #[arg(long)]
        dump: Option<PathBuf>,
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

    let (env_filter, log_env_warning) =
        resolve_log_filter(cli.log_level.map(|level| level.to_string()), std::env::var("RUST_LOG").ok());
    if let Some(warning) = log_env_warning {
        eprintln!("{warning}");
    }

    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    match cli.command {
        Commands::Run {
            port,
            backend_url,
            streaming_mode,
            hide_requests,
            log_augmented_request_text,
            dump,
        } => {
            run_proxy(
                cli.config,
                port,
                backend_url,
                streaming_mode,
                hide_requests,
                log_augmented_request_text,
                dump,
            )
            .await?;
        }
        Commands::ListFixes { verbose } => {
            list_fixes(&cli.config, verbose);
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
    hide_requests: bool,
    log_augmented_request_text: bool,
    dump_path: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let mut config = load_config_or_exit(&config_path);

    // Apply CLI overrides
    let mut applied_overrides: Vec<String> = Vec::new();
    if let Some(port) = port_override {
        config.server.port = port;
        applied_overrides.push(format!("port={port}"));
    }
    if let Some(url) = backend_url_override {
        if config.backends.is_some() {
            tracing::warn!("--backend-url ignored: multi-backend 'backends:' config is active");
        } else if let Some(b) = config.backend.as_mut() {
            b.url = url;
            applied_overrides.push(format!("backend-url={}", b.url));
        } else {
            applied_overrides.push(format!("backend-url={url}"));
            config.backend = Some(BackendConfig {
                url,
                ..BackendConfig::default()
            });
        }
    }
    // Streaming mode precedence: CLI switch > config file > default (fake)
    config.streaming = match llama_proxy::config::StreamingMode::resolve(streaming_mode_override.as_deref(), config.streaming) {
        Ok(mode) => mode,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    if streaming_mode_override.is_some() {
        applied_overrides.push(format!("streaming={:?}", config.streaming));
    }

    if applied_overrides.is_empty() {
        tracing::info!(config = %config_path.display(), "Configuration active (no CLI overrides applied)");
    } else {
        tracing::info!(config = %config_path.display(), overrides = ?applied_overrides, "Configuration active (CLI overrides applied)");
    }

    // Log all configuration settings
    log_config_settings(&config);

    // Apply dump path override if provided
    if let Some(ref dump_path) = dump_path {
        config.dump.enabled = true;
        config.dump.path = dump_path.to_string_lossy().to_string();
        tracing::info!(dump_path = %dump_path.display(), "Debug dump mode enabled");
    }

    // F-M1/F-M2: validate the FINAL merged config. The loader checked the
    // file; only what CLI overrides could have broken is re-checked here.
    if let Err(e) = validate_final(&config) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    // Build the fix registry honoring the `fixes:` config at startup: a module
    // with `enabled: false` is never constructed into the registry, so it
    // cannot run; each skip is named by a "not constructed" INFO line from
    // create_registry_from_config.
    let fix_registry = create_registry_from_config(&config.fixes);

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
        if let Some(gate_err) = influxdb_startup_gate(config.exporters.influxdb.enabled, &config_path) {
            eprintln!("Error: {gate_err}");
            std::process::exit(1);
        }
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
    run_server(
        config,
        fix_registry,
        exporter_manager,
        hide_requests,
        log_augmented_request_text,
    )
    .await?;

    Ok(())
}

/// plan:27 (D3): `exporters.influxdb.enabled: true` on a binary compiled
/// without the `influxdb` cargo feature is a `run`-path startup HARD ERROR,
/// not a WARN-and-boot with a dead exporter (F11 BUG-2 adjudication). Returns
/// the actionable message when the gate fires; the caller turns it into the
/// startup error (exit 1, no listener bound). `check-config` deliberately
/// never consults this gate — its validation surface is load+structure only
/// (F7 pinned its exit codes).
#[cfg(not(feature = "influxdb"))]
fn influxdb_startup_gate(enabled: bool, config_path: &Path) -> Option<String> {
    enabled.then(|| {
        format!(
            "exporters.influxdb.enabled is true but this binary was built without the \
             `influxdb` cargo feature — rebuild with `cargo build --features influxdb` \
             or set exporters.influxdb.enabled: false in {}",
            config_path.display()
        )
    })
}

/// Feature-compiled-in counterpart: the gate can never fire — `from_config`
/// builds the real exporter and the run path behaves exactly as before (F11-B2
/// scope lock: feature-ON path byte-identical behavior).
#[cfg(feature = "influxdb")]
fn influxdb_startup_gate(_enabled: bool, _config_path: &Path) -> Option<String> {
    None
}

/// Single source of truth for printing the effective configuration. Shared by
/// `log_config_settings` (run, via tracing) and `check_config` (via println),
/// so the two views can never drift apart. Plain lines, fixed section order;
/// the InfluxDB token is intentionally never rendered.
fn render_config_summary(config: &AppConfig) -> Vec<String> {
    let mut lines = vec![format!("Server: {}:{}", config.server.host, config.server.port)];

    if let Some(ref backends) = config.backends {
        lines.push(format!("Backend: multi-group mode ({} group(s))", backends.len()));
        for (name, group) in backends {
            let mapping_str = if group.mappings.is_empty() {
                "catch-all".to_string()
            } else {
                format!("{:?}", group.mappings)
            };
            lines.push(format!(
                "  Group [{name}]: mappings={mapping_str} strategy={} nodes={}",
                group.strategy,
                group.nodes.len()
            ));
            for (i, node) in group.nodes.iter().enumerate() {
                lines.push(format!("    Node [{i}]: {}", node.url.trim_end_matches('/')));
            }
        }
    } else if let Some(ref backend) = config.backend {
        lines.push(format!(
            "Backend: single {} (timeout {}s)",
            backend.base_url(),
            backend.timeout_seconds
        ));
        if let Some(ref tls) = backend.tls {
            lines.push(format!(
                "  TLS: accept_invalid_certs={} ca_cert={} client_cert={}",
                tls.accept_invalid_certs,
                tls.ca_cert_path.as_deref().unwrap_or("none"),
                tls.client_cert_path.as_deref().unwrap_or("none")
            ));
        }
    } else {
        lines.push("Backend: none configured — completion requests answer 503".to_string());
    }

    lines.push(format!(
        "Fixes: enabled={} modules={}",
        config.fixes.enabled,
        config.fixes.modules.len()
    ));
    for (name, module) in &config.fixes.modules {
        lines.push(format!("  {name}: {}", module.enabled));
    }

    lines.push(format!(
        "Streaming: mode={} effective={} honored_on={}",
        config.streaming,
        config.streaming.effective_label(),
        if config.streaming.is_passthrough() {
            "/v1/chat/completions with stream:true"
        } else {
            "all streaming clients"
        }
    ));
    if config.streaming.is_passthrough() {
        lines.push("  Anthropic /v1/messages: buffered + synthesized (no OpenAI->Anthropic SSE translator yet)".to_string());
        lines.push(format!(
            "  Fixes on this path: {} - DETECT ONLY, not repaired (passthrough_fix_unrepaired_total)",
            if config.fixes.enabled {
                "enabled"
            } else {
                "disabled in config"
            }
        ));
        let reprompt_asked = config.reprompt.as_ref().map(|r| r.enabled).unwrap_or(false);
        lines.push(format!(
            "  Reprompt on the streamed path: cannot run{}",
            if reprompt_asked {
                " - config asks for enabled: true; it still runs on non-streaming and /v1/messages"
            } else {
                ""
            }
        ));
    }

    lines.push(format!(
        "Stats: enabled={} format={:?}",
        config.stats.enabled, config.stats.format
    ));
    lines.push(format!(
        "InfluxDB exporter: enabled={} url={} org={} bucket={}",
        config.exporters.influxdb.enabled,
        config.exporters.influxdb.url,
        config.exporters.influxdb.org,
        config.exporters.influxdb.bucket
    ));
    lines
}

/// Log all configuration settings at startup (masks sensitive values)
fn log_config_settings(config: &AppConfig) {
    tracing::info!("=== Configuration ===");
    for line in render_config_summary(config) {
        tracing::info!("{line}");
    }

    // A mode can quietly switch off features the config explicitly asked for. Say it
    // once, loudly, naming them - otherwise the user debugs "why didn't reprompt fire"
    // with no idea the mode they chose is why.
    if config.streaming.is_passthrough() {
        let reprompt_asked = config.reprompt.as_ref().map(|r| r.enabled).unwrap_or(false);
        let degraded = config.fixes.enabled || reprompt_asked;

        let mut gives_up = Vec::new();
        if config.fixes.enabled {
            gives_up.push("on that path fixes DETECT but do NOT repair (passthrough_fix_unrepaired_total)");
        }
        if reprompt_asked {
            gives_up.push("on that path reprompt cannot run - it needs the complete JSON body. Non-streaming and /v1/messages requests still reprompt");
        }

        let msg = format!(
            "streaming: passthrough forwards backend SSE verbatim for /v1/chat/completions with stream:true. \
             Everything else (non-streaming, /v1/messages) stays buffered + synthesized. {}",
            if gives_up.is_empty() {
                "Nothing configured is bypassed by this mode.".to_string()
            } else {
                format!(
                    "This mode gives up on the streamed path: {}. Want repair + premature-stop recovery there? use: streaming: fake",
                    gives_up.join("; ")
                )
            }
        );

        if degraded {
            tracing::warn!("{}", msg);
        } else {
            tracing::info!("{}", msg);
        }
    }

    tracing::info!("=== End Configuration ===");
}

/// List all known fix modules with their real enabled state.
///
/// The global `--config` flag defaults to "config.yaml". When that path is a
/// file, its `fixes:` section decides what prints, through the same
/// `create_registry_from_config` construction the server uses at startup
/// (task 32) - a module disabled in config prints `[disabled]`; an invalid
/// config file is a hard error (exit 1), same as check-config. When no file
/// exists at the path, the default config applies: every module prints
/// `[enabled]`, preceded by a note naming the missing path.
fn list_fixes(config_path: &Path, verbose: bool) {
    let fixes_cfg = if config_path.is_file() {
        load_config_or_exit(config_path).fixes
    } else {
        println!(
            "note: no config file at {} - showing default state (every module enabled)\n",
            config_path.display()
        );
        FixesConfig::default()
    };

    let states = fix_module_states(&create_default_registry(), &create_registry_from_config(&fixes_cfg));

    println!("Available response fix modules:\n");

    for state in &states {
        if verbose {
            println!("  {}:", state.name);
            println!("    {}", state.description);
            println!("    Enabled: {}", state.enabled);
            println!();
        } else {
            let status = if state.enabled { "[enabled]" } else { "[disabled]" };
            println!("  {:30} {} - {}", state.name, status, state.description);
        }
    }

    if verbose {
        println!("\nTo enable/disable fixes, edit your config.yaml:");
        println!("\nfixes:");
        println!("  enabled: true");
        println!("  modules:");
        println!("    toolcall_bad_filepath:");
        println!("      enabled: true");
    }
}

/// One row of `list-fixes`: a known module and its real enabled state.
struct FixModuleState {
    name: String,
    description: String,
    enabled: bool,
}

/// Pure (disclosed extraction for testability): the enabled state of every
/// known fix module, derived from the CONSTRUCTED registry - never from a
/// hardcoded `true`. Task 32 removed config-disabled modules from
/// construction entirely, so membership in `constructed` IS the enabled
/// answer; `catalog` (the full default registry) supplies name, description,
/// and row order for the modules the config skipped.
fn fix_module_states(catalog: &FixRegistry, constructed: &FixRegistry) -> Vec<FixModuleState> {
    catalog
        .list_fixes()
        .iter()
        .map(|fix| FixModuleState {
            name: fix.name().to_string(),
            description: fix.description().to_string(),
            enabled: constructed.is_enabled(fix.name()),
        })
        .collect()
}

/// Validate configuration file
fn check_config(config_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Loading configuration from {config_path:?}");
    match AppConfig::from_file(&config_path) {
        Ok(config) => {
            println!("✓ Configuration file is valid\n");
            for line in render_config_summary(&config) {
                println!("{line}");
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("✗ Configuration error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Honest /v1/models probe outcome: HTTP success alone is NOT evidence of a
/// model list - the body must actually carry a `data[]` array. A 200 whose body
/// is HTML, an error envelope, or empty counts as a FAILED probe and shows what
/// came back instead. Returns (is_model_list, detail lines to print).
fn models_probe_body_outcome(body: &str) -> (bool, Vec<String>) {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
    match parsed.as_ref().and_then(|v| v.get("data")).and_then(|d| d.as_array()) {
        Some(data) => {
            let mut details = vec![format!("    Available models: {}", data.len())];
            for model in data.iter().take(5) {
                if let Some(id) = model.get("id").and_then(|i| i.as_str()) {
                    details.push(format!("      - {id}"));
                }
            }
            (true, details)
        }
        None => {
            let snippet: String = body.chars().take(120).collect();
            (
                false,
                vec![
                    "  ✗ /v1/models: 200 but the body is not a model list (no data[] array)".to_string(),
                    format!("    body starts: {snippet:?}"),
                ],
            )
        }
    }
}

/// Probe ONE node's /health and /v1/models - the single probe implementation
/// shared by the grouped and single loops (they were verbatim clones). Runs
/// exactly two probes; returns (report lines to print, failed probe count).
/// The caller owns the node label, the blank separator, and the counters.
async fn probe_one_node(node: &BackendNode) -> (Vec<String>, usize) {
    let base_url = node.base_url();
    let mut lines = Vec::new();
    let mut failures = 0usize;

    let health_url = format!("{base_url}{}", node.effective_path("/health"));
    lines.push(format!("  Testing {}: {}", node.effective_path("/health"), health_url));
    match node.http_client.get(&health_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            lines.push(format!("  ✓ Reachable ({})", resp.status()));
            if let Ok(body) = resp.text().await {
                lines.push(format!("    Response: {}", body.trim()));
            }
        }
        Ok(resp) => {
            lines.push(format!("  ✗ Error status: {}", resp.status()));
            failures += 1;
        }
        Err(e) => {
            lines.push(format!("  ✗ Failed to connect: {}", e));
            failures += 1;
        }
    }

    let models_url = format!("{base_url}{}", node.effective_path("/v1/models"));
    lines.push(format!("  Testing {}: {}", node.effective_path("/v1/models"), models_url));
    match node.http_client.get(&models_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            lines.push(format!("  ✓ /v1/models answered {}", resp.status()));
            let body = resp.text().await.unwrap_or_default();
            let (list_ok, details) = models_probe_body_outcome(&body);
            lines.extend(details);
            if !list_ok {
                failures += 1;
            }
        }
        Ok(resp) => {
            lines.push(format!("  ✗ /v1/models returned: {}", resp.status()));
            failures += 1;
        }
        Err(e) => {
            lines.push(format!("  ✗ /v1/models error: {}", e));
            failures += 1;
        }
    }

    (lines, failures)
}

/// Test connection to backend
async fn test_backend(config_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config_or_exit(&config_path);

    let mut probes = 0usize;
    let mut failures = 0usize;
    // Collect all nodes from all groups (or single backend)
    if let Some(ref backends) = config.backends {
        let total_nodes: usize = backends.values().map(|g| g.nodes.len()).sum();
        println!(
            "Testing {} backend node(s) across {} group(s)...\n",
            total_nodes,
            backends.len()
        );

        for (group_name, group) in backends {
            for node_cfg in &group.nodes {
                let node = BackendNode::from_config(
                    node_cfg.url.clone(),
                    5, // short timeout for test
                    node_cfg.tls.as_ref(),
                    node_cfg.model.clone(),
                    node_cfg.api_key.clone(),
                    node_cfg.strip_path_prefix.clone(),
                    node_cfg.temperature,
                )?;

                println!("[{}]: {}", group_name, node.base_url());
                let (report, failed) = probe_one_node(&node).await;
                probes += 2;
                failures += failed;
                for line in report {
                    println!("{line}");
                }

                println!();
            }
        }
    } else if let Some(ref backend) = config.backend {
        // Single backend mode
        println!("Testing single backend...\n");

        let node = BackendNode::from_config(
            backend.url.clone(),
            5, // short timeout for test
            backend.tls.as_ref(),
            backend.model.clone(),
            backend.api_key.clone(),
            backend.strip_path_prefix.clone(),
            None, // single-backend mode has no temperature override
        )?;

        println!("[single]: {}", node.base_url());
        let (report, failed) = probe_one_node(&node).await;
        probes += 2;
        failures += failed;
        for line in report {
            println!("{line}");
        }
    } else {
        println!("No backend configured — nothing to test.");
    }

    if failures > 0 {
        return Err(format!("test-backend: {failures} of {probes} probe(s) failed").into());
    }
    if probes > 0 {
        println!("✓ all {probes} probe(s) passed");
    }
    Ok(())
}

/// Validate the merged (file + CLI) config just before startup. The loader
/// already validated the file; this covers only what CLI overrides can change:
/// port, backend URL (single mode), and — as a cheap belt at the last gate —
/// the augment URL. `--port 0` is rejected with an explicit message: binding
/// an OS-ephemeral port is out of scope.
fn validate_final(cfg: &AppConfig) -> Result<(), String> {
    if cfg.server.port == 0 {
        return Err(
            "server port 0 is not supported (ephemeral-port bind is out of scope): pick a fixed port 1-65535".to_string(),
        );
    }
    if let Some(backend) = cfg.backend.as_ref() {
        if backend.url.trim().is_empty() {
            return Err("backend url must not be empty".to_string());
        }
        llama_proxy::config::validate_http_url(&backend.url, "Backend").map_err(|e| e.to_string())?;
    }
    if let Some(augment) = cfg.augment_backend.as_ref() {
        llama_proxy::config::validate_http_url(&augment.url, "Augment backend").map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Decide the log filter and whether the user deserves to know why it is not
/// what they asked for. Precedence: --log-level > RUST_LOG > "info". A set-but
/// unparseable RUST_LOG falls back to "info" WITH a warning (the old silent
/// `unwrap_or_else` made a typo'd filter look like the default); unset stays
/// silent. Returns (filter, optional one-line stderr warning).
fn resolve_log_filter(
    cli_level: Option<String>,
    env_rust_log: Option<String>,
) -> (tracing_subscriber::EnvFilter, Option<String>) {
    if let Some(level) = cli_level {
        return (tracing_subscriber::EnvFilter::new(level), None);
    }
    match env_rust_log {
        None => (tracing_subscriber::EnvFilter::new("info"), None),
        Some(raw) => match tracing_subscriber::EnvFilter::try_new(&raw) {
            Ok(filter) => (filter, None),
            Err(e) => (
                tracing_subscriber::EnvFilter::new("info"),
                Some(format!(
                    "warning: RUST_LOG={raw:?} is not a valid filter ({e}); falling back to \"info\" (fix RUST_LOG or use --log-level)"
                )),
            ),
        },
    }
}

/// Load configuration or exit with error
fn load_config_or_exit(config_path: &Path) -> AppConfig {
    tracing::info!("Loading configuration from {:?}", config_path);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fix_module_states_reports_config_disabled_module() {
        // Given: a config that disables exactly one module
        let mut modules = std::collections::HashMap::new();
        modules.insert(
            "toolcall_bad_filepath".to_string(),
            llama_proxy::config::FixModuleConfig {
                enabled: false,
                options: std::collections::HashMap::new(),
            },
        );
        let fixes = FixesConfig { enabled: true, modules };

        // When: rows are derived from the config-constructed registry
        let catalog = create_default_registry();
        let states = fix_module_states(&catalog, &create_registry_from_config(&fixes));

        // Then: the disabled module reports false, the others true - all rows present
        assert_eq!(states.len(), 3);
        let enabled_of = |name: &str| states.iter().find(|s| s.name == name).map(|s| s.enabled);
        assert_eq!(enabled_of("toolcall_bad_filepath"), Some(false));
        assert_eq!(enabled_of("toolcall_null_index_fix"), Some(true));
        assert_eq!(enabled_of("toolcall_malformed_arguments"), Some(true));
    }

    #[test]
    fn test_fix_module_states_default_config_enables_everything() {
        // Given: no config file at all -> FixesConfig::default()
        // When/Then: every known module row is enabled (the no-config print)
        let catalog = create_default_registry();
        let states = fix_module_states(&catalog, &create_registry_from_config(&FixesConfig::default()));
        assert_eq!(states.len(), 3);
        assert!(states.iter().all(|s| s.enabled));
    }

    #[test]
    fn test_fix_module_states_global_switch_disables_every_row() {
        // Given: fixes.enabled=false globally, a module asking to be enabled
        let mut modules = std::collections::HashMap::new();
        modules.insert(
            "toolcall_bad_filepath".to_string(),
            llama_proxy::config::FixModuleConfig {
                enabled: true,
                options: std::collections::HashMap::new(),
            },
        );
        let fixes = FixesConfig { enabled: false, modules };

        // When/Then: no row is enabled
        let catalog = create_default_registry();
        let states = fix_module_states(&catalog, &create_registry_from_config(&fixes));
        assert_eq!(states.len(), 3);
        assert!(states.iter().all(|s| !s.enabled));
    }

    fn cfg(yaml: &str) -> llama_proxy::config::AppConfig {
        serde_yaml::from_str(yaml).expect("test fixture must deserialize")
    }

    const VALID: &str = "server:\n  port: 8066\n  host: \"0.0.0.0\"\nbackend:\n  url: \"http://localhost:8080\"\n";

    #[test]
    fn t72_validate_final_rejects_port_zero_with_explicit_message() {
        let c = cfg(&VALID.replace("port: 8066", "port: 0"));
        let err = validate_final(&c).expect_err("--port 0 must be rejected");
        assert!(err.contains("port 0") && err.contains("ephemeral"), "got {err}");
    }

    #[test]
    fn t72_validate_final_rejects_unparseable_or_empty_backend_url() {
        for bad in ["ftp://localhost:8080", "not-a-url", "", "   "] {
            let c = cfg(&VALID.replace("url: \"http://localhost:8080\"", &format!("url: \"{bad}\"")));
            assert!(validate_final(&c).is_err(), "url '{bad}' must be rejected");
        }
    }

    #[test]
    fn t72_validate_final_rejects_invalid_augment_url() {
        let c = cfg(&format!(
            "{VALID}augment-backend:\n  enabled: true\n  url: \"ftp://nope\"\n  model: \"f\"\n"
        ));
        let err = validate_final(&c).expect_err("ftp augment url must be rejected");
        assert!(err.contains("Augment"), "got {err}");
    }

    #[test]
    fn t72_validate_final_accepts_valid_and_backendless_configs() {
        assert!(validate_final(&cfg(VALID)).is_ok());
        assert!(
            validate_final(&cfg("server:\n  port: 8066\n  host: \"0.0.0.0\"\n")).is_ok(),
            "the F-H1 backendless (503-path) config must stay valid"
        );
    }

    #[test]
    fn f11b2_gate_stays_quiet_when_exporter_disabled() {
        // Given: exporters.influxdb.enabled: false — When: gate consulted
        // Then: no error in ANY feature set (WARN-when-disabled path stays)
        assert!(influxdb_startup_gate(false, Path::new("cfg.yaml")).is_none());
    }

    #[cfg(not(feature = "influxdb"))]
    #[test]
    fn f11b2_gate_hard_errors_when_enabled_without_feature() {
        // plan:27 (D3): enabled + feature compiled out = startup hard error.
        let msg = influxdb_startup_gate(true, Path::new("/tmp/cfg.yaml"))
            .expect("enabled without the influxdb feature must produce the startup error");
        assert!(msg.contains("influxdb"), "must name the missing feature: {msg}");
        assert!(msg.contains("--features influxdb"), "must give the rebuild hint: {msg}");
        assert!(msg.contains("enabled: false"), "must give the config hint: {msg}");
        assert!(msg.contains("/tmp/cfg.yaml"), "must name the config file: {msg}");
    }

    #[cfg(feature = "influxdb")]
    #[test]
    fn f11b2_gate_never_fires_when_feature_compiled_in() {
        // Scope lock: with the feature built in, `run` behaves exactly as before.
        assert!(influxdb_startup_gate(true, Path::new("cfg.yaml")).is_none());
    }

    #[test]
    fn t75_models_probe_accepts_a_real_model_list() {
        // Given: a genuine OpenAI model-list body
        let body = r#"{"object":"list","data":[{"id":"qwen3"},{"id":"llama"}]}"#;
        // When
        let (ok, details) = models_probe_body_outcome(body);
        // Then
        assert!(ok, "valid data[] must pass the probe");
        assert!(details.iter().any(|l| l.contains("Available models: 2")), "{details:?}");
        assert!(details.iter().any(|l| l.contains("qwen3")), "{details:?}");
    }

    #[test]
    fn t75_models_probe_refuses_200_bodies_that_are_not_model_lists() {
        // The dishonest case: status was 200 but the payload proves nothing -
        // HTML, an error envelope, and an empty body must all FAIL the probe
        // and surface what actually came back.
        for body in ["<html><body>OK</body></html>", r#"{"error":"unauthorized"}"#, ""] {
            let (ok, details) = models_probe_body_outcome(body);
            assert!(!ok, "body {body:?} is not a model list and must fail");
            assert!(!details.is_empty(), "failure must show what came back instead");
        }
    }

    #[test]
    fn t75_cli_version_follows_the_crate_not_a_frozen_literal() {
        use clap::CommandFactory;
        assert_eq!(
            Cli::command().get_version().map(str::to_string),
            Some(env!("CARGO_PKG_VERSION").to_string()),
            "--version must report the crate version, never a hardcoded string"
        );
    }

    #[test]
    fn t76_cli_log_level_beats_env_and_nags_nothing() {
        let (filter, warn) = resolve_log_filter(Some("debug".into()), Some("!!!".into()));
        assert_eq!(filter.to_string(), "debug", "--log-level must win outright");
        assert!(warn.is_none(), "an overridden RUST_LOG is not our business: {warn:?}");
    }

    #[test]
    fn t76_valid_rust_log_is_honored_silently() {
        let (filter, warn) = resolve_log_filter(None, Some("warn,llama_proxy=trace".into()));
        assert_eq!(filter.to_string(), "llama_proxy=trace,warn");
        assert!(warn.is_none());
    }

    #[test]
    fn t76_invalid_rust_log_falls_back_to_info_with_one_warning_line() {
        // The RED case: RUST_LOG='!!!' used to degrade to info SILENTLY,
        // making a typo'd filter indistinguishable from the default.
        let (filter, warn) = resolve_log_filter(None, Some("!!!".into()));
        assert_eq!(filter.to_string(), "info", "fallback must be info");
        let w = warn.expect("invalid RUST_LOG must warn");
        assert!(w.contains("\"!!!\"") && w.to_lowercase().starts_with("warning"), "{w}");
        assert!(!w.contains('\n'), "warning must be exactly one line: {w:?}");
    }

    #[test]
    fn t76_unset_rust_log_is_silent_info() {
        let (filter, warn) = resolve_log_filter(None, None);
        assert_eq!(filter.to_string(), "info");
        assert!(warn.is_none(), "unset RUST_LOG must not nag");
    }

    #[test]
    fn t77_summary_names_the_whole_effective_config() {
        let text = render_config_summary(&cfg(VALID)).join("\n");
        for needle in [
            "Server: ",
            "Backend: single",
            "Fixes: ",
            "Streaming: ",
            "Stats: ",
            "InfluxDB exporter: ",
        ] {
            assert!(text.contains(needle), "missing {needle:?}:\n{text}");
        }
        assert!(text.contains("http://localhost:8080"), "backend url must appear:\n{text}");
    }

    #[test]
    fn t77_summary_backendless_names_the_503_path() {
        let text = render_config_summary(&cfg("server:\n  port: 8066\n  host: \"0.0.0.0\"\n")).join("\n");
        assert!(text.contains("none configured") && text.contains("503"), "{text}");
    }

    #[test]
    fn t77_summary_lists_every_group_and_node() {
        // Given: multi-group config with two nodes (one with a trailing slash)
        let yaml = "server:\n  port: 8066\n  host: \"0.0.0.0\"\nbackends:\n  g1:\n    mappings: [\"qwen3\"]\n    strategy: round_robin\n    nodes:\n      - url: \"http://a:8080\"\n      - url: \"http://b:8080/\"\n";
        let text = render_config_summary(&cfg(yaml)).join("\n");
        // Then: both nodes named, single-mode wording absent
        assert!(text.contains("Group [g1]"), "{text}");
        assert!(
            text.contains("http://a:8080") && text.contains("http://b:8080"),
            "both node urls:\n{text}"
        );
        assert!(!text.contains("Backend: single"), "multi-group must not render single mode");
        assert!(!text.contains("b:8080/"), "node urls render without trailing slash:\n{text}");
    }
}
