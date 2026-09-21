# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`llama-proxy` is an HTTP reverse proxy for llama.cpp server that:
- Proxies requests to llama-server's OpenAI-compatible API
- Fixes malformed LLM responses (specifically tool call issues from models like Qwen3-Coder)
- Collects and logs performance metrics (tokens/sec, timing, context usage)
- Exports metrics to external systems (InfluxDB)

### Client Compatibility

The proxy sits between **llama.cpp server** and AI client tools, supporting:

**Supported Clients:**
1. **Claude Code CLI/TUI** (`../vendor/claude-code`) - Anthropic's official Claude CLI
2. **Opencode CLI/TUI** (`../vendor/opencode`) - Open-source AI coding assistant

**API Standards Supported:**
- ✅ **Vanilla OpenAI Chat Completions API** (`/v1/chat/completions`)
  - Standard request/response format
  - Streaming via Server-Sent Events (SSE)
  - Tool calls in OpenAI format with index-based accumulation
  - All standard parameters (temperature, top_p, max_tokens, etc.)

- ✅ **llama.cpp Extensions**
  - `timings` object for performance metrics (prompt_n, predicted_n, tokens/sec)
  - `/props` endpoint for server properties (n_ctx, model info)
  - `/slots` endpoint for KV cache monitoring
  - Extended sampling parameters (mirostat, DRY sampling, etc.)

- ✅ **Opencode Extensions** (full type-safe support)
  - **Request parameters**: `reasoning_effort`, `verbosity`, `thinking_budget` (pass-through to backend)
  - **Response fields**: `reasoning_text` / `reasoning_opaque` in messages and deltas
  - **Extended usage**: `completion_tokens_details` with `reasoning_tokens`, `accepted_prediction_tokens`, `rejected_prediction_tokens`
  - **Streaming support**: Proper accumulation of reasoning fields in SSE chunks
  - **Metrics tracking**: Extended token counts logged and exported to InfluxDB
  - **Note**: llama.cpp models don't generate these fields, but proxy preserves them for forward compatibility

**Key Compatibility Principles:**
1. **Transparent Pass-Through**: Unknown fields in requests/responses are preserved (forward compatibility)
2. **No Modification of Standard Fields**: OpenAI-compliant fields passed unmodified
3. **Extension Preservation**: Client-specific extensions preserved even if not used by backend
4. **Fix Layer Isolation**: Response fixes applied without breaking API contract

**Detailed Client Documentation:**
See `../context/opencode_claude_llama_notes.md` for comprehensive details on:
- Request/response formats for each client
- Streaming behavior and SSE format
- Tool call accumulation patterns
- Context window calculations
- Testing procedures
- Known issues and workarounds

**Architecture Insight:**
The proxy is designed as a **transparent interceptor** that:
- Routes all requests to llama.cpp backend unchanged
- Applies fixes to complete responses (the fix layer runs on the buffered body;
  `fake` mode - the default - fetches ONE complete JSON from the backend, runs the
  fix layer, then synthesizes the SSE stream from the fixed body; `passthrough`
  forwards the backend's SSE bytes verbatim, where fixes DETECT but do NOT repair)
- Collects metrics from llama.cpp-specific extensions
- Maintains full OpenAI API compatibility for maximum client support

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
- `handler.rs`: Request router with pass-through endpoints (/props, /slots, /health, /v1/models, /metrics)
- `streaming.rs`: SSE pass-through stream forwarding (passthrough mode) — forwards backend bytes verbatim; fixes DETECT only, never repair
- `context.rs`: Fetches context_total from backend /slots endpoint for stats

**fixes/** - Pluggable response fix system
- `mod.rs`: Defines `ResponseFix` trait with `applies()` and `apply()` methods
- `registry.rs`: Registry pattern for managing multiple fixes, enable/disable per fix
- Individual fix modules (e.g., `toolcall_bad_filepath_fix.rs`)
- Each fix implements: name, description, applies check, and apply logic
- Fixes run on complete responses (`apply()`); there is no streaming fix API —
  the per-chunk `apply_stream()` machinery was deleted (21f0548)

**stats/** - Metrics collection
- `collector.rs`: `RequestMetrics` struct, calculates tokens/sec, context usage, extended token details
- `formatter.rs`: Formats metrics as pretty/json/compact for logging (includes reasoning tokens when present)
- Metrics collected for both streaming and non-streaming requests
- Context usage calculated from model's KV cache via /slots endpoint
- Extended metrics: reasoning_tokens, accepted_prediction_tokens, rejected_prediction_tokens (Opencode/Copilot)

**exporters/** - Remote metrics export
- `mod.rs`: `ExporterManager` with pluggable exporter trait
- `influxdb.rs`: InfluxDB v2 exporter (bounded writer queue; one write per
  sample since tasks 81/84 - there is no batching)
- Exporters run async after request completes

**api/** - Type definitions
- `openai.rs`: OpenAI API types with Opencode extensions (reasoning fields, extended usage)
- `llama.rs`: llama.cpp specific types
- Shared between request parsing and response handling
- All extension fields use `Option<T>` with `#[serde(skip_serializing_if = "Option::is_none")]` for backward compatibility

**config/** - YAML configuration
- `mod.rs`: Main AppConfig struct
- `loader.rs`: Loads from file, validates structure
- Supports CLI overrides for port, backend host/port

### Key Patterns

**Registry Pattern for Fixes**: `FixRegistry` stores `Arc<dyn ResponseFix>` allowing dynamic enable/disable without recompilation. Fixes are checked with `applies()` before calling `apply()`.

**Streaming vs Non-Streaming**: In `fake` mode (default) the proxy fetches ONE complete
JSON from the backend, runs the fix layer on that buffered body, then synthesizes the SSE
stream from the fixed body (`src/proxy/synthesis.rs`). In `passthrough` mode the backend's
SSE bytes are forwarded verbatim (OpenAI `/v1/chat/completions` with `stream: true` only);
fixes DETECT but do NOT repair on that path. Both modes run fixes on buffered requests.

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

**Testing with Real Clients:**
```bash
# Test with Claude Code (point it at proxy instead of llama.cpp directly)
# Configure in Claude Code settings to use http://localhost:8066 as base URL

# Test with Opencode
# Configure Opencode to use http://localhost:8066 as OpenAI-compatible endpoint

# Manual API testing (simulates client behavior)
# Non-streaming request
curl -X POST http://localhost:8066/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"qwen3","messages":[{"role":"user","content":"Hello"}]}'

# Streaming request
curl -X POST http://localhost:8066/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"qwen3","messages":[{"role":"user","content":"Count to 5"}],"stream":true}'

# Tool call test
curl -X POST http://localhost:8066/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"qwen3","messages":[{"role":"user","content":"Get weather"}],"tools":[{"type":"function","function":{"name":"get_weather","parameters":{"type":"object","properties":{"location":{"type":"string"}}}}}]}'
```

**Reference Implementation:**
For understanding how clients interact with the API:
- Claude Code client code: `../vendor/claude-code/` (TypeScript/Node.js)
- Opencode client code: `../vendor/opencode/packages/opencode/src/provider/sdk/copilot/` (TypeScript)
- llama.cpp server API: `../vendor/llama.cpp/examples/server/` (C++)
- Comprehensive client notes: `../context/opencode_claude_llama_notes.md`

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
4. Implement `apply()` on the complete JSON response. There is no per-chunk
   streaming API to implement: the legacy `apply_stream()` machinery was deleted
   (21f0548); `fake` mode synthesizes SSE after the fix layer ran, `passthrough`
   forwards backend bytes untouched.

**New Exporter**:
1. Create `src/exporters/my_exporter.rs` implementing `MetricsExporter` trait
2. Add initialization in `src/main.rs` run_proxy()
3. Add config section to `config.yaml.default`

**New Metric**:
1. Add field to `RequestMetrics` in `src/stats/collector.rs`
2. Update calculation logic in `from_response()` or `merge_stream_stats()` (the per-chunk streaming stats accumulation)
3. Update formatters in `src/stats/formatter.rs`
4. Add to InfluxDB fields in `src/exporters/influxdb.rs`

## Streaming Fixes: No Per-Chunk Delta Calculation (Important!)

**The legacy per-chunk delta machinery was DELETED by big-fix 29 (21f0548).**
`ToolCallAccumulator`, the `ResponseFix` streaming methods, the
`FixRegistry::apply_fixes_stream*` family, `calculate_completion_delta` +
`safe_completion`, and the per-fix streaming overrides had zero production
callers and were removed wholesale (it flagged this section for docs sync).
Do not reintroduce delta subtraction against client-accumulated SSE chunks.

**Current reality** (authoritative wording lives in the `streaming:` block of
`config.yaml.default` and the streaming docs in README.md):

- **fake mode (default)**: the proxy fetches ONE complete JSON from the
  backend, runs the fix layer on that buffered response, then synthesizes the
  SSE stream from the FIXED body (`src/proxy/synthesis.rs`). The proxy emits
  every synthesized chunk itself, so no client-side delta accumulation can
  corrupt a repair. Chunk pacing/size are configured via `synthesis:`
  (`chunk_delay_ms`, `chunk_size_chars`).
- **passthrough mode**: the backend's SSE bytes are forwarded verbatim
  (OpenAI `/v1/chat/completions` with `stream: true` only). On that path fixes
  DETECT but do NOT repair — patching partial deltas is the corruption class
  the deleted machinery courted — and reprompt cannot run (it needs the
  complete body). Both still apply to buffered requests.

**Testing the streaming paths**: fix logic via `cargo test fixes::`; stream
behavior via the curl recipe in README "Is it really streaming?" (fake emits
everything at once after the backend finishes; passthrough trickles).

## Maintaining Client Compatibility

When modifying the proxy, ensure these compatibility requirements:

**MUST Preserve:**
- ✅ All standard OpenAI API fields (model, messages, temperature, etc.)
- ✅ Tool call format and IDs
- ✅ Finish reason values (stop, length, tool_calls, content_filter)
- ✅ Token counts in usage object (including extended details like reasoning_tokens)
- ✅ Message ordering and structure
- ✅ Reasoning fields (reasoning_text, reasoning_opaque) in both messages and streaming deltas
- ✅ Unknown request/response fields (pass-through)

**MUST NOT:**
- ❌ Modify standard OpenAI field values
- ❌ Strip or rename fields from requests/responses
- ❌ Reorder tool_calls array or change indices
- ❌ Alter streaming chunk format (SSE protocol)
- ❌ Change Authorization header behavior
- ❌ Break backward compatibility without versioning

**Testing Checklist for Changes:**
1. Run unit tests: `cargo test`
2. Test non-streaming requests with curl
3. Test streaming requests (verify SSE format)
4. Test tool calls (both streaming and non-streaming)
5. Verify metrics collection still works
6. Check that unknown fields pass through
7. Optionally: Test with real Claude Code or Opencode client

**When in Doubt:**
- Consult `../context/opencode_claude_llama_notes.md` for client behavior
- Check `../vendor/opencode/` for Opencode implementation patterns
- Review `../vendor/llama.cpp/examples/server/README.md` for backend API
- Prefer pass-through over modification (transparency principle)
