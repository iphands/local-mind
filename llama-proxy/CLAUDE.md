# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`llama-proxy` is an HTTP reverse proxy for llama.cpp server that:
- Proxies requests to llama-server's OpenAI-compatible API
- Fixes malformed LLM responses (specifically tool call issues from models like Qwen3-Coder)
- Collects and logs performance metrics (tokens/sec, timing, context usage)
- Exports metrics to external systems (InfluxDB)

## Build, Run, and Test Commands

```bash
# Build the project
cargo build
cargo build --release

# Run tests (unit tests are embedded in source files)
cargo test

# Run a specific test
cargo test <test_name>

# Run with logging
RUST_LOG=debug cargo run -- run --config config.yaml

# CLI commands
cargo run -- run --config config.yaml                    # Start proxy server
cargo run -- run --debug --port 8066                     # With debug logging
cargo run -- list-fixes --verbose                        # List available fixes
cargo run -- check-config --config config.yaml           # Validate config
cargo run -- test-backend --config config.yaml           # Test backend connection

# Before first run, create config from template
cp config.yaml.default config.yaml
```

## Architecture

### Core Components

**proxy/** - HTTP server and request handling
- `server.rs`: Axum server setup, ProxyState with shared config/registry/exporters
- `handler.rs`: Request router that determines streaming vs non-streaming
- `streaming.rs`: SSE stream processing with fix application per chunk
- `context.rs`: Fetches context_total from backend /slots endpoint for stats

**fixes/** - Pluggable response fix system
- `mod.rs`: Defines `ResponseFix` trait with `applies()` and `apply()` methods
- `registry.rs`: Registry pattern for managing multiple fixes, enable/disable per fix
- Individual fix modules (e.g., `toolcall_bad_filepath_fix.rs`)
- Each fix implements: name, description, applies check, and apply logic
- Fixes work on both streaming chunks (`apply_stream()`) and complete responses (`apply()`)

**stats/** - Metrics collection
- `collector.rs`: `RequestMetrics` struct, calculates tokens/sec, context usage
- `formatter.rs`: Formats metrics as pretty/json/compact for logging
- Metrics collected for both streaming and non-streaming requests
- Context usage calculated from model's KV cache via /slots endpoint

**exporters/** - Remote metrics export
- `mod.rs`: `ExporterManager` with pluggable exporter trait
- `influxdb.rs`: InfluxDB v2 exporter with batching
- Exporters run async after request completes

**api/** - Type definitions
- `openai.rs`: OpenAI API types
- `llama.rs`: llama.cpp specific types
- Shared between request parsing and response handling

**config/** - YAML configuration
- `mod.rs`: Main AppConfig struct
- `loader.rs`: Loads from file, validates structure
- Supports CLI overrides for port, backend host/port

### Key Patterns

**Registry Pattern for Fixes**: `FixRegistry` stores `Arc<dyn ResponseFix>` allowing dynamic enable/disable without recompilation. Fixes are checked with `applies()` before calling `apply()`.

**Streaming vs Non-Streaming**: Handler detects streaming via `Content-Type: text/event-stream` header and routes to specialized streaming handler that applies fixes per SSE chunk.

**Stats Collection Flow**:
1. Parse request JSON to extract prompt tokens
2. After response, extract completion tokens and timing
3. Fetch context_total from backend /slots
4. Calculate metrics (tokens/sec, context percentage)
5. Format and log to stdout
6. Export to remote systems asynchronously

**Async Architecture**: Uses tokio runtime, reqwest for HTTP client, axum for server. All handlers are async, exporters spawn tokio tasks.

## Testing

Tests are embedded in source files using `#[cfg(test)]` modules. Run with `cargo test`.

Key test files:
- `src/fixes/registry.rs`: Fix registration and enable/disable logic
- `src/fixes/toolcall_bad_filepath_fix.rs`: Fix application logic
- `src/stats/collector.rs`: Metrics calculation
- `src/config/loader.rs`: Config parsing

## Configuration

The proxy requires `config.yaml` (copy from `config.yaml.default`). Key settings:

- `server.port`: Proxy listen port (default: 8066)
- `backend.host/port`: llama-server location (default: localhost:8080)
- `fixes.enabled`: Global fix toggle
- `fixes.modules.<name>.enabled`: Per-fix toggle
- `stats.enabled`: Enable metrics logging
- `stats.format`: pretty | json | compact
- `exporters.influxdb.enabled`: Enable InfluxDB export

## Adding New Features

**New Fix Module**:
1. Create `src/fixes/my_fix.rs` implementing `ResponseFix` trait
2. Register in `create_default_registry()` in `src/fixes/mod.rs`
3. Add config section to `config.yaml.default`
4. Implement both `apply()` for non-streaming and `apply_stream()` for streaming

**New Exporter**:
1. Create `src/exporters/my_exporter.rs` implementing `MetricsExporter` trait
2. Add initialization in `src/main.rs` run_proxy()
3. Add config section to `config.yaml.default`

**New Metric**:
1. Add field to `RequestMetrics` in `src/stats/collector.rs`
2. Update calculation logic in `from_response()` or `from_streaming_chunks()`
3. Update formatters in `src/stats/formatter.rs`
4. Add to InfluxDB fields in `src/exporters/influxdb.rs`
