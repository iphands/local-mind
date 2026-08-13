# Plan: Address Critical Issues in llama-proxy

**Created**: Thu Aug 13 2026
**Revised**: Thu Aug 13 2026 — verified against the actual tree; several items were already
implemented or factually wrong. See "Corrections from the first draft" at the bottom.
**Status**: Ready for execution
**Estimated Effort**: ~1 day

---

## Overview

This plan addresses issues identified through code exploration. The focus is on observability,
overload protection, and correcting configuration handling, while maintaining backward
compatibility.

**Terminology note**: earlier drafts said context size is fetched from `/slots`. That is wrong.
`fetch_context_total` (`src/proxy/context.rs:36`) tries **`/props`** (llama.cpp,
`default_generation_settings.n_ctx`) and falls back to **`/v1/models`** (vLLM/OpenAI-compatible,
`data[0].max_model_len`). `/slots` is only a pass-through route in `handler.rs`. The value is a
`u64`, not a `u32`.

---

## Priority 1: Critical Reliability Fixes

### 1.1 Add Streaming Fallback Metrics
**Goal**: Track how often the legacy streaming fallback is hit

**Files to Modify**:
- `src/proxy/server.rs` (new `ProxyState` field + initialization at the struct literal, line ~145)
- `src/proxy/handler.rs` (increment at line ~518; new metrics route)

**Changes**:
1. Add counter to `ProxyState` (`server.rs:25`):
   ```rust
   pub struct ProxyState {
       // ... existing fields
       pub backend_streaming_fallback_hits: Arc<AtomicUsize>,
   }
   ```
   There is no `ProxyState::new()` — the struct is built as a literal in `run_server`
   (`server.rs:145`) and in the handler tests (`handler.rs:1059`). Both sites need the new field.

2. Increment in `handler.rs:518-541` when the fallback triggers.

3. Expose it on a **new** route — see the warning below.

> ⚠️ **Do not repurpose `/metrics`.** `handler.rs:317-337` currently pass-throughs
> `/props`, `/slots`, `/health`, `/v1/health`, `/v1/models`, and `/metrics` to the backend.
> Overwriting `/metrics` with proxy-local Prometheus output would shadow llama.cpp's / vLLM's
> own metrics and break existing scrapers. Add **`/proxy/metrics`** instead.
>
> **Placement**: Check path first and return early, OR place after backend selection. If placed
> before backend selection and tries to call `load_balancer.select()`, it will 404 when no backend
> matches the (missing) model field.

**Acceptance Criteria**:
- Counter increments when fallback occurs
- Counter exposed at `/proxy/metrics`; backend `/metrics` pass-through unchanged
- Warning logged on occurrence, escalating log level when it recurs
- Not added to per-request `RequestMetrics` or InfluxDB points

---

### 1.2 Add Context-Fetch Failure Logging
**Goal**: Make silent `/props` + `/v1/models` failures visible

**Files to Modify**:
- `src/proxy/handler.rs:689`
- `src/proxy/streaming.rs:435` — **second call site, easy to miss**

**Changes**: log a warning when `fetch_context_total` returns `None`.

> ⚠️ **Negative caching is required.** `context.rs` caches successes permanently but not
> failures, and it is called per request on the miss path. A backend exposing neither endpoint
> would emit one warning per request forever.
>
> **Solution**: Track warned URLs in a `static WARNED_BACKENDS: OnceLock<RwLock<HashSet<String>>>`
> (or move into `ProxyState` for testability). Warn once per backend URL, then skip.

**Acceptance Criteria**:
- Warning logged when the fetch fails, at both call sites
- At most one warning per backend URL (no per-request spam)
- Warning message mentions `/props` and `/v1/models`, not `/slots`
- No impact on request success

---

### 1.3 Implement Concurrent Request Limiting
**Goal**: Prevent backend overload

**Files to Modify**:
- `src/config/mod.rs` (`ServerConfig`, line 54)
- `src/proxy/handler.rs`

**Changes**:
1. Add config option. `ServerConfig` currently has **no serde defaults at all**:
   ```rust
   pub struct ServerConfig {
       pub port: u16,
       pub host: String,
   }
   ```
   The new field must carry `#[serde(default = "...")]` or every existing `config.yaml` fails to
   parse:
   ```rust
   pub struct ServerConfig {
       pub port: u16,
       pub host: String,
       #[serde(default = "default_max_concurrent")]
       pub max_concurrent_requests: usize,
   }

   fn default_max_concurrent() -> usize { 100 }
   ```

2. Enforce the limit — see the two warnings below.

> ⚠️ **`load()`-then-`fetch_add()` is racy (TOCTOU).** N tasks can each read `max - 1` and all
> pass. Use `tokio::sync::Semaphore` with `try_acquire_owned()` (tokio is already
> `features = ["full"]`), or a `compare_exchange` loop. The existing `ConcurrentGuard`
> (`handler.rs:194`) can stay for the stats snapshot, or be replaced by the permit.

> ⚠️ **Do not apply the limit to monitoring routes.** `concurrent_requests` is incremented at
> `handler.rs:264`, *before* routing — so `/health`, `/props`, `/v1/models`, and `/metrics` all
> count toward the limit and would be rejected at capacity, exactly when monitoring needs them.
>
> **Place the capacity check AFTER the pass-through match at `handler.rs:317-337`**, so only
> completion routes are gated. Health/status routes must never be rejected.

**Acceptance Criteria**:
- Requests rejected with 429 when limit reached, without a race window
- Health/status pass-through routes never rejected (even at capacity)
- `Content-Type: application/json` and `Retry-After` set on the rejection
- Configurable via `config.yaml`, defaulting to 100
- 0 = unlimited (disable the limit)

---

### 1.4 Fix `toolcall_null_index` Config Key Mismatch (NEW)
**Goal**: Make the documented config key actually work

**Problem**: `FixRegistry::configure` (`registry.rs:229`) matches config keys against
`fix.name()`. That fix reports `"toolcall_null_index_fix"` (`toolcall_null_index_fix.rs:72`), but
every config and doc uses `toolcall_null_index` — `README.md:115`, `config.yaml.default`,
`e2e/test_configs/proxy_fixes_on.yaml:20`, `e2e/test_configs/proxy_fixes_off.yaml:20`. The key is
silently ignored, so **the fix cannot be disabled**, and the e2e "fixes off" case is not testing
what it claims. The other two fixes (`toolcall_bad_filepath`, `toolcall_malformed_arguments`) match
their keys exactly.

**Solution** — accept both spellings for backward compatibility:

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

**Acceptance Criteria**:
- Setting `toolcall_null_index.enabled: false` actually disables the fix
- Setting `toolcall_null_index_fix.enabled: false` also works (backward compatibility)
- `list-fixes` output and README agree with the accepted key
- e2e `proxy_fixes_off.yaml` genuinely disables all three fixes

---

## Priority 2: Documentation & UX

### 2.1 Document Fix Ordering
**Status**: **mostly already done.** `fixes/mod.rs:249-260` already has the ordering comments and
explains why null-index runs first and malformed-arguments before bad-filepath.

**Remaining work**: promote the `//` comments to `///` doc comments so they surface in rustdoc.
~10 minutes.

Note the real type name when touching this code: `ToolCallNullIndexFix` (capital C) and it takes
an argument — `ToolCallNullIndexFix::new(true)`.

---

### 2.2 Streaming Mode Startup Warning — **DROPPED**

The first draft proposed warning whenever the mode is not `fake`. That does not fit the actual
enum:

- `StreamingMode` (`config/mod.rs:292`) is `Disabled | Fake | Accumulator`; the field is
  `config.streaming` (`StreamingConfig` is a type alias for `StreamingMode`), not
  `config.streaming_mode`.
- The default is already `Fake`.
- `accumulator` is documented in `config.yaml.default` as "NOT IMPLEMENTED - will error if used".
- That leaves `disabled`, which is a deliberate choice, not a legacy mode.

So the warning would be pure noise. If anything is worth adding, it is a hard error (not a warning)
when `accumulator` is selected — verify whether that already errors before adding one.

---

### 2.3 Extend `config.yaml.default`
**Goal**: Document all options in one place

The first draft proposed a new `config.yaml.full-example`. `config.yaml.default` already serves
that role and is fairly complete; a second file would drift. **Extend the existing file** instead.

**Actually missing / incorrect** in the current `config.yaml.default`:
- `server.max_concurrent_requests` (new, from 1.3)
- Per-node options under `backends.*.nodes` are only partly shown — `tls` is documented for the
  single `backend` block but not per node
- `exporters.influxdb` — `batch_size` and `flush_interval_seconds` are present; confirm they
  match `ExportersConfig`
- The `dump` section is entirely absent

Do **not** add `stats.fetch_context` — no such field exists. `StatsConfig` (`config/mod.rs:217`)
is exactly `{ enabled, format, log_interval }`.

**Acceptance Criteria**:
- Every field in `AppConfig` appears in `config.yaml.default`
- `cargo run -- check-config` accepts the file with all sections uncommented

---

## Priority 3: Testing

### 3.1 Integration Tests for Fix Interactions

> ⚠️ **`tests/integration/fixes.rs` will not run.** No `tests/` directory exists, and cargo only
> auto-discovers `tests/*.rs` at the top level — files nested in `tests/integration/` are not
> built as test targets without a `tests/integration/main.rs` or an explicit `[[test]]` entry.

Use the existing harness instead: `e2e/` is a separate crate with a runner, a mock backend
(`e2e/src/backend.rs`), and tests in `e2e/src/tests/{basic,passthrough,toolcall}.rs`.

**Files to Modify**:
- `e2e/src/tests/toolcall.rs` (exists) — multi-fix sequences
- `e2e/src/tests/mod.rs` — register any new test module

**Test Cases**:
1. Multiple fixes applied in sequence
2. Fix ordering doesn't break responses
3. Streaming + fixes combination
4. Config actually disables each fix (regression test for 1.4)

---

### 3.2 Load Test for Concurrent Limiting

**Files to Modify**:
- New module under `e2e/src/tests/`, registered in `e2e/src/tests/mod.rs`

**Test Cases**:
1. Send 150 concurrent completion requests with limit=100
2. Verify ~100 succeed, the rest get 429 with `Retry-After`
3. Verify `/health` still returns 200 while at capacity
4. Verify no crashes or hangs

---

## Dropped Items

### `/slots` context caching — **ALREADY IMPLEMENTED**
`src/proxy/context.rs:36` already has a process-global `CONTEXT_CACHE`
(`OnceLock<RwLock<HashMap<String, (u64, BackendType)>>>`), keyed by backend URL, permanent for the
process lifetime. `cache_context_from_preflight` primes it at startup from the preflight `/v1/models`
call, so in the common case **zero** requests ever perform a fetch. No `dashmap` dependency is
needed and no `ProxyState` field is needed. The ">90% cache hit rate" success metric is already
~100%.

### Early exit in `apply_fixes` — **NO-OP**
The proposed snippet added a `let mut modified = false;` that is never read (compiler warning) and
left the actual optimization commented out. `applies()` (`registry.rs:57`) is already the early
exit. If an `exclusive()` flag on the `ResponseFix` trait is genuinely wanted, propose it with a
concrete case where one fix must suppress the others — there isn't one today.

### Per-request `backend_streaming_fallback_hits` in `RequestMetrics` — **CATEGORY ERROR**
This is a process-global counter. Putting it in `RequestMetrics` would emit a monotonically
increasing global into every stats line and into every InfluxDB point. It belongs only in the
`/proxy/metrics` endpoint.

### Per-client backoff — **DROPPED**
The first draft proposed a `DashMap<client_id, Instant>` keyed on an `X-Client-ID` header. Neither
Claude Code nor Opencode sends that header, so the key would always be `"unknown"` and the backoff
would become a global 30-second lockout after a single rejection — strictly worse than the plain
limit. **Drop it entirely.** If per-client fairness is ever wanted, key on something that actually
exists (API key hash or source socket address) and design it separately.

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| New required config field breaks existing configs | Low | High | `#[serde(default)]` on `max_concurrent_requests` |
| Concurrent limit rejects health checks | High if placed wrong | High | Check *after* the pass-through match |
| Concurrent limit race lets N > max through | High if using load/fetch_add | Medium | Use `Semaphore::try_acquire_owned` |
| Overwriting `/metrics` breaks scrapers | High if done | High | Use `/proxy/metrics` |
| Renaming the null-index fix name breaks a config | Low | Low | Accept both keys, or note in changelog |
| Log spam from context-fetch warnings | High without dedup | Low | Warn once per backend URL |

---

## Rollout Plan

**Phase 1**: 1.1, 1.2, 1.4 — observability and the config-key bug. No behavior change for
correctly-working setups.

**Phase 2**: 1.3 — concurrent limiting. Ship with a high default, monitor, then tune.

**Phase 3**: 2.1, 2.3 — docs and config. No code change beyond comments.

**Phase 4**: 3.1, 3.2 — e2e coverage.

---

## Success Metrics

- [ ] 0% increase in error rate
- [ ] Streaming fallback rate visible at `/proxy/metrics`
- [ ] Context-fetch failures logged once per backend, not per request
- [ ] `toolcall_null_index` config key takes effect
- [ ] Concurrent limit prevents backend overload without rejecting health checks
- [ ] All e2e tests pass

---

## Dependencies

- None. `tokio` (already `features = ["full"]`) provides `Semaphore`; no `dashmap` needed.

---

## Corrections from the first draft

Recorded so the same mistakes don't come back:

1. Context size comes from `/props` and `/v1/models`, **not `/slots`**; the type is `u64`, not
   `u32`. (CLAUDE.md is stale on this too and should be corrected.)
2. `/slots` caching was already implemented in `context.rs` — priority dropped.
3. `/metrics` is an existing backend pass-through — must not be repurposed.
4. Fix-ordering documentation already exists in `fixes/mod.rs:249-260`.
5. `ProxyState::new()` does not exist; `ProxyHandler` is not `Clone` (only `ProxyState` is) —
   the original test snippets would not compile.
6. `ServerConfig` has no serde defaults today.
7. Type is `ToolCallNullIndexFix::new(true)`, not `ToolcallNullIndexFix::new()`.
8. `StreamingMode` has three variants and the field is `config.streaming`.
9. `stats.fetch_context` does not exist.
10. `tests/integration/*.rs` is not auto-discovered by cargo; the `e2e/` crate is the real harness.
11. `fetch_context_total` has two call sites, not one.
12. The `toolcall_null_index` config key never matches the fix name — new item 1.4.
