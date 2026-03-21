//! llama-proxy e2e test runner
//!
//! Default (no args): builds proxy if needed, spawns it, runs all tests, kills it.
//!
//!   cargo run                          # auto-detect proxy binary, run all tests
//!   cargo run -- list                  # list all tests
//!   cargo run -- run                   # connect to already-running proxy
//!   cargo run -- spawn-and-run [opts]  # explicit paths / ports

mod backend;
mod client;
mod runner;
mod tests;
mod types;

use clap::{Parser, Subcommand};
use colored::Colorize;
use runner::{list_tests, run_tests, TestContext};
use tests::all_tests;

/// Default proxy binary candidates, tried in order
const DEFAULT_PROXY_BINS: &[&str] = &["../target/release/llama-proxy", "../target/debug/llama-proxy"];

const DEFAULT_PROXY_CONFIG: &str = "test_configs/proxy_fixes_on.yaml";
const DEFAULT_BACKEND_PORT: u16 = 18080;
const DEFAULT_PROXY_PORT: u16 = 18066;

#[derive(Parser)]
#[command(
    name = "e2e",
    about = "End-to-end tests for llama-proxy",
    long_about = "Runs all e2e tests by default (no arguments needed).\n\
                  Spawns the proxy binary automatically, runs tests, then kills it."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Only run tests whose name contains this string (applies to default run)
    #[arg(long, short, global = true)]
    filter: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Connect to an already-running proxy and run tests
    Run {
        /// Address of the real proxy
        #[arg(long, default_value = "127.0.0.1:18066")]
        proxy_addr: String,

        /// Port for the mock backend - must not conflict with real services
        #[arg(long, default_value_t = DEFAULT_BACKEND_PORT)]
        backend_port: u16,

        /// Only run tests whose name contains this string
        #[arg(long, short)]
        filter: Option<String>,
    },

    /// List all available tests
    List,

    /// Spawn the proxy binary, run all tests, then kill it
    SpawnAndRun {
        /// Path to the llama-proxy binary
        #[arg(long)]
        proxy_bin: Option<String>,

        /// Path to the proxy config YAML (backend must point at mock backend port)
        #[arg(long, default_value = DEFAULT_PROXY_CONFIG)]
        proxy_config: String,

        /// Port for the mock backend - must match config
        #[arg(long, default_value_t = DEFAULT_BACKEND_PORT)]
        backend_port: u16,

        /// Proxy listen port - must match config
        #[arg(long, default_value_t = DEFAULT_PROXY_PORT)]
        proxy_port: u16,

        /// Only run tests whose name contains this string
        #[arg(long, short)]
        filter: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        // ── No subcommand: default full run ──────────────────────────────────
        None => {
            let proxy_bin = find_proxy_bin()?;
            do_spawn_and_run(
                proxy_bin,
                DEFAULT_PROXY_CONFIG.to_string(),
                DEFAULT_BACKEND_PORT,
                DEFAULT_PROXY_PORT,
                cli.filter,
            )
            .await?;
        }

        // ── list ─────────────────────────────────────────────────────────────
        Some(Command::List) => {
            list_tests(&all_tests());
        }

        // ── run (connect to existing proxy) ───────────────────────────────────
        Some(Command::Run {
            proxy_addr,
            backend_port,
            filter,
        }) => {
            let filter = filter.or(cli.filter);
            println!("Starting mock backend on port {}...", backend_port);
            let backend_state = backend::start(backend_port).await?;
            println!("Mock backend running on 127.0.0.1:{}", backend_port);

            let ctx = TestContext {
                proxy_addr,
                backend_state,
                http_client: client::build_client(),
            };

            let results = run_tests(all_tests(), ctx, filter.as_deref()).await;
            exit_on_failure(&results);
        }

        // ── spawn-and-run ─────────────────────────────────────────────────────
        Some(Command::SpawnAndRun {
            proxy_bin,
            proxy_config,
            backend_port,
            proxy_port,
            filter,
        }) => {
            let filter = filter.or(cli.filter);
            let proxy_bin = match proxy_bin {
                Some(p) => p,
                None => find_proxy_bin()?,
            };
            do_spawn_and_run(proxy_bin, proxy_config, backend_port, proxy_port, filter).await?;
        }
    }

    Ok(())
}

/// Shared implementation for spawn-and-run (used by both default and explicit subcommand)
async fn do_spawn_and_run(
    proxy_bin: String,
    proxy_config: String,
    backend_port: u16,
    proxy_port: u16,
    filter: Option<String>,
) -> anyhow::Result<()> {
    println!("Starting mock backend on port {}...", backend_port);
    let backend_state = backend::start(backend_port).await?;
    println!("Mock backend running on 127.0.0.1:{}", backend_port);

    println!("Spawning proxy: {} run --config {}", proxy_bin, proxy_config);
    let mut proxy_process = tokio::process::Command::new(&proxy_bin)
        .arg("run")
        .arg("--config")
        .arg(&proxy_config)
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn '{}': {}", proxy_bin, e))?;

    let proxy_addr = format!("127.0.0.1:{}", proxy_port);
    println!("Waiting for proxy at {}...", proxy_addr);
    wait_for_proxy(&proxy_addr).await?;
    println!("Proxy is ready!\n");

    let ctx = TestContext {
        proxy_addr,
        backend_state,
        http_client: client::build_client(),
    };

    let results = run_tests(all_tests(), ctx, filter.as_deref()).await;

    proxy_process.kill().await.ok();

    exit_on_failure(&results);
    Ok(())
}

/// Find the proxy binary, trying release then debug builds
fn find_proxy_bin() -> anyhow::Result<String> {
    for candidate in DEFAULT_PROXY_BINS {
        if std::path::Path::new(candidate).exists() {
            println!("Using proxy binary: {}", candidate.bright_cyan());
            return Ok(candidate.to_string());
        }
    }
    Err(anyhow::anyhow!(
        "No proxy binary found. Tried: {}\nBuild with: cd .. && cargo build --release",
        DEFAULT_PROXY_BINS.join(", ")
    ))
}

/// Exit with code 1 if any tests failed
fn exit_on_failure(results: &[crate::types::TestResult]) {
    let failed = results.iter().filter(|r| !r.passed).count();
    if failed > 0 {
        std::process::exit(1);
    }
}

/// Wait for the proxy to start accepting connections (retry with backoff)
async fn wait_for_proxy(addr: &str) -> anyhow::Result<()> {
    let client = client::build_client();
    let health_url = format!("http://{}/health", addr);

    for attempt in 0..30 {
        tokio::time::sleep(tokio::time::Duration::from_millis(200 + attempt * 100)).await;
        if client.get(&health_url).send().await.is_ok() {
            return Ok(());
        }
    }

    Err(anyhow::anyhow!(
        "Proxy did not start within timeout. Is the binary correct? Check: {}",
        addr
    ))
}
