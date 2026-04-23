# llama-proxy

[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

An HTTP reverse proxy for [llama.cpp](https://github.com/ggerganov/llama.cpp) server that fixes malformed LLM responses, collects performance metrics, and exports telemetry to external systems.

## Features

### For Users

- **Response Fixing**: Automatically repairs malformed tool calls and other LLM output issues
  - `toolcall_null_index`: Fixes null tool call indices (required by strict clients)
  - `toolcall_bad_filepath`: Removes duplicate `filePath` keys in tool call arguments
  - `toolcall_malformed_arguments`: Repairs malformed JSON argument patterns
  - Pluggable fix system - enable/disable fixes per your needs

- **Performance Metrics**: Real-time monitoring of LLM performance
  - Tokens per second (prompt processing and generation)
  - Context window usage tracking
  - Request duration and timing breakdowns
  - Extended token metrics (reasoning tokens, prediction acceptance)

- **Flexible Output Formats**: View metrics in your preferred format
  - `pretty`: Beautiful terminal-formatted boxes
  - `json`: Structured logging for tools like `jq`
  - `compact`: Single-line format for log aggregation

- **Remote Telemetry Export**: Send metrics to external systems
  - InfluxDB v2 support with batching
  - Extensible exporter architecture for other backends

- **Multi-Backend Load Balancing**: Route requests across multiple backends
  - Model-based routing: map model names to backend groups
  - Strategies: `round_robin` (cycle through) or `priority_free` (least-busy wins)
  - Per-node model override, API key, and TLS configuration
  - Catch-all group for unmatched models

- **Streaming Synthesis**: Improved streaming reliability
  - `fake` mode: fetches complete JSON from backend, synthesizes SSE for clients
  - Eliminates delta calculation complexity and streaming fix edge cases
  - Tool calls delivered as single complete chunks

- **Request Augmentation** (experimental): Enrich requests before forwarding
  - Calls a fast LLM backend to generate additional context
  - Injects context into user messages transparently

- **Client Compatibility**: Works seamlessly with AI coding tools
  - Full OpenAI Chat Completions API support
  - Claude Code CLI/TUI compatibility
  - Opencode CLI/TUI compatibility
  - Streaming (SSE) and non-streaming modes
  - Preserves all client-specific extensions

## Quick Start

### Installation

```bash
# Clone the repository
git clone <repo-url>
cd llama-proxy

# Build the project
cargo build --release
```

### Configuration

```bash
# Copy the default config
cp config.yaml.default config.yaml

# Edit config.yaml with your settings
nano config.yaml
```

Key configuration sections:

```yaml
# Proxy server settings
server:
  host: "0.0.0.0"
  port: 8066

# Single backend llama-server location
backend:
  url: "http://localhost:8080"
  timeout_seconds: 300
  # Optional TLS settings for https:// backends
  # tls:
  #   accept_invalid_certs: false
  #   ca_cert_path: "/path/to/ca.pem"

# Multi-backend load balancing (overrides 'backend' when present)
# backends:
#   local:
#     mappings: ["qwen3", "llama"]  # empty = catch-all
#     strategy: priority_free       # or round_robin
#     nodes:
#       - url: "http://localhost:8080"
#         timeout_seconds: 300
#   remote:
#     mappings: []  # catch-all
#     strategy: round_robin
#     nodes:
#       - url: "http://remote-host:8080"
#         api_key: "sk-xxx"
#         model: "gpt-4o"           # override model name sent to backend

# Enable/disable response fixes
fixes:
  enabled: true
  modules:
    toolcall_null_index:
      enabled: true
    toolcall_bad_filepath:
      enabled: true
    toolcall_malformed_arguments:
      enabled: true

# Streaming mode (default: fake)
#   fake: request non-streaming from backend, synthesize SSE for client
#   disabled: force streaming off completely
# streaming: fake

# Metrics logging
stats:
  enabled: true
  format: pretty  # pretty | json | compact
  log_interval: 1  # log every N requests

# Remote exporters
exporters:
  influxdb:
    enabled: false
    url: "http://localhost:8086"
    org: "my-org"
    bucket: "llm-metrics"
    token: "your-token-here"

# Augment backend - enriches requests with additional context before forwarding (experimental)
# augment-backend:
#   enabled: true
#   url: "http://localhost:8701"
#   model: "fast-model"
#   prompt_file: "./augmenter/backend_prompt.md"
#   request_prompt_file: "./augmenter/request_prompt.md"
```

### Running the Proxy

```bash
# Start the proxy server
cargo run --release -- run --config config.yaml

# Or with explicit log level
cargo run -- run --config config.yaml --log-level debug

# Override port or backend from CLI
cargo run -- run --config config.yaml --port 8066
cargo run -- run --config config.yaml --backend-url http://other-host:8080

# Override streaming mode
cargo run -- run --config config.yaml --streaming-mode fake

# Dump request/response pairs for debugging
cargo run -- run --config config.yaml --dump ./debug-dumps
```

### Usage with Clients

Point your AI coding tool at the proxy instead of llama.cpp directly:

```bash
# Instead of: http://localhost:8080
# Use:        http://localhost:8066
```

The proxy maintains full API compatibility while adding fixes and metrics.

## CLI Commands

```bash
# Start the server
llama-proxy run --config config.yaml

# List available response fix modules
llama-proxy list-fixes
llama-proxy list-fixes --verbose

# Validate configuration file
llama-proxy check-config --config config.yaml

# Test backend connection
llama-proxy test-backend --config config.yaml

# Override settings from CLI
llama-proxy run --port 8066 --backend-url http://localhost:8080
llama-proxy run --log-level debug
llama-proxy run --streaming-mode fake
llama-proxy run --hide-requests        # suppress per-request log lines
llama-proxy run --dump ./debug-dumps   # dump request/response pairs
```

## Example Metrics Output

### Pretty Format
```
┌──────────────────────────────────────────────────────────────────┐
│ LLM Request Metrics                                              │
├──────────────────────────────────────────────────────────────────┤
│ Model: Qwen3-14B-128K-Q3_K_S.gguf                                │
│ Time:  2026-02-15 10:30:45 UTC                                   │
├──────────────────────────────────────────────────────────────────┤
│ Performance                                                      │
│   Prompt Processing:  1698.07 tokens/sec (  316.8ms)             │
│   Generation:           33.13 tokens/sec (29669.4ms)             │
├──────────────────────────────────────────────────────────────────┤
│ Tokens                                                           │
│   Input:    538 │ Output:    983 │ Total:   1521                 │
├──────────────────────────────────────────────────────────────────┤
│ Context: 538/4096 (13.1%)                                        │
│ Finish: stop                                                     │
│ Duration: 30181.0ms                                              │
└──────────────────────────────────────────────────────────────────┘
```

### JSON Format
```json
{
  "request_id": "a1b2c3d4-...",
  "timestamp": "2026-02-15T10:30:45Z",
  "model": "Qwen3-14B-128K-Q3_K_S.gguf",
  "prompt_tokens": 538,
  "completion_tokens": 983,
  "total_tokens": 1521,
  "prompt_tps": 1698.07,
  "generation_tps": 33.13,
  "context_total": 4096,
  "context_used": 538,
  "context_percent": 13.1,
  "streaming": true,
  "finish_reason": "stop",
  "duration_ms": 30181.0
}
```

### Compact Format
```
model=Qwen3-14B toks=538/983 tps=1698.07/33.13 ctx:=38/4096 stream finish=stop dur=30181.0ms
```

---

## Developer Guide

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                        Client                               │
│              (Claude Code, Opencode, curl)                  │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                     llama-proxy                             │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Handler: Routes requests                              │ │
│  │  - Pass-through: /props, /slots, /health, /v1/models   │ │
│  │  - Fix + Stats: /v1/chat/completions                   │ │
│  └────────────────────────────────────────────────────────┘ │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Augment Backend (optional): Enrich requests           │ │
│  │  - Calls fast LLM to generate context                  │ │
│  │  - Injects context into user messages                  │ │
│  └────────────────────────────────────────────────────────┘ │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Fix Registry: Apply response fixes                    │ │
│  │  - toolcall_null_index (fix null indices)              │ │
│  │  - toolcall_bad_filepath (remove duplicate keys)       │ │
│  │  - toolcall_malformed_arguments (fix broken JSON args) │ │
│  └────────────────────────────────────────────────────────┘ │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Streaming Synthesis: SSE from complete JSON           │ │
│  │  - Eliminates streaming delta complexity               │ │
│  │  - Tool calls sent as single complete chunks           │ │
│  └────────────────────────────────────────────────────────┘ │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Stats Collector: Gather metrics                       │ │
│  │  - Token counts, TPS, timing, context usage            │ │
│  │  - Extended metrics (reasoning, predictions)           │ │
│  └────────────────────────────────────────────────────────┘ │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Exporters: Send metrics to external systems           │ │
│  │  - InfluxDB (with batching)                            │ │
│  └────────────────────────────────────────────────────────┘ │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│           Load Balancer (single or multi-backend)           │
│  GroupedLoadBalancer → model routing → RoundRobin /         │
│                                        PriorityFree         │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│          llama.cpp server / OpenAI-compatible API           │
└─────────────────────────────────────────────────────────────┘
```

### Project Structure

```
src/
├── main.rs              # CLI and application entry point
├── lib.rs               # Library exports
├── augment.rs           # Request augmentation backend client
├── config/              # Configuration loading and types
│   ├── mod.rs           # AppConfig, ServerConfig, BackendConfig, etc.
│   └── loader.rs        # YAML config file parsing
├── proxy/               # HTTP proxy server
│   ├── server.rs        # Axum server setup, ProxyState
│   ├── handler.rs       # Request routing and response handling
│   ├── streaming.rs     # SSE stream processing (legacy fallback path)
│   ├── synthesis.rs     # SSE synthesis from complete JSON (fake streaming)
│   └── context.rs       # Context fetching from /slots endpoint
├── backends/            # Multi-backend load balancing
│   ├── mod.rs           # Builder functions
│   ├── balancer.rs      # LoadBalancer trait and BackendGuard
│   ├── node.rs          # BackendNode (URL, TLS, model override, etc.)
│   ├── grouped.rs       # GroupedLoadBalancer (model-based routing)
│   ├── round_robin.rs   # RoundRobin strategy
│   ├── priority_free.rs # PriorityFree strategy (least-busy node)
│   └── preflight.rs     # Backend preflight/health checks
├── fixes/               # Pluggable response fix system
│   ├── mod.rs           # ResponseFix trait, ToolCallAccumulator
│   ├── registry.rs      # Fix registration and management
│   ├── toolcall_null_index_fix.rs        # Fix null tool call indices
│   ├── toolcall_bad_filepath_fix.rs      # Fix duplicate filePath keys
│   └── toolcall_malformed_arguments_fix.rs  # Fix malformed JSON args
├── stats/               # Metrics collection and formatting
│   ├── collector.rs     # RequestMetrics extraction from responses
│   ├── formatter.rs     # pretty/json/compact output formatting
│   └── request_log.rs   # Request logging utilities
├── exporters/           # Remote metrics export
│   ├── mod.rs           # MetricsExporter trait, ExporterManager
│   └── influxdb.rs      # InfluxDB v2 exporter with batching
└── api/                 # Type definitions
    ├── openai.rs        # OpenAI API types (with Opencode extensions)
    └── llama.rs         # llama.cpp specific types
```

### Adding a New Response Fix

Response fixes implement the `ResponseFix` trait and are managed by the `FixRegistry`.

#### 1. Create Your Fix Module

Create `src/fixes/my_custom_fix.rs`:

```rust
use super::ResponseFix;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};

/// Fix for [describe what your fix does]
pub struct MyCustomFix {
    enabled: AtomicBool,
}

impl MyCustomFix {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }
}

#[async_trait]
impl ResponseFix for MyCustomFix {
    fn name(&self) -> &str {
        "my_custom_fix"
    }

    fn description(&self) -> &str {
        "Fixes [specific issue] in LLM responses"
    }

    fn applies(&self, response: &Value) -> bool {
        // Return true if this fix should apply to the response
        response.get("choices")
            .and_then(|c| c.as_array())
            .map(|arr| !arr.is_empty())
            .unwrap_or(false)
    }

    fn apply(&self, mut response: Value) -> (Value, FixAction) {
        // Apply fix to complete response (primary method)
        // Return (modified_response, FixAction) for standardized logging

        tracing::debug!("Applying my_custom_fix");

        if let Some(choices) = response.get_mut("choices") {
            // ... your fix logic here ...
            return (response, FixAction::fixed("original snippet", "fixed snippet"));
        }

        (response, FixAction::NotApplicable)
    }
}
```

#### 2. Register Your Fix

Add to `src/fixes/mod.rs`:

```rust
mod my_custom_fix;
pub use my_custom_fix::MyCustomFix;

pub fn create_default_registry() -> FixRegistry {
    let mut registry = FixRegistry::new();
    registry.register(Arc::new(ToolcallBadFilepathFix::new()));
    registry.register(Arc::new(MyCustomFix::new(true)));  // Add your fix
    registry
}
```

#### 3. Add Configuration Support

Update `config.yaml.default`:

```yaml
fixes:
  enabled: true
  modules:
    my_custom_fix:
      enabled: true
      # Add any custom options your fix needs
      option1: value1
```

Update the `configure()` method in `src/fixes/registry.rs` to handle your fix's options:

```rust
pub fn configure(&mut self, config: &HashMap<String, FixModuleConfig>) {
    for (name, module_config) in config {
        if let Some(fix) = self.fixes.iter().find(|f| f.name() == name) {
            self.enabled.insert(name.clone(), module_config.enabled);

            // Apply fix-specific options
            if name == "my_custom_fix" {
                if let Some(casted) = Arc::clone(fix)
                    .as_any()
                    .downcast_ref::<MyCustomFix>()
                {
                    if let Some(opt) = module_config
                        .options
                        .get("option1")
                        .and_then(|v| v.as_bool())
                    {
                        casted.set_option1(opt);
                    }
                }
            }
        }
    }
}
```

#### 4. Test Your Fix

```bash
# Unit tests
cargo test my_custom_fix

# Integration test
cargo run -- run --config config.yaml

# Test with real request
curl -X POST http://localhost:8066/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"test","messages":[{"role":"user","content":"test"}]}'
```

### Adding a New Metrics Exporter

Exporters implement the `MetricsExporter` trait and run asynchronously after each request.

#### 1. Create Your Exporter

Create `src/exporters/my_exporter.rs`:

```rust
use super::{ExportError, MetricsExporter};
use crate::stats::RequestMetrics;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct MyExporterConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub api_key: String,
    // Add your config fields
}

pub struct MyExporter {
    config: MyExporterConfig,
    client: reqwest::Client,
}

impl MyExporter {
    pub fn new(config: MyExporterConfig) -> Result<Self, ExportError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| ExportError::Config(e.to_string()))?;

        Ok(Self { config, client })
    }

    pub fn from_config(config: &MyExporterConfig) -> Result<Self, ExportError> {
        Self::new(config.clone())
    }
}

#[async_trait]
impl MetricsExporter for MyExporter {
    async fn export(&self, metrics: &RequestMetrics) -> Result<(), ExportError> {
        // Convert metrics to your format
        let payload = serde_json::json!({
            "timestamp": metrics.timestamp,
            "model": metrics.model,
            "tokens": {
                "prompt": metrics.prompt_tokens,
                "completion": metrics.completion_tokens,
            },
            "performance": {
                "prompt_tps": metrics.prompt_tps,
                "generation_tps": metrics.generation_tps,
            }
        });

        // Send to your backend
        self.client
            .post(&self.config.endpoint)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&payload)
            .send()
            .await
            .map_err(|e| ExportError::Write(e.to_string()))?;

        Ok(())
    }

    fn name(&self) -> &str {
        "my_exporter"
    }
}
```

#### 2. Register Your Exporter

Add to `src/exporters/mod.rs`:

```rust
mod my_exporter;
pub use my_exporter::{MyExporter, MyExporterConfig};
```

Update `src/config/mod.rs`:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExportersConfig {
    pub influxdb: InfluxDbConfig,
    pub my_exporter: MyExporterConfig,  // Add your config
}
```

Update `src/main.rs` in the `run_proxy()` function:

```rust
// Add MyExporter if enabled
if config.exporters.my_exporter.enabled {
    match MyExporter::from_config(&config.exporters.my_exporter) {
        Ok(exporter) => {
            exporter_manager.add(Arc::new(exporter));
            tracing::info!("MyExporter enabled");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to initialize MyExporter");
        }
    }
}
```

#### 3. Add Configuration

Update `config.yaml.default`:

```yaml
exporters:
  my_exporter:
    enabled: false
    endpoint: "https://api.example.com/metrics"
    api_key: "your-api-key"
```

### Adding a New Stats Formatter

Stats formatters control how metrics are displayed. They're defined in `src/stats/formatter.rs`.

#### 1. Add Your Format to the Enum

Edit `src/config/mod.rs`:

```rust
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StatsFormat {
    #[default]
    Pretty,
    Json,
    Compact,
    Csv,  // Your new format
}
```

#### 2. Implement the Formatter

Edit `src/stats/formatter.rs`:

```rust
pub fn format_metrics(metrics: &RequestMetrics, format: StatsFormat) -> String {
    match format {
        StatsFormat::Pretty => format_pretty(metrics),
        StatsFormat::Json => format_json(metrics),
        StatsFormat::Compact => format_compact(metrics),
        StatsFormat::Csv => format_csv(metrics),  // Add your formatter
    }
}

fn format_csv(m: &RequestMetrics) -> String {
    format!(
        "{},{},{},{},{},{},{},{}",
        m.timestamp.to_rfc3339(),
        m.model,
        m.prompt_tokens,
        m.completion_tokens,
        m.prompt_tps,
        m.generation_tps,
        m.finish_reason,
        m.duration_ms
    )
}
```

#### 3. Test Your Formatter

```bash
# Set format in config.yaml
stats:
  format: csv

# Run and verify output
cargo run -- run --config config.yaml
```

### Testing

```bash
# Run all tests
cargo test

# Run tests for a specific module
cargo test fixes::
cargo test stats::
cargo test exporters::

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_request_metrics_from_response
```

### Key Design Patterns

#### Registry Pattern
The `FixRegistry` stores `Arc<dyn ResponseFix>` allowing dynamic enable/disable without recompilation. Fixes are checked with `applies()` before calling `apply()`.

#### Streaming vs Non-Streaming
The handler detects streaming via `Content-Type: text/event-stream` and routes to specialized streaming handler that applies fixes per SSE chunk.

#### Async Architecture
- Uses tokio runtime for async I/O
- reqwest for HTTP client
- axum for HTTP server
- Exporters spawn background tasks to avoid blocking responses

#### Transparent Pass-Through
Unknown fields in requests/responses are preserved for forward compatibility. The proxy only modifies what it needs to fix.

## API Compatibility

The proxy maintains full compatibility with:

- **OpenAI Chat Completions API**: Standard request/response format
- **llama.cpp Extensions**: `timings` object, `/props`, `/slots` endpoints
- **Opencode Extensions**: `reasoning_text`, `reasoning_opaque`, extended usage details
- **Claude Code**: All standard features work seamlessly

See `../context/opencode_claude_llama_notes.md` for comprehensive client compatibility documentation.

## Troubleshooting

### Proxy won't start
```bash
# Check config validity
cargo run -- check-config --config config.yaml

# Test backend connectivity
cargo run -- test-backend --config config.yaml
```

### Fixes not applying
```bash
# List enabled fixes
cargo run -- list-fixes --verbose

# Enable debug logging
cargo run -- run --config config.yaml --log-level debug
```

### Metrics not appearing
Check that `stats.enabled: true` in your config and verify the format setting.
