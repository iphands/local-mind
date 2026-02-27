//! llama-proxy: HTTP reverse proxy for llama.cpp server
//!
//! Features:
//! - OpenAI-compatible API proxying
//! - Response fix modules for malformed tool calls
//! - Stats logging with tokens per second metrics
//! - Pluggable exporters (InfluxDB, etc.)

pub mod api;
pub mod backends;
pub mod config;
pub mod exporters;
pub mod fixes;
pub mod proxy;
pub mod stats;

pub use config::AppConfig;
pub use fixes::{create_default_registry, FixRegistry};
pub use proxy::run_server;
