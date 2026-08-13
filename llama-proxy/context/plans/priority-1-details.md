# Priority 1: Critical Reliability Fixes - Detailed Implementation Guide

**Status**: Ready for implementation
**Revised**: Thu Aug 13 2026 — verified against the tree; snippets corrected to match real
types, real line numbers, and the real `ProxyState` construction pattern.
**Goal**: Add observability, protection against backend overload, and fix a silently-ignored
config key

---

## Ground Truth (read before writing code)

Facts checked against the current tree that the first draft got wrong:

| Claim in the first draft | Reality |
|---|---|
| Context size comes from `/slots` | `/props` (llama.cpp `default_generation_settings.n_ctx`), falling back to `/v1/models` (`data[0].max_model_len`) — `src/proxy/context.rs:36`. `/slots` is only a pass-through route. |
| `context_total` is `u32` | `u64` |
| `ProxyState::new(config)` exists | It does not. `ProxyState` is a plain struct literal built in `run_server` (`server.rs:145`) and in `create_test_handler_with_streaming` (`handler.rs:1010`). Both must be updated when adding a field. |
| `ProxyHandler` is `Clone` | Only `ProxyState` derives `Clone` (`server.rs:24`). |
| `ServerConfig` has `default_host` / `default_port` | It has no serde defaults at all: `pub struct ServerConfig { pub port: u16, pub host: String }` (`config/mod.rs:54`). |
| `/metrics` is free to repurpose | It is a backend pass-through (`handler.rs:317-337`). |
| `fetch_context_total` has one call site | Two: `handler.rs:689` and `streaming.rs:435`. |

---

## 1.1 Add Streaming Fallback Metrics

### Problem Statement

When the backend returns a streaming response despite the proxy forcing `stream: false`, the code
falls back to the legacy streaming handler (`handler.rs:518-541`). That path is:
- **Under-tested**: rarely hit in normal operation
- **Fragile**: complex delta calculation logic
- **Unmeasured**: nothing tracks how often it happens

Frequent hits would indicate backend misconfiguration, a bug in the force-non-streaming logic, or
that fake-streaming synthesis isn't engaging.

### Implementation Details

#### Step 1: Add Counter to ProxyState

**File**: `src/proxy/server.rs`

```rust
pub struct ProxyState {
    // ... existing fields
    pub concurrent_requests: Arc<AtomicUsize>,

    // NEW: backend returned streaming despite stream:false
    pub backend_streaming_fallback_hits: Arc<AtomicUsize>,
}
```

There is no constructor. Add the field at **both** struct-literal sites:
- `src/proxy/server.rs:145` (`run_server`)
- `src/proxy/handler.rs:1010` (`create_test_handler_with_streaming`)

Missing the second one is a compile error, not a silent bug — good.

**Why `Arc<AtomicUsize>`**: shared across async tasks, lock-free, same pattern as
`concurrent_requests`.

---

#### Step 2: Increment Counter in Handler

**File**: `src/proxy/handler.rs:510-541`

```rust
// Check if streaming response (unexpected since we force stream:false)
let is_streaming_response = backend_response
    .headers()
    .get(header::CONTENT_TYPE)
    .and_then(|ct| ct.to_str().ok())
    .map(|ct| ct.contains("text/event-stream"))
    .unwrap_or(false);

if is_streaming_response {
    // UNEXPECTED! Backend ignored our stream:false request
    let fallback_count = self
        .state
        .backend_streaming_fallback_hits
        .fetch_add(1, Ordering::Relaxed)
        + 1;

    tracing::warn!(
        backend_url = %backend.node.base_url(),
        fallback_count = fallback_count,
        "Backend returned streaming response despite stream:false request"
    );

    if fallback_count == 10 || fallback_count % 100 == 0 {
        tracing::error!(
            fallback_count = fallback_count,
            "Backend streaming fallback has triggered {} times - check backend configuration",
            fallback_count
        );
    }

    // Fall back to old streaming handler (arguments unchanged from current code)
    let concurrent_snapshot = self.state.concurrent_requests.load(Ordering::Relaxed);
    handle_streaming_response(
        backend_response,
        self.state.fix_registry.clone(),
        self.state.config.stats.enabled,
        self.state.config.stats.format,
        self.state.exporter_manager.clone(),
        request_json.clone(),
        start,
        backend.node.http_client.clone(),
        backend.node.base_url().to_string(),
        backend.group_name.clone(),
        backend.node.strip_path_prefix.clone(),
        self.state.dump_path.clone(),
        Some(method.to_string()),
        Some(uri.to_string()),
        Some(body_bytes.clone().to_vec()),
        concurrent_snapshot,
    )
    .await
}
```

---

#### Step 3: Expose Counter on a NEW Endpoint

> ⚠️ **`/metrics` is already taken.** `handler.rs:317-337` pass-throughs `/props`, `/slots`,
> `/health`, `/v1/health`, `/v1/models`, and `/metrics` to the backend. llama.cpp and vLLM both
> serve Prometheus metrics there. Overwriting it would shadow the backend's metrics and silently
> break any existing scrape config. Use **`/proxy/metrics`**.

**File**: `src/proxy/handler.rs` — add a new arm *before* the pass-through match:

```rust
// Proxy-local metrics (distinct from the backend's /metrics pass-through)
if method == Method::GET && path == "/proxy/metrics" {
    let fallback_hits = self.state.backend_streaming_fallback_hits.load(Ordering::Relaxed);
    let concurrent = self.state.concurrent_requests.load(Ordering::Relaxed);
    let rejected = self.state.rejected_requests.load(Ordering::Relaxed);

    let body = format!(
        "# HELP llama_proxy_backend_streaming_fallback_hits Total streaming fallback events\n\
         # TYPE llama_proxy_backend_streaming_fallback_hits counter\n\
         llama_proxy_backend_streaming_fallback_hits {fallback_hits}\n\
         # HELP llama_proxy_concurrent_requests Current in-flight requests\n\
         # TYPE llama_proxy_concurrent_requests gauge\n\
         llama_proxy_concurrent_requests {concurrent}\n\
         # HELP llama_proxy_rejected_requests Requests rejected at capacity\n\
         # TYPE llama_proxy_rejected_requests counter\n\
         llama_proxy_rejected_requests {rejected}\n"
    );

    return (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
        .into_response();
}
```

Note this arm must be placed where a backend has already been selected, or moved earlier and made
independent of `load_balancer.select()` — proxy-local metrics shouldn't 404 just because no
backend matches the (absent) model field.

---

#### Step 4: ~~Add to Request Log~~ — **DROPPED**

The first draft proposed adding `backend_streaming_fallback_hits` to `RequestMetrics` and the
stats formatters. That is a category error: it is a process-global monotonic counter, and
embedding it in per-request output would emit an ever-increasing number on every stats line and
into every InfluxDB point. Keep it in `/proxy/metrics` only.

---

### Testing

`ProxyState` derives `Clone`, so a test can hold the state and the handler separately. `ProxyHandler`
does **not** derive `Clone` — wrap it in an `Arc` if a test needs to share it across tasks.

```rust
#[tokio::test]
async fn test_streaming_fallback_counter() {
    // Build state the same way create_test_handler_with_streaming does (handler.rs:1010),
    // then keep a handle on the counter:
    let counter = Arc::new(AtomicUsize::new(0));
    // ... construct ProxyState { backend_streaming_fallback_hits: counter.clone(), .. }

    assert_eq!(counter.load(Ordering::Relaxed), 0);
    counter.fetch_add(1, Ordering::Relaxed);
    assert_eq!(counter.load(Ordering::Relaxed), 1);
}
```

For an end-to-end check (mock backend that returns SSE despite `stream:false`), use the **`e2e/`
crate**, not a `tests/` directory — see the note in section 1.3 testing.

---

## 1.2 Add Context-Fetch Failure Logging

### Problem Statement

When `fetch_context_total` returns `None`, the code silently continues with incomplete metrics.
Users can't distinguish "no data" from "fetch failed".

**Naming**: this is *not* `/slots`. `fetch_context_total` (`context.rs:36`) tries `/props` first,
then `/v1/models`, and returns `Option<u64>`.

### ⚠️ Negative caching is required

`context.rs` caches **successes** permanently (`CONTEXT_CACHE`, keyed by backend URL) but not
failures, and the miss path runs on every request. A backend serving neither `/props` nor
`/v1/models` would emit one warning per request, forever.

**Add a warn-once helper next to the cache in `src/proxy/context.rs`:**

```rust
static WARNED_BACKENDS: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();

/// Warn at most once per backend URL that context size could not be determined.
pub async fn warn_context_fetch_failed_once(backend_url: &str, model: &str) {
    let warned = WARNED_BACKENDS.get_or_init(|| RwLock::new(HashSet::new()));
    {
        if warned.read().await.contains(backend_url) {
            return;
        }
    }
    if warned.write().await.insert(backend_url.to_string()) {
        tracing::warn!(
            backend_url = %backend_url,
            model = %model,
            "Could not determine context size: neither /props nor /v1/models returned usable data. \
             Context usage metrics will be incomplete for this backend."
        );
    }
}
```

**Alternative for better testability**: Move `WARNED_BACKENDS` into `ProxyState` as an `Arc<RwLock<HashSet<String>>>`
so warnings reset on server restart and are easier to test.

### Current Code

**File**: `src/proxy/handler.rs:688-695`

```rust
// Fetch and set context_total for stats
if let Some(ref mut m) = metrics {
    if let Some(ctx_total) =
        fetch_context_total(&backend.http_client, backend.base_url(), backend.strip_path_prefix.as_deref())
            .await
    {
        m.context_total = Some(ctx_total);
        m.calculate_context_percent();
    }
    // If fetch returns None, we silently continue without context metrics
}
```

The same pattern exists at **`src/proxy/streaming.rs:435`** — both need the change.

### Fixed Code

```rust
if let Some(ref mut m) = metrics {
    match fetch_context_total(
        &backend.http_client,
        backend.base_url(),
        backend.strip_path_prefix.as_deref(),
    )
    .await
    {
        Some(ctx_total) => {
            m.context_total = Some(ctx_total);
            m.calculate_context_percent();
        }
        None => {
            warn_context_fetch_failed_once(backend.base_url(), &m.model);
            // Continue without context metrics - the request still succeeds
        }
    }
}
```

### ⚠️ Negative caching is required

`context.rs` caches **successes** permanently (`CONTEXT_CACHE`, keyed by backend URL) but not
failures, and the miss path runs on every request. A backend serving neither `/props` nor
`/v1/models` would emit one warning per request, forever.

Add a warn-once helper next to the cache in `src/proxy/context.rs`:

```rust
static WARNED_BACKENDS: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();

/// Warn at most once per backend URL that context size could not be determined.
pub async fn warn_context_fetch_failed_once(backend_url: &str, model: &str) {
    let warned = WARNED_BACKENDS.get_or_init(|| RwLock::new(HashSet::new()));
    {
        if warned.read().await.contains(backend_url) {
            return;
        }
    }
    if warned.write().await.insert(backend_url.to_string()) {
        tracing::warn!(
            backend_url = %backend_url,
            model = %model,
            "Could not determine context size: neither /props nor /v1/models returned usable data. \
             Context usage metrics will be incomplete for this backend."
        );
    }
}
```

Better still: consider caching the negative result inside `fetch_context_total` itself so the two
failing HTTP requests aren't repeated per request either. That is a real per-request latency cost
on a misbehaving backend, and it fixes the log spam as a side effect.

### Testing

Point the e2e mock backend at a config where `/props` and `/v1/models` both 404, then assert the
request still succeeds and that stats show no `context_total`. Asserting on log output is more
trouble than it's worth here; asserting the request survives is the contract that matters.

---

## 1.3 Implement Concurrent Request Limiting

### Problem Statement

The proxy tracks concurrent requests (`concurrent_requests`, incremented at `handler.rs:264`,
decremented by `ConcurrentGuard`'s `Drop` at `handler.rs:194`) but never enforces a limit. Under
load this can overwhelm the backend and exhaust file descriptors.

### Step 1: Add Config Option

**File**: `src/config/mod.rs:54`

Current:
```rust
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
}
```

There are **no serde defaults on this struct**. A bare new field would break every existing
`config.yaml`:

```rust
pub struct ServerConfig {
    pub port: u16,
    pub host: String,

    /// Maximum in-flight completion requests before returning 429. 0 = unlimited.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,
}

fn default_max_concurrent() -> usize { 100 }
```

**⚠️ Critical: Limit placement** — The limit must be checked **AFTER** the pass-through match
at `handler.rs:317-337`, NOT before. If checked before routing, `/health`, `/props`, `/slots`,
`/v1/models`, `/metrics`, and `/proxy/metrics` all count toward the limit and get 429'd at capacity
— precisely when monitoring needs them to work.

**Config example** (add to `config.yaml.default`):
```yaml
server:
  port: 8066
  host: "0.0.0.0"
  # Reject completion requests beyond this many in flight (429). 0 disables the limit.
  # max_concurrent_requests: 100
```

---

### Step 2: Enforce the Limit — two things the first draft got wrong

#### ⚠️ (a) `load()` then `fetch_add()` is racy

```rust
// WRONG - TOCTOU. N tasks can each observe max-1 and all proceed.
let current = self.state.concurrent_requests.load(Ordering::Relaxed);
if current >= max { return too_many(); }
self.state.concurrent_requests.fetch_add(1, Ordering::Relaxed);
```

Use a semaphore. `tokio` is already `features = ["full"]`, so `tokio::sync::Semaphore` costs
nothing new:

```rust
// In ProxyState
pub request_permits: Option<Arc<tokio::sync::Semaphore>>, // None = unlimited

// At the enforcement point (AFTER pass-through match)
let _permit = match self.state.request_permits {
    Some(ref sem) => match Arc::clone(sem).try_acquire_owned() {
        Ok(p) => Some(p),
        Err(_) => {
            self.state.rejected_requests.fetch_add(1, Ordering::Relaxed);
            return at_capacity_response(self.state.config.server.max_concurrent_requests);
        }
    },
    None => None,
};
```

The permit releases on drop, exactly like the existing `ConcurrentGuard`. Keep `ConcurrentGuard`
for the stats snapshot (`concurrent_snapshot` is passed into `handle_streaming_response`), or fold
both into one guard type.

#### ⚠️ (b) Do not gate monitoring endpoints

`concurrent_requests` is incremented at `handler.rs:264`, **before** routing. Putting the capacity
check there means `/health`, `/props`, `/slots`, `/v1/models`, `/metrics`, and `/proxy/metrics`
all count toward the limit and all get 429'd at capacity — precisely when monitoring needs to
work.

**Place the check AFTER the pass-through match at `handler.rs:317-337`, so only completion
routes are gated.** (Note this is also after backend selection, which is fine — selection is cheap
and in-process.)

#### Rejection response

Match the existing error style in `handler.rs:305-312`, which uses `Json(...).into_response()`.
That sets `Content-Type: application/json` for you, but does **not** set the status — so build it
explicitly to get both the status and `Retry-After`:

```rust
fn at_capacity_response(max: usize) -> Response {
    tracing::warn!(max = max, "Server at capacity, rejecting request");
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, "5")],
        Json(serde_json::json!({
            "error": {
                "type": "too_many_requests",
                "message": format!(
                    "Server at capacity ({max} concurrent requests), try again in 5 seconds"
                )
            }
        })),
    )
        .into_response()
}
```

---

### Step 3: Add Rejection Counter

**File**: `src/proxy/server.rs`

```rust
pub struct ProxyState {
    // ... existing fields
    pub rejected_requests: Arc<AtomicUsize>,
}
```

Again: add it at both struct-literal sites (`server.rs:145`, `handler.rs:1010`). Expose it via
`/proxy/metrics` (see 1.1 Step 3).

---

### Step 4: ~~Adaptive Per-Client Backoff~~ — **DROPPED**

The first draft proposed a `DashMap<client_id, Instant>` keyed on an `X-Client-ID` header. Neither
Claude Code nor Opencode sends that header, so the key would always be `"unknown"` and the backoff
would become a global 30-second lockout after a single rejection — strictly worse than the plain
limit. **Drop it entirely.**

If per-client fairness is ever wanted, key on something that actually exists (API key hash or
source socket address) and design it separately.

---

### Testing

> ⚠️ **`e2e/` is a separate crate.** It has its own `Cargo.toml` and `Cargo.lock`, NOT a workspace
> member of the root crate. Run tests with:
> ```bash
> cd e2e && cargo test
> ```
> NOT with `cargo test` from the root.

**Load test cases**:
1. Proxy configured with `max_concurrent_requests: 50`; mock backend holds each request open.
2. Fire 150 concurrent completion requests.
3. Assert: ~50 in flight succeed, the rest return 429 with `Retry-After: 5` and
   `Content-Type: application/json`.
4. Assert: `GET /health` returns 200 **while at capacity** (regression test for the placement bug).
5. Assert: `/proxy/metrics` reports a non-zero `llama_proxy_rejected_requests`.
6. Assert: no hangs; all permits released after the backend drains.

---

## 1.4 Fix `toolcall_null_index` Config Key Mismatch (NEW)

### Problem Statement

`FixRegistry::configure` (`registry.rs:229`) enables/disables fixes by matching config keys against
`fix.name()`:

```rust
pub fn configure(&mut self, config: &HashMap<String, crate::config::FixModuleConfig>) {
    for (name, module_config) in config {
        if self.fixes.iter().any(|f| f.name() == name) {
            self.enabled.insert(name.clone(), module_config.enabled);
        }
    }
}
```

The null-index fix reports `"toolcall_null_index_fix"` (`toolcall_null_index_fix.rs:72`), but every
config and doc uses `toolcall_null_index`:

- `README.md:13`, `README.md:115`
- `config.yaml.default` (fixes section)
- `e2e/test_configs/proxy_fixes_on.yaml:20`
- `e2e/test_configs/proxy_fixes_off.yaml:20`

The `any()` guard fails silently, so the key is ignored and **the fix cannot be disabled**. The
other two fixes (`toolcall_bad_filepath`, `toolcall_malformed_arguments`) match their config keys
exactly, which is why this went unnoticed.

Consequence beyond config: `e2e/test_configs/proxy_fixes_off.yaml` does not actually turn all fixes
off, so any e2e assertion built on "fixes disabled" is testing a different state than it claims.
(The global `fixes.enabled: false` path in `main.rs:225-228` *does* work — it disables everything
by iterating `list_fixes()` — so only the per-module key is affected.)

### Fix

**Preferred approach** — accept both spellings for backward compatibility:

**File**: `src/fixes/registry.rs:229`
```rust
pub fn configure(&mut self, config: &HashMap<String, crate::config::FixModuleConfig>) {
    for (name, module_config) in config {
        // Normalize: strip trailing "_fix" for comparison
        let normalized_name = name.strip_suffix("_fix").unwrap_or(name);
        for fix in &self.fixes {
            let fix_name = fix.name().strip_suffix("_fix").unwrap_or(fix.name());
            if normalized_name == fix_name {
                self.enabled.insert(name.clone(), module_config.enabled);
                break;
            }
        }
    }
}
```

**Why**: Users may already have `toolcall_null_index_fix` in their configs. Accepting both spellings
is safer than breaking existing configs.

**Alternative** (if you prefer strict naming): Rename the fix in `toolcall_null_index_fix.rs:72`:
```rust
fn name(&self) -> &str {
    "toolcall_null_index"
}
```
Then update all references in docs and configs.

Check for other references to the old string (`grep -rn toolcall_null_index_fix src/`) —
`src/fixes/mod.rs:6,16` are the module path and type name (`ToolCallNullIndexFix`), which stay as
they are.

### Testing

- Unit test in `registry.rs`: register the three default fixes, `configure` with
  `toolcall_null_index: { enabled: false }`, assert `is_enabled("toolcall_null_index") == false`.
- Unit test in `registry.rs`: also test that `toolcall_null_index_fix: { enabled: false }` works
  (backward compatibility).
- e2e: assert `proxy_fixes_off.yaml` leaves a null tool-call index untouched.

---

## Test Coverage Additions

**Add these test requirements to the plan**:

### 1.1 Streaming Fallback Metrics
- Verify counter increments when backend returns streaming despite `stream:false`
- Verify `/proxy/metrics` shows the counter
- Verify backend `/metrics` pass-through is unchanged

### 1.2 Context-Fetch Failure Logging
- Verify warning appears **once per backend URL**, not per request
- Verify request still succeeds even when fetch fails
- Verify warning message mentions `/props` and `/v1/models` (not `/slots`)

### 1.3 Concurrent Request Limiting
- Verify ~100 requests succeed when limit=100 and 150 are sent
- Verify rest get 429 with `Retry-After: 5` and `Content-Type: application/json`
- **Critical**: Verify `GET /health` returns 200 **while at capacity** (not 429)
- Verify `/proxy/metrics` reports non-zero `llama_proxy_rejected_requests`

### 1.4 Config Key Mismatch
- Verify both `toolcall_null_index.enabled: false` and
  `toolcall_null_index_fix.enabled: false` disable the fix
- Verify `list-fixes` output matches the accepted config key

---

## Acceptance Criteria Checklist

### 1.1 Streaming Fallback Metrics
- [ ] Counter added to `ProxyState` and to **both** struct-literal sites
- [ ] Counter increments on fallback, with backend URL in the log
- [ ] Escalates to `error!` at 10 hits and every 100 thereafter
- [ ] Exposed at **`/proxy/metrics`**; backend `/metrics` pass-through unchanged
- [ ] Not added to `RequestMetrics` / InfluxDB

### 1.2 Context-Fetch Failure Logging
- [ ] Warning at both `handler.rs:689` and `streaming.rs:435`
- [ ] At most one warning per backend URL (no per-request spam)
- [ ] Message names `/props` and `/v1/models`, not `/slots`
- [ ] Request still succeeds

### 1.3 Concurrent Request Limiting
- [ ] `max_concurrent_requests` added with `#[serde(default)]`, default 100, 0 = unlimited
- [ ] Enforced with a semaphore permit, not load-then-add
- [ ] Check placed **after** the pass-through match; health/status routes never rejected
- [ ] 429 with `Retry-After` and `Content-Type: application/json`
- [ ] Rejection counter tracked and exposed
- [ ] e2e load test covers capacity, health-during-capacity, and drain

### 1.4 Config Key Mismatch
- [ ] `toolcall_null_index: { enabled: false }` actually disables the fix
- [ ] README, `config.yaml.default`, and both e2e configs agree with the accepted key
- [ ] Unit test in `registry.rs` guards the mapping

---

## Rollback Plan

1. **Disable 1.3 via config**: `server.max_concurrent_requests: 0` (unlimited).
2. **Code revert**: `git revert` the individual commits — the four items are independent.
3. **Watch**: `llama_proxy_rejected_requests` at `/proxy/metrics`; any non-zero value on a
   previously-healthy deployment means the limit is set too low.

---

## Deployment Checklist

- [ ] `cargo test` passes
- [ ] `cargo run -- check-config --config config.yaml` accepts existing, unmodified configs
      (proves the serde default works)
- [ ] e2e suite passes
- [ ] Existing Prometheus scrape of `/metrics` still returns backend metrics
- [ ] Deployed, monitored, `/proxy/metrics` scraped

---

## Estimated Time Breakdown

| Task | Duration | Notes |
|------|----------|-------|
| 1.1: Streaming fallback counter + `/proxy/metrics` | 2-3 hours | Includes the new endpoint |
| 1.2: Context-fetch failure logging + warn-once | 1-2 hours | Two call sites |
| 1.3: Concurrent limiting | 3-4 hours | Semaphore + placement + e2e load test |
| 1.4: Config key mismatch | 1 hour | Rename + tests + doc sweep |
| **Total** | **7-10 hours** | ~1 working day |

---

## Dependencies

- None. `tokio::sync::Semaphore` is already available (`features = ["full"]`).
- **No `dashmap`** — the earlier plan's need for it came from a caching item that turned out to be
  already implemented in `src/proxy/context.rs`, plus the dropped per-client backoff.

---

## Success Metrics

- [ ] 0% increase in error rate
- [ ] Streaming fallback rate visible at `/proxy/metrics`
- [ ] Context-fetch failures logged once per backend, debuggable
- [ ] `toolcall_null_index` config key takes effect
- [ ] Concurrent limit prevents backend overload without rejecting health checks
- [ ] No breaking changes to existing configs

---

**Order of implementation**: 1.4 (smallest, fixes a real bug), then 1.2, then 1.1, then 1.3
(most complex).
