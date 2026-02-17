# llama-proxy e2e Tests

End-to-end test harness for `llama-proxy`. Simulates both the **client** (Claude Code / Opencode) and the **backend** (llama.cpp server), with the **real compiled proxy** running in between.

```
Mock Client (e2e) → [REAL llama-proxy binary] → Mock Backend (e2e)
```

This is a standalone Rust binary with **no shared code** with the main proxy project.

## Quick Start

```bash
# Build the proxy first (only needed once, or after proxy changes)
cd .. && cargo build --release && cd e2e

# Run all tests - that's it
cargo run
```

`cargo run` with no arguments automatically:
1. Finds the proxy binary (`../target/release/llama-proxy`, falls back to debug)
2. Starts the mock backend on port 18080
3. Spawns the proxy with `test_configs/proxy_fixes_on.yaml`
4. Runs all 24 tests
5. Kills the proxy and exits (code 0 = all pass, code 1 = failures)

## Filtering Tests

```bash
# Run only tests whose name contains a substring
cargo run -- --filter bad_filepath
cargo run -- --filter toolcall
cargo run -- --filter passthrough
```

## CLI Reference

```
cargo run                    # default: spawn proxy, run all tests
cargo run -- list            # list all test names and descriptions
cargo run -- run             # connect to an already-running proxy
cargo run -- spawn-and-run   # same as default, with optional overrides
```

### `--filter` / `-f`

Works on the default run and on every subcommand:

```bash
cargo run -- --filter null_index
cargo run -- run --filter streaming
cargo run -- spawn-and-run --filter toolcall
```

### `run` — connect to existing proxy

```bash
# Terminal 1: start proxy manually
../target/release/llama-proxy run --config test_configs/proxy_fixes_on.yaml

# Terminal 2: run tests against it
cargo run -- run
cargo run -- run --proxy-addr 127.0.0.1:18066
```

| Flag | Default | Description |
|------|---------|-------------|
| `--proxy-addr` | `127.0.0.1:18066` | Address of the running proxy |
| `--backend-port` | `18080` | Port the mock backend listens on |
| `--filter` / `-f` | *(none)* | Only run matching tests |

### `spawn-and-run` — explicit paths

```bash
cargo run -- spawn-and-run \
  --proxy-bin ../target/release/llama-proxy \
  --proxy-config test_configs/proxy_fixes_on.yaml
```

| Flag | Default | Description |
|------|---------|-------------|
| `--proxy-bin` | auto-detect | Path to `llama-proxy` binary |
| `--proxy-config` | `test_configs/proxy_fixes_on.yaml` | Proxy config YAML |
| `--backend-port` | `18080` | Mock backend port (must match config) |
| `--proxy-port` | `18066` | Proxy listen port (must match config) |
| `--filter` / `-f` | *(none)* | Only run matching tests |

## Test Configs

| Config | Purpose |
|--------|---------|
| `test_configs/proxy_fixes_on.yaml` | All fixes enabled — used by default |
| `test_configs/proxy_fixes_off.yaml` | All fixes disabled — for negative tests |

Both configs point the proxy backend at `127.0.0.1:18080` (the mock backend port).

## Test Coverage

24 tests across three categories:

### `basic/` — Core proxy behavior

| Test | What it verifies |
|------|-----------------|
| `simple_text_non_streaming` | Non-streaming JSON response passes through unchanged |
| `simple_text_streaming_synthesis` | Client gets SSE stream synthesized from complete backend JSON |
| `streaming_finish_reason` | Synthesized SSE has correct `finish_reason` in final chunk |
| `proxy_forces_non_streaming_to_backend` | Proxy always sends `stream:false` to backend |
| `sse_chunk_structure` | SSE stream has correct OpenAI format: role chunk → content chunks → final chunk → `[DONE]` |
| `backend_error_passthrough` | Backend 5xx errors are forwarded to the client |

### `toolcall/` — Fix modules

| Test | What it verifies |
|------|-----------------|
| `valid_non_streaming` | Valid tool call passes through unchanged |
| `valid_streaming` | Valid tool call synthesized to SSE; accumulated args are valid JSON |
| `no_fix_when_not_needed` | Non-`write_file` tools are not incorrectly modified |
| `multiple_tool_calls` | Multiple tool calls all pass through correctly |
| `empty_arguments` | `{}` arguments are valid and pass through |
| `bad_filepath/fixed_non_streaming` | Duplicate `filePath` key fixed in non-streaming response |
| `bad_filepath/fixed_streaming` | Duplicate `filePath` fixed; synthesized SSE accumulates to valid JSON |
| `bad_filepath/realworld_paths` | Fix works with real-world absolute paths (`.tsx`, deep dirs) |
| `bad_filepath/special_chars` | Documents known bug: fix returns `{}` for Unicode filePaths |
| `null_index/fixed_non_streaming` | `null` tool call index is replaced with `0` |
| `null_index/fixed_streaming` | Synthesized SSE has integer index after null-index fix |
| `malformed_args/fixed` | `{}":` pattern replaced with correct param name from tool schema |

### `passthrough/` — Monitoring endpoints

| Test | What it verifies |
|------|-----------------|
| `health` | `/health` returns proxy's own `OK` response (not proxied to backend) |
| `v1_health` | `/v1/health` is proxied to backend, returns `{"status":"ok"}` |
| `slots` | `/slots` is proxied to backend, returns slot array |
| `props` | `/props` is proxied to backend, returns server properties |
| `models` | `/v1/models` is proxied to backend, returns model list |
| `not_modified` | Pass-through responses have no extra proxy-injected fields |

## Architecture

### How It Works

1. `e2e` starts a **mock backend** (axum HTTP server) on port 18080
2. The **real proxy** is started (auto-spawned or pre-running) pointing at port 18080
3. Each test:
   - Queues a mock response on the backend (what `llama.cpp` would return)
   - Sends a request to the proxy (what Claude Code / Opencode would send)
   - Validates the proxy's response (JSON or SSE stream)

### Key Design Decisions

**Mock backend** (`src/backend.rs`): Returns a pre-configured queue of responses. Tests push responses before sending requests. Default handlers for `/health`, `/slots`, `/props`, `/v1/models` always return sensible defaults.

**Mock client** (`src/client.rs`): Thin wrapper around `reqwest`. `send_non_streaming()` and `send_streaming()` handle the two response paths. The SSE parser accumulates tool call argument deltas the same way Claude Code does.

**Sequential tests**: Tests run one at a time to avoid response-queue confusion on the shared backend.

**No shared code**: Zero imports from `llama-proxy`. Types like `MockResponse`, `ProxyResponse`, and `StreamingResponse` are defined locally in `src/types.rs`.

### Adding a New Test

1. Write a test function in `src/tests/basic.rs`, `toolcall.rs`, or `passthrough.rs`:

```rust
pub async fn test_my_scenario(ctx: TestContext) -> anyhow::Result<()> {
    // 1. Configure what the backend returns
    queue_response(&ctx.backend_state, MockResponse::json(
        backend_valid_tool_call_response("my_tool", r#"{"param":"value"}"#)
    ));

    // 2. Send request to the real proxy
    let resp = send_non_streaming(
        &ctx.http_client,
        &ctx.proxy_addr,
        basic_request("do something"),
    ).await?;

    // 3. Assert on the result
    assert_true(resp.status == 200, "Expected 200")?;
    let args = resp.tool_call_args(0, 0)
        .ok_or_else(|| anyhow::anyhow!("No tool call args"))?;
    assert_valid_json(args, "tool call arguments")?;

    Ok(())
}
```

2. Register it in `src/tests/mod.rs`:

```rust
test!(
    "toolcall/my_scenario",
    "Description of what this tests",
    toolcall::test_my_scenario
),
```

### Known Bugs Discovered

The `toolcall/bad_filepath/special_chars` test documents a real proxy bug:

> When `filePath` contains Unicode characters (e.g. `αβγ/文件.ts`), the `toolcall_bad_filepath` fix returns `{}` instead of the fixed JSON. The fix's string-truncation logic does not account for multi-byte UTF-8 character boundaries.

The test passes (it only asserts valid JSON), but prints a `KNOWN BUG:` line to stderr to make the issue visible.
