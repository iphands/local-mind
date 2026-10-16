# llama-proxy

[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

An HTTP reverse proxy for **vLLM** ([vllm-project/vllm](https://github.com/vllm-project/vllm)), [llama.cpp](https://github.com/ggerganov/llama.cpp), or any OpenAI-compatible server. It fixes malformed LLM responses, collects performance metrics, and exports telemetry to external systems.

vLLM is the primary supported backend; llama.cpp and any OpenAI-compatible server remain fully supported.

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
  - InfluxDB v2 support (single write per request through a bounded queue)
  - Extensible exporter architecture for other backends

- **Multi-Backend Load Balancing**: Route requests across multiple backends
  - Model-based routing: map model names to backend groups
  - Strategies: `round_robin` (cycle through) or `priority_free` (least-busy wins)
  - Per-node model override, API key, and TLS configuration
  - Catch-all group for unmatched models

- **Concurrency Control**: Protect backend from overload
  - `max_concurrent_requests`: Limit in-flight requests (default: 0 = unlimited)
  - Returns HTTP 429 when saturated, rejecting new requests gracefully
  - Monitoring endpoints always accessible for observability
  - Semaphore-based enforcement for strict capacity tracking

- **Streaming Synthesis**: Improved streaming reliability
  - `fake` mode: fetches complete JSON from backend, synthesizes SSE for clients
  - Eliminates delta calculation complexity and streaming fix edge cases
  - Tool calls delivered as single complete chunks

- **Request Augmentation** (experimental): Enrich requests before forwarding
  - Calls a fast LLM backend to generate additional context
  - Injects context into user messages transparently

- **Reprompt Engine** (optional): Silently recover from premature stops
  - Configurable prompt file with dynamic reload from disk
  - Customizable retry count and done sentinels
  - Returns clean stop on sentinel match; hides stops on tool call/content resume
  - Never drops assistant text: a follow-up turn is merged into the stopped turn, never
    substituted for it, so an answer the backend already gave always reaches the client
  - Skips read-only requests (`skip_read_only_requests`, default on): a request offering no
    file-mutating tool is a read-only subagent that answers once and has no task list to resume
  - Logs stop responses for debugging (optional)

- **Client Compatibility**: Works seamlessly with AI coding tools
  - Full OpenAI Chat Completions API support
  - Claude Code CLI/TUI compatibility
  - Opencode CLI/TUI compatibility
  - AnythingLLM compatibility (its `POST /api/models/vram-estimate` probe is answered by the
    proxy itself, so it learns your real context window instead of assuming 8192 tokens)
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

# Single backend server location (the example URL below is llama.cpp)
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

# Streaming mode (default: fake).  CLI --streaming-mode overrides this file.
#   fake        - one JSON from backend, synthesize SSE for client; fixes and reprompt run
#   passthrough - backend SSE copied verbatim; /v1/chat/completions with stream:true only.
#                 On that path fixes detect but do NOT repair and reprompt cannot run;
#                 both still apply to buffered requests.  See streaming: in
#                 config.yaml.default for the full trade-off.
#   disabled    - reserved; behaves like fake until enforced
# streaming: fake

# Streaming synthesis chunk timing (fake mode only). Chunking is CHAR-based
# (emoji-aware), never bytes.
# synthesis:
#   chunk_delay_ms: 0      # sleep inserted between synthesized SSE chunks (0 = instant)
#   chunk_size_chars: 2000 # max characters per synthesized text chunk

# Metrics logging
stats:
  enabled: true
  format: pretty  # pretty | json | compact

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

Point your AI coding tool at the proxy instead of the backend directly:

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
llama-proxy run --config config.yaml --port 8066
llama-proxy run --config config.yaml --log-level debug
llama-proxy run --config config.yaml --streaming-mode fake
llama-proxy run --config config.yaml --hide-requests        # suppress per-request log lines
llama-proxy run --config config.yaml --dump ./debug-dumps   # dump request/response pairs
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

With no backend split (vLLM without `--enable-per-request-metrics`, or any
Anthropic `/v1/messages` response) the Performance block names both derived
numbers instead of pretending to have a measurement:

```
Performance
  Decode (floor):        32.57 tokens/sec (wall-bounded)
  Work rate:             50.40 tokens/sec (all tokens/wall)
  (no backend split: decode is a floor, work is cost)
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
  "decode_tps_min": 32.57,
  "context_total": 4096,
  "context_used": 538,
  "context_percent": 13.1,
  "streaming": true,
  "finish_reason": "stop",
  "duration_ms": 30181.0
}
```

`decode_tps_min` is always present (the wall-bounded floor is computable whenever
the backend counted output tokens). `cached_tokens` and
`cache_creation_tokens` appear only when the backend reported them — absent is
not "zero cached".

### Compact Format

One line per completed request, on the `llama_proxy::stats` log target. Fields are
`key=value`, space-joined, right-aligned to fixed widths, never zero-padded.

**The head is columnar; the tail is not.** Everything from `model=` through `finish=`
sits at a fixed offset whatever the numbers say — prefill is swept from 1 to 9,999,999
tok/s and the decode floor from 0.07 to 99,999.9 — so a head cell can be sliced by
position:

| Cell | Columns | Slice |
|---|---|---|
| `model=` | 1-22 | `cut -c1-22` |
| rate (`pp=`/`tg=`) | 24-46 | `cut -c24-46` |
| `dur=` | 48-60 | `cut -c48-60` |
| `ctx=` | 62-78 | `cut -c62-78` |
| `tok=` | 80-96 | `cut -c80-96` |
| `stream=` | 98-111 | `cut -c98-111` |
| `finish=` | 113-129 | `cut -c113-129` |
| whole head | 1-129 | `cut -c1-129` |
| tail | 130- | `cut -c130-` |

Five limits on that table, all structural:

- **Offsets hold only through `finish=`, and `finish=` is a start offset.** After it the
  tail — `group=`, `work=`, `queue=`, `concurrent=`, `cache=`, in that order — is emitted
  conditionally, so its columns depend on what the request reported. A line with no tail
  at all simply ends at 129 (line 6); `cut -c130-` on it returns nothing. Slice the head
  by position, read the tail by key.
- **`content_filter` overflows `finish=` on purpose.** The pad is 10 cells: `tool_calls`
  fills it exactly, `content_filter` (14) pushes everything after it right. That is
  deliberate — a rare diagnostic is never clamped into invisibility — and it only moves
  tail columns, which were never columnar.
- **The table is bytes; the contract is display cells.** Widths are computed in terminal
  cells, so an ASCII name and a CJK name both occupy 22 cells and neither shifts the line.
  But `cut -c` counts bytes, so a non-ASCII model name misaligns the *slice*, not the
  line.
- **`model=` is truncated to 16 cells with a trailing `...`**, as line 5 shows:
  `Qwen3-14B-128K-Q3_K_S.gguf` prints as `Qwen3-14B-128...`. A full-name grep therefore
  never matches, and two models sharing their first 13 cells are indistinguishable here —
  read the true name from the `json` format.
- **Whitespace splitting is tier-dependent.** `pp=` disappears in tiers 2 and 3, and
  `work=` sits in the tail on tier 2 — on tier 3 only when the backend still reported a
  `usage.total_tokens`, which an abandoned stream usually has not — so field 2 is not the
  same quantity on two different lines. Don't `awk '{print $2}'`; use the offsets above or
  the recipe below.

Pulling one number out, whichever tier rendered it — both commands run against the lines
below, not typed from memory:

```bash
# decode tok/s, tier-tagged: "= 146.3" measured · ">= 131.0" a floor · "= n/a" nothing counted
sed -nE 's/.* tg([>=]+)[[:space:]]*([^ ]+)[[:space:]]+dur=.*/\1 \2/p'

# whole head, for lining two requests up side by side
cut -c1-129
```

The tag is not decoration: `>=` is a bound and `=` is a measurement, so a panel that
averages the column it gets from that command is averaging a number with a bound — the one
thing this line exists to prevent. Keep them in separate series.

The lines below **are** the formatter's output, not a description of it: a test renders
these same fixtures and fails if the file and the code ever disagree. Lines 1-4 are one
request four ways — 9,074 prompt tokens of which 3,200 came from the prefix cache, 412
generated, 3.144 s of wall clock, on a node advertising 262,144 tokens. Lines 5 and 6 are
separate requests, there to show the cells those four never reach: a truncated model
name, `finish=tool_calls`, `group=`, the `trunc` badge, and an empty tail.

```
model=cosmo-6000       pp=  14.765k tg=  146.3 dur=   3144ms ctx=  9074/262144 tok=  9074/   412 stream=ok      finish=stop       concurrent=1 cache=3200
model=cosmo-6000       pp=     n/a tg>=  131.0 dur=   3144ms ctx=  9074/262144 tok=  9074/   412 stream=ok      finish=stop       work=3017.18 concurrent=1 cache=3200
model=cosmo-6000       pp=     n/a tg=     n/a dur=   3144ms ctx=  9074/262144 tok=  9074/   n/a stream=gone    finish=n/a        concurrent=1
model=cosmo-6000       pp=     n/a tg=     n/a dur=     12ms ctx=   n/a/262144 tok=   n/a/   n/a stream=sync    finish=n/a        concurrent=1
model=Qwen3-14B-128... pp=  12.858k tg=   51.1 dur=   2841ms ctx=  4100/262144 tok=  4100/   118 stream=ok      finish=tool_calls group=local concurrent=3
model=cosmo-6000       pp=     n/a tg=     n/a dur=  30000ms ctx=  2048/262144 tok=  2048/   n/a stream=trunc   finish=n/a
```

The rate group has three tiers, and which one you see is a fact about the **backend**,
not about the request:

| Rendered | Meaning | Source |
|---|---|---|
| `pp=  14.765k tg=  146.3` | prefill and decode, **backend-sourced** — not both engine-computed | llama.cpp `timings` supplies `prompt_per_second`/`predicted_per_second`, computed by the engine; vLLM `metrics` (needs `--enable-per-request-metrics`) supplies counts and durations and **the proxy does the division** |
| `pp=     n/a tg>=  131.0` | no prefill rate exists; decode is **at least** 131.0 | `completion / wall`; its `work=` partner is in the tail |
| `pp=     n/a tg=     n/a` | nothing to measure (no counted output tokens) | — |

`tg>=` is a floor, never a measurement: the wall clock still contains prefill, network
and (in `fake` mode) the proxy's own synthesis, so it understates. It is the best
available number when the backend reports no split — the normal case for the Anthropic
`/v1/messages` path, where the proxy must buffer the response to repair it and the
backend's per-request timing block is not in the payload at all. `work=` is a **cost**
rate, not engine speed: on a long-context agent turn with a short completion it runs to
thousands while decode runs at tens.

A tier-2 line prints `pp=n/a` where the `pretty` box for the same sample prints a
`Decode (floor)` / `Work rate` pair: the head has room for one honest number per cell, so
it drops a derived figure rather than letting a fixed-width slot imply it is a measurement.

**Prefill is corrected for prefix-cache hits — but only where the body proves the
correction is well-defined.** A cache hit makes vLLM's `prompt_tokens` include tokens the
engine never recomputed, so the proxy discounts the cached prefix from the prefill
numerator, and only when the OpenAI `usage.prompt_tokens` shape proves that count is a
subset of the prompt (`collector.rs:591`). On the Anthropic shape `cached_tokens` is
`cache_read_input_tokens`, **disjoint** from `input_tokens`, so it is left alone —
subtracting it would understate prefill by the whole cached amount, and the rate would
merely look slow. A **full** hit (`cached == prompt_tokens`) is legitimate and left
undiscounted, which makes it the most inflated number this line can carry; `cached >
prompt_tokens` is reported as a data error rather than saturating. llama.cpp's `timings`
path runs the same formula but **saturates** where the vLLM path keeps the raw rate, and
whether it should subtract at all is unresolved upstream. The mechanics, and why a single
`if` cannot cover both shapes, are under *A prefill rate that looks too good to be true*.

`cache=N` is how many prompt tokens the engine served from its prefix cache. It is a
discount on the **`pp=` numerator only**. `work=` deliberately still counts those tokens:
it is a cost rate over everything the request moved and is kept on the wire precisely
because it reconciles with `sum(total_tokens)`, so subtracting the cache there too would
double-count the discount against a number that has to add up.

A prefill rate your hardware could not physically produce is counted by the canary at
`stats.max_plausible_prefill_tps` — see *A prefill rate that looks too good to be true*
below. Like the other stats counters it is **inert while `stats.enabled: false`**, and it
never reaches an exporter.

The `stream=` badge says how the proxy **obtained** the body, not what the client
received. There are five values and only these: `ok` a completed SSE stream; `gone` a
stream the client abandoned before the final chunk; `trunc` a stream cut short by the
backend or left incomplete; `stalled` a stream that stopped making progress; and `sync`
one buffered JSON — `fake` mode or `/v1/messages`. A client that asked for SSE still
receives synthesized SSE on a `sync` line.

`gone` is why line 3 looks thin: the final chunk is the only one carrying
`usage`/`timings`, so an abandoned stream has no **completion count** and no rate at all.
The prompt count is still there on it, because prompt tokens come from the request rather
than from the response — `ctx=` and the left side of `tok=` are filled while the right
side and both rates are not.

`finish=n/a` means the value was absent or arrived as `unknown`, as on line 4's backend
error body. Note the head and the log disagree here: the exporter's WARN prints
`finish_reason=unknown` while the head normalises it to `n/a`, so search them
differently.

**`dur=` is the only cell that saturates.** It is the proxy's own wall clock rather than a
backend-reported quantity, and it caps at 9,999,999 ms (~2.8 h): past that a request is
stuck, not slow, and a column that shifts during a stuck request teaches nothing. A
**non-finite** wall clock prints `NaN` (or `inf`) inside those same 7 cells — it is
deliberately *not* clamped to the cap, because `f64::min` returns the other operand when
one side is NaN, and a wedged backend would otherwise be logged as a plausible 2.8 hour
request. `ctx=`, `tok=` and the rate cells are never clamped at all: they are
backend-reported, and an out-of-band figure must widen the line rather than be rounded
into a number the engine never printed.

Nothing here records whether the proxy had to re-ask for a first token; that signal is the
reprompt counters (`curl -s localhost:8066/proxy/metrics | grep reprompt`), documented
under *Troubleshooting*.

**Arriving from the previous compact line?** The old line put prefill and decode behind a
single label: `tps=536.42/ 115.68` when the backend reported a split, `tps>= 146.32
work= 125.00` when it did not. Both are retired — grep this file for `tps=` and every hit
is in this section, which is the only place the old strings still appear. Five things
changed, each of which reads as data loss rather than as a rename if you are looking at a
dashboard:

- `tps=A/B` and `tps>=N` became three separate keys: `pp=`, `tg=` and `tg>=`. A panel
  keying either old form now finds *nothing*, rather than finding a number whose meaning
  silently split.
- The unlabelled bare number pair after `ctx=` gained a key: `tok=`.
- `dur= 184.0ms` became `dur=   3144ms`: integer milliseconds, still glued to the unit.
- Cells are space-padded to fixed widths, so a hand-typed single-space version of a line
  no longer matches it. Match the keys you need, or slice by the offsets above.
- The cell that used to sit before `finish=`, recording re-asked first tokens, is gone. It
  was never emitted by the formatter; the counters above are the real signal.


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
│  │  - InfluxDB (bounded queue)                            │ │
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
│          vLLM / llama.cpp / OpenAI-compatible API           │
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
│   ├── streaming.rs     # SSE pass-through stream forwarding (passthrough mode)
│   ├── synthesis.rs     # SSE synthesis from complete JSON (fake streaming)
│   └── context.rs       # Fetch context window from /props + /v1/models (cached)
├── backends/            # Multi-backend load balancing
│   ├── mod.rs           # Builder functions
│   ├── balancer.rs      # LoadBalancer trait and BackendGuard
│   ├── node.rs          # BackendNode (URL, TLS, model override, etc.)
│   ├── grouped.rs       # GroupedLoadBalancer (model-based routing)
│   ├── round_robin.rs   # RoundRobin strategy
│   ├── priority_free.rs # PriorityFree strategy (least-busy node)
│   └── preflight.rs     # Backend preflight/health checks
├── fixes/               # Pluggable response fix system
│   ├── mod.rs           # ResponseFix trait
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
│   └── influxdb.rs      # InfluxDB v2 exporter (bounded queue)
└── api/                 # Type definitions
    ├── openai.rs        # OpenAI API types (with Opencode extensions)
    └── llama.rs         # llama.cpp specific types
```

### Adding a New Response Fix

Response fixes implement the `ResponseFix` trait and are managed by the `FixRegistry`.

#### 1. Create Your Fix Module

Create `src/fixes/my_custom_fix.rs`:

```rust
use crate::fixes::{FixAction, ResponseFix};
use serde_json::Value;

/// Fix for [describe what your fix does]
///
/// The enabled/disabled switch is owned by the registry and the `fixes:`
/// config (a disabled module is not even constructed) - the struct carries
/// no flag of its own.
#[derive(Default)]
pub struct MyCustomFix;

impl MyCustomFix {
    pub fn new() -> Self {
        Self
    }
}

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
        // Apply fix to the COMPLETE response (the primary method; see the
        // "Streaming Fixes" section in CLAUDE.md).
        // Return (modified_response, FixAction) for standardized logging.

        tracing::debug!("Applying my_custom_fix");

        if let Some(choices) = response.get_mut("choices") {
            // ... your fix logic here ...
            return (response, FixAction::fixed("original snippet", "fixed snippet"));
        }

        (response, FixAction::NotApplicable)
    }
}
```

The context-aware pair (`applies_with_context` / `apply_with_context`, the latter
Result-typed) has trait defaults that delegate to `applies` / `apply`; override them
only when the fix needs the request body or can fail structurally.

#### 2. Register Your Fix

Add to `src/fixes/mod.rs`:

```rust
mod my_custom_fix;
pub use my_custom_fix::MyCustomFix;

pub fn create_default_registry() -> FixRegistry {
    let mut registry = FixRegistry::new();
    registry.register(Arc::new(ToolCallNullIndexFix::new()));          // FIRST
    registry.register(Arc::new(ToolcallMalformedArgumentsFix::new())); // before filepath
    registry.register(Arc::new(MyCustomFix::new()));                   // insert at the
    registry.register(Arc::new(ToolcallBadFilepathFix::new()));        // order you need
    registry
}
```

Also add the constructor to the `specs` array in `create_registry_from_config()` in the
same file - that is the builder the `run` command actually uses, and it keeps the same
load-bearing order.

**Important: Fix Registration Order**

The `create_default_registry()` function registers fixes in a critical order:

1. **ToolCallNullIndexFix** (FIRST) - Foundational fix that assigns sequential indices
   to tool calls lacking them. Other fixes assume valid indices exist.
2. **ToolcallMalformedArgumentsFix** - Handles the specific `{}`":" pattern in arguments
   before the broader filepath fix runs.
3. **ToolcallBadFilepathFix** - Removes duplicate filePath keys (runs last as it's more general)

**Note:** Config uses snake_case names (`toolcall_null_index`, `toolcall_malformed_arguments`,
`toolcall_bad_filepath`) without the `_fix` suffix. See the rustdoc on
`create_default_registry()` for detailed ordering requirements.

#### 3. Add Configuration Support

Update `config.yaml.default`:

```yaml
fixes:
  enabled: true
  modules:
    my_custom_fix:
      enabled: true
```

No `registry.rs` edit is needed for the toggle: `FixRegistry::configure()` already
normalizes the config key (`_fix` suffix optional) and records `modules.<name>.enabled`
for every registered fix, and `create_registry_from_config()` does not even construct a
module the config disables. Fix-specific options have no consumer today; if a fix needs
one, add the typed field to its config path rather than reading the generic
`FixModuleConfig.options` map.

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
Streaming mode is a config/CLI choice (`streaming:`, default `fake`), not a response header sniff.
In `fake` mode the proxy fetches ONE complete JSON from the backend, runs the fix layer on
that buffered body, then synthesizes the SSE stream from the fixed body (`src/proxy/synthesis.rs`)
— so fixes never touch partial deltas. In `passthrough` mode the backend's SSE bytes are
forwarded verbatim (`/v1/chat/completions` with `stream: true` only), and fixes DETECT but do
NOT repair on that path. Both modes run fixes on buffered requests.

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
- **vLLM**: per-request `metrics` for the measured prefill/decode split (the compact line's `pp=` and `tg=` cells), `/v1/models` `max_model_len` for the context window, and the `vllm:cache_config_info` gauge read by the `vram-estimate` endpoint
  - The split needs the backend started with `--enable-per-request-metrics`; without it the proxy shows a wall-bounded floor `tg>=` plus a cost rate `work=` instead of a fabricated split
- **llama.cpp Extensions**: `timings` object, `/props`, `/slots` endpoints
- **Opencode Extensions**: `reasoning_text`, `reasoning_opaque`, extended usage details
- **Claude Code**: All standard features work seamlessly
- **AnythingLLM**: Standard chat requests, plus `POST /api/models/vram-estimate`, which the proxy
  answers itself from the backend's advertised context window and never forwards

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

### Unknown fix name in config
The proxy now warns at startup if a config key doesn't match any known fix.
Both `toolcall_null_index` and `toolcall_null_index_fix` are accepted (the `_fix` suffix
is normalized). A typo like `toolcall_badfilepath` (missing underscore) will trigger a
warning listing all known fix names.

### Is it really streaming?
Watch deltas arrive. `-N` stops curl buffering; adjust the port to your `server.port`.
```bash
curl -N http://localhost:8066/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"any","messages":[{"role":"user","content":"Count slowly from 1 to 40"}],"stream":true}'
```
- `passthrough`: chunks trickle in over the whole generation, and the per-request log line
  reads `stream=ok`.
- `fake`: nothing, then every chunk at once after the backend finishes; the log line reads `stream=sync`.

`curl -s localhost:8066/proxy/metrics | grep openai_stream_passthrough_total` moves once per
streamed OpenAI request in `passthrough` and never in `fake`. `/v1/messages` (Claude Code)
is buffered in every mode, so it always looks like `fake` here.

### Metrics not appearing
Check that `stats.enabled: true` in your config and verify the format setting.

Samples with **no throughput signal** — no token count and no backend-reported
rate — are excluded from every exporter, including InfluxDB. That is the absence of
a measurement rather than a slow request: a stream the client abandoned before
llama.cpp sent its final chunk (the only chunk carrying `usage`/`timings`), or a
backend error body. Writing those would post a run of literal zeros into every
throughput aggregate.

They are counted exactly — each one increments
`llama_proxy_metrics_export_skipped_total` on `/proxy/metrics` — and 1 in 100 is also
logged at WARN (the first drop always logs) with `model`, `stream_end`,
`finish_reason`, `prompt_tokens` and `duration_ms`, the only fields of such a sample
that are not zero. Sampling is deliberate: a client that interrupts streams would
otherwise bury every other warning. So **do not hunt the log for a specific dropped
request** — 99 times in 100 no line exists for it; the counter is the reconciliation
number, not the log. A surviving line reads `stream_end=client_gone` for an abandoned
stream and `stream_end=sync finish_reason=unknown` for a backend error body.

Two things that will otherwise send you down a dead end: the counter only moves when
`stats.enabled: true` (the filter sits in the stats path, so an empty dashboard *and*
a counter at 0 means stats are off, not that the filter is hungry), and it is a
process-global counter that **resets on restart**, so a before/after diff spanning a
deploy will show it go down. The counter also spans both the buffered and
pass-through paths, which is why it does not match
`llama_proxy_passthrough_stream_client_gone_total`. And note the corollary of the
filter itself: load the proxy never observed no longer reaches InfluxDB, so remote
token totals under-count actual backend work by roughly the skipped count.

### A prefill rate that looks too good to be true

If the compact line's `pp=` cell reports a prefill rate your hardware could not
physically produce, the proxy flags it: any exported sample whose prefill rate exceeds
`stats.max_plausible_prefill_tps` (default `50000` tok/s) increments
`llama_proxy_stats_prefill_implausible_total` on `/proxy/metrics`.

**Treat it as a canary, not a fault.** The most common trip is entirely expected: on a
**full prefix-cache hit** vLLM reports `cached_tokens == prompt_tokens`, and the proxy
deliberately leaves that rate un-discounted, because claiming it computed zero tokens is
not a number it can stand behind (a *partial* hit is discounted — see the `cache=` cell).
That un-discounted rate is also the most inflated figure the proxy can honestly report. So
compare `cached_tokens` against `prompt_tokens` on the WARN line before believing anything
is broken: at or near equal, that is the cache case. If `cached_tokens=n/a` the backend
reports no prefix cache at all, and the WARN genuinely cannot separate benign from wrong —
judge `prompt_tps` against your own measured engine rate instead.

The WARN carries the raw fields and **no derived cache arithmetic**, deliberately. A
`prompt_tokens - cached_tokens` figure would be wrong on half the traffic: on the Anthropic
`/v1/messages` shape `cached_tokens` is `cache_read_input_tokens`, which `input_tokens`
already excludes, so the subtraction removes a prefix that was never counted in the first
place. The collector knows which shape it is holding; a log line printed later does not. And
any "implied prefill duration" divided back out of the rate is just `prompt_ms` again — the
rate *is* tokens over that same first-token interval. Compare the two printed numbers
instead. Run with `RUST_LOG=llama_proxy::stats=debug` and the collector's own
full-cache-hit line appears too, from the code that does know the shape.

Like its sibling above, this counter is **exact while the log is sampled** (first
occurrence, then one in a fixed interval — the WARN line prints the interval it used), so do
not hunt the log for a specific request; it only moves when `stats.enabled: true`, because a
run with stats off builds no sample at all; and it is process-global and **resets on
restart**, so a zero after a deploy proves nothing. It is never exported to InfluxDB — but
the rate that tripped it is: that number ships to your dashboard as a measured
`prompt_tps`, which is exactly why the canary is worth having.

Size the threshold by **measuring**, not by guessing, because it fails in both
directions: set below roughly 2x your real engine prefill rate and it warns on healthy
traffic forever, while an engine genuinely faster than the threshold leaves the canary
**dead and silent** — the worse failure, because it looks like health. Concretely: on a box
whose engine computes 14,765 tok/s, the `50000` default does **not** fire at a 22,000 tok/s
prefill figure, even though that is ~1.5x what the hardware can do. The default comes from
this deployment's own measurements (engine compute 14,765 tok/s, corrected warm runs
clustering 12.8k–14.3k, so ~3.4x headroom); send one long uncached prompt, read the prefill
figure off the compact line, and set yours from that. `0.0` or any negative value disables
the canary **entirely** — the counter stops too, this is not a log-only mute — and a
non-finite value (`inf`, `nan`) is rejected at load rather than silently disabling it.

### AnythingLLM assumed an 8192-token window

AnythingLLM's LocalAI provider asks the server how large the context window is by POSTing
`/api/models/vram-estimate`. Neither llama.cpp nor vLLM has ever heard of that path, so the request
used to come back as a 404 and AnythingLLM fell back to a hard-coded 8192 tokens no matter how big
your model actually is. The proxy answers that path **itself** and never forwards it, using the
context size it learned during startup preflight or, when that cache is cold, the backend's own
advertised window (`/props` `n_ctx`, otherwise `/v1/models` `max_model_len`). The window
AnythingLLM shows you is now the real one.

The only field AnythingLLM reads out of that response is `context_length`, in tokens. The body also
carries a nested `vllm` object, and that is real data scraped from the backend's
`vllm:cache_config_info` gauge: `num_gpu_blocks`, `block_size`, `kv_cache_size_tokens`,
`kv_cache_max_concurrency`, `gpu_memory_utilization`, `cache_dtype`, `enable_prefix_caching`, each one
passed through as the string vLLM printed. It appears only when the backend exposes that gauge, so a
llama.cpp backend simply has no `vllm` key. `engine` is a series selector rather than a memory
figure, so it is not surfaced. The byte-sized fields that endpoint's schema declares stay absent on
purpose: vLLM prints its KV-cache memory figure to its own stdout while the engine profiles and
never serves it over HTTP, so there is no honest number to report, and AnythingLLM does not read
those fields either. The proxy reports no figure it cannot source.

Two limitations are documented rather than repaired:

- The window is **per backend node, not per model**. One number is cached per backend URL, so a
  server hosting several models with different windows reports one number for all of them.
- A request whose `model` matches no backend mapping still gets a **503** at load-balancer
  selection, before the shim is reached. The shim only answers requests the proxy already routed.

Both outcomes log at `debug!`, which is off by default, so the signal an operator can actually see
is a pair of counters:

```bash
curl -s localhost:8066/proxy/metrics | grep vram_estimate
```

- `llama_proxy_vram_estimate_served_total`: answered with a real context window.
- `llama_proxy_vram_estimate_unknown_total`: answered with the proxy's own 404 because the backend
  advertised no context window anywhere, which sends AnythingLLM back to its default and gives you
  the 8192 behaviour above. Like the counters above, both are per process and reset on restart.

Nothing here changes a completion request. `/v1/chat/completions` is still forwarded as it always
was; only the management probe is answered locally.

### The `context_total` / `context_percent` in your metrics

Every sample carries the backend's advertised context window (`context_total`) and, when a request
used part of it, `context_percent`. The number is the same advertised window the AnythingLLM shim
above reports: `/props` `n_ctx` for llama.cpp, otherwise `/v1/models` `max_model_len`. It is learned
once at startup and then **cached per backend URL**, not fetched on every request, so three things
about it are worth knowing:

- **The probe is bounded.** When the cache is cold the proxy probes the backend, but the whole probe
  is capped at 2s for `/props` and `/v1/models` together. A backend that accepts the connection and
  never answers can no longer delay an already-complete response; that stall becomes a counted
  timeout instead. A backend behind an `api_key` is probed *with* that key, exactly as startup
  preflight does — so an auth-guarded backend now reports a window instead of silently 401-ing the
  probe and leaving `context_total` empty.
- **The cache expires.** A cached window is dropped 600s after it was fetched, and *immediately* when
  the backend is marked failed. This is what lets a backend restarted with a different `-c` be
  picked up: without it, the first window seen for a URL would be reported forever, because a
  restart keeps the same base URL.
- **Multi-model `/v1/models` is selected, not guessed.** The cold fallback no longer reads
  `data[0]`. It uses the same policy as startup: the entry whose id matches the node's configured
  model name, otherwise the largest `max_model_len` among entries that carry both an `id` and a
  numeric `max_model_len`. An entry with no `id` is ignored, which matches what vLLM and
  OpenAI-compatible servers actually emit.

Like the counters above, the window is cached **per backend node, not per model**: a node hosting
several models reports one number for all of them, and this section changes only which `/v1/models`
entry is chosen for that node, not the per-node caching.

These outcomes are invisible in the default log, so an operator's signal is the counters:

```bash
curl -s localhost:8066/proxy/metrics | grep -E 'context_probe_timeout|context_cache'
```

- `llama_proxy_context_probe_timeout_total`: probes that hit the 2s bound (the response was not
  delayed by them; this says how often it would have been).
- `llama_proxy_context_cache_evictions_total`: windows dropped because their backend failed, so the
  next request re-probes it. A number that climbs with a flapping backend is expected.
- `llama_proxy_context_cache_stale_skips_total`: refreshes that found the cache write-locked and were
  skipped, so the old window was kept one more round; it explains `context_percent` jumps.

The reprompt engine adds its own three, also process-global and reset on restart:
`llama_proxy_reprompt_skipped_read_only_total` (declined: no mutating tool),
`llama_proxy_reprompt_triggered_total` (a premature stop it acted on), and
`llama_proxy_reprompt_exhausted_total` (a loop that ran out of rounds or budget and returned the
text unchanged — a subset of triggered).
