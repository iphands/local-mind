# Plan 01 — stats columnar + prefill fix

> **Status**: done
> **Created**: 2026-10-16
> **Depends on**: N/A
> **Goal**: Make the compact stats line columnar (reordered, right-aligned fixed widths) and fix the inflated prefill TPS caused by prefix-cache hits.
> **Agent**: implementation agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Reorder and pad the compact stats line for columnar readability, and correct the prefill TPS that llama.cpp inflates when prefix-cache hits are present.

**Deliverables**:
1. Compact stats line reordered to `model, tps, dur, ctx, cache, tokens, stream, finish, concurrent` with right-aligned fixed widths.
2. Prefill TPS corrected to `(prompt_n - cache_n) / prompt_ms * 1000` when cache hits are present.
3. Unit tests for the corrected rate and the new column order/widths; full stats suite green.
4. `README.md` compact-format example regenerated; breaking change documented.

**Estimated effort**: Medium (1–2 days)

## Scope

**In**: `src/stats/collector.rs`, `src/stats/formatter.rs`, `README.md`.
**Out**: pretty format (unchanged — reads the corrected `prompt_tps` field automatically), JSON format (unchanged — serializes the struct), vLLM path (unchanged — computes its own `prompt_tps` from ttft), InfluxDB export (unchanged — writes the corrected `prompt_tps` field automatically).
If the work must cross this line (e.g. changing the pretty layout), **stop** and record a blocker in the tracker instead of expanding scope.

## Context

### Pre-Identified Bug/Issue

llama.cpp reports `timings.prompt_per_second = prompt_n / prompt_ms * 1000`, and the proxy passes it through verbatim as a tier-1 "measurement". When prefix-cache hits are present, `prompt_n` counts the cached tokens but `prompt_ms` covers only the non-cached compute, so the rate is inflated — physically implausible numbers appear on real lines:

```
2026-10-16T02:18:14.861332Z  INFO llama_proxy::stats: model=cosmo-6000 tokens=164663/2154 tps=880447.55/195.36 cache=164096 ctx=164663/524288 stream=ok finish=tool_calls dur=11410.0ms concurrent=1
2026-10-16T02:19:52.866495Z  INFO llama_proxy::stats: model=cosmo-6000 tokens=64641/2646 tps=369942.79/178.82 cache=64128 ctx=64641/524288 stream=ok finish=tool_calls dur=15058.0ms concurrent=1
2026-10-16T02:18:28.527879Z  INFO llama_proxy::stats: model=cosmo-6000 tokens=175566/258 tps=405202.50/245.80 cache=175488 ctx=175566/524288 stream=ok finish=tool_calls dur=1684.0ms concurrent=1
```

For `tokens=164663, cache=164096`, only ~567 tokens were actually computed; the true prefill rate is ~3,000 tok/s, not 880k. Lines with low/no cache show sane prefill TPS (12k–15k), and the generation TPS (second value) is always sane (69–257) — the bug is isolated to prefill.

### Why [Approach]

Correct the rate rather than suppress it: every input (`prompt_n`, `cache_n`, `prompt_ms`) is backend-measured, so nothing is invented, and the real compute signal is preserved. Suppressing to `n/a` throws information away. Zero-padding the compact line was rejected in favor of right-aligned spaces, since zero-padding rates fakes precision.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| `prompt_per_second = prompt_n / prompt_ms * 1000` passed through verbatim | backend artifact | `src/stats/collector.rs:371-375` |
| `tps=880447.55`, `tokens=164663`, `cache=164096` → non-cached ≈ 567 | inflation ≈ 290× | verbatim log line above |
| `tps=369942.79`, `tokens=64641`, `cache=64128` → non-cached ≈ 513 | inflation ≈ 126× | verbatim log line above |
| `tps=405202.50`, `tokens=175566`, `cache=175488` → non-cached ≈ 78 | inflation ≈ 2250× | verbatim log line above |
| Low-cache lines show sane prefill TPS (12k–15k) | not a general breakage | log lines, e.g. `tps=14914.45` |
| Generation TPS always sane (69–257) | bug isolated to prefill | all log lines |
| Gate red at init (clippy/fmt/test fail pre-existing) | standing constraint | `init` baseline, recorded in `RULES.md` project gate |

**Unverified assumption (Open Question 1)**: llama.cpp's `prompt_per_second` includes cache-hit tokens in `prompt_n`. If llama.cpp already excludes them, `(prompt_n - cache_n)/prompt_ms` double-subtracts and understates prefill TPS. Confirm before implementing T2.

### Rejected Claims

- "The prefill TPS is genuinely that fast" — rejected; it scales exactly with cache-hit ratio and is physically implausible.
- "The generation TPS is also buggy" — rejected; the second value is sane on every line.
- "The fmt diff in `src/stats/formatter.rs` is from this work" — rejected; pre-existing at init (baseline recorded in `RULES.md`).

## Step-by-Step Tasks

### T1: Make the project gate green at init

**File**: `src/stats/formatter.rs` (and any pre-existing clippy/test failures)

**What to do**: The init baseline (RULES.md project gate, steps 2–4) fails pre-existing on clippy, `cargo fmt --check`, and `cargo test --workspace --lib`, including a formatting diff in `src/stats/formatter.rs` — the exact file this plan rewrites. Fix those pre-existing failures so the gate is green. Scope T1 tightly to making the gate pass; do not add logic here. Record any failures unrelated to this plan as standing constraints in `SERIES.md` rather than expanding scope.

**Before**: `cargo fmt --check` exits 1 with a diff in `src/stats/formatter.rs`; `cargo clippy --all-targets -- -D warnings` and `cargo test --workspace --lib` exit non-zero at init.

**After**: All four gate steps exit 0.

**Verify**:
```bash
cargo check --workspace --features influxdb -- -D warnings && cargo clippy --all-targets --features influxdb -- -D warnings && cargo fmt --check && cargo test --workspace --lib
```
→ exit 0. Record which failures were pre-existing vs. fixed.

**Expected observation**: gate green; no pre-existing fmt/clippy/test failures remain unaccounted for.

**Commit**: `[llama-proxy][P1][T1][gate] make project gate green at init`

### T2: Fix inflated prefill TPS from prefix-cache hits

**File**: `src/stats/collector.rs` (`extract_timings`)

**What to do**: In `extract_timings`, the proxy reads `timings.prompt_per_second` verbatim (`src/stats/collector.rs:371-375`) *before* reading `cache_n` (`:393`). Restructure so `prompt_n` (via `context_used`), `cache_n` (via `cached_tokens`), and `prompt_ms` are read first, then compute the corrected prefill TPS when cache hits are present:

- When `has_timing_split && cached_tokens > 0 && prompt_ms > 0`: `prompt_tps = (context_used - cached_tokens) / prompt_ms * 1000`, wrapped in `finite_rate` (`collector.rs:345`).
- Otherwise: fall back to the backend `prompt_per_second` verbatim (unchanged behavior for the no-cache case).
- Guard against division by zero (`prompt_ms > 0`) and inconsistent data (`context_used >= cached_tokens`; fall back otherwise, never saturate to a "measured zero").
- Keep `has_timing_split = true` so the compact line keeps the `tps=X/Y` tier. Scope: llama.cpp `timings` path only; the vLLM path (`extract_vllm_usage`, `:433`) computes its own `prompt_tps` from ttft and is untouched.

**Before**: `metrics.prompt_tps = finite_rate(timings.prompt_per_second, …)` unconditionally (line 371-375).

**After**: corrected rate when cache hits present; backend verbatim otherwise.

**Verify**: unit test in `src/stats/collector.rs`:
- response with `timings:{prompt_n:1000, cache_n:200, prompt_ms:500, prompt_per_second:2000.0, predicted_per_second:50.0}` → `prompt_tps == 1600.0` ((1000-200)/500*1000), `has_timing_split == true`.
- no-cache case → backend `prompt_per_second` verbatim.
```bash
cargo test stats::collector
```
→ all pass.

**Expected observation**: corrected rate ≈ 1,600.00 tok/s for the cached case (true compute rate), not 2,000.00 (inflated backend value).

**Commit**: `[llama-proxy][P1][T2][stats] fix inflated prefill tps from prefix-cache hits`

### T3: Reorder and pad compact stats columns, update affected tests

**File**: `src/stats/formatter.rs` (`format_compact`)

**What to do**: Reorder the compact line to `model, tps, dur, ctx, cache, tokens, stream, finish, concurrent` with right-aligned fixed widths (`tps={:>6.2}`, `dur={:>6.1}`) instead of zero-padding. **Test updates are part of this task** (not a separate task): reordering breaks existing formatter assertions, so update them in the same task to keep the gate green per Rule A. Decision on `queue=`: keep it appended at the end (saturation signal, `formatter.rs:193-197`) — do not drop it silently.

**Before**: `model={} tokens={}/{} {tps} {cache} {ctx} {stream} {finish} dur={:.1}ms{queue}{concurrent}` (current order).

**After**: `model={} {tps} dur={:>6.1}ms {ctx} {cache} tokens={}/{} {stream} {finish} {queue}{concurrent}`.

**Verify**:
```bash
cargo test stats::formatter
```
→ all pass; eyeball a rendered compact line for the new order and widths.

**Expected observation**: column order is `model, tps, dur, ctx, cache, tokens, stream, finish, concurrent`; numbers right-aligned within their widths; no leading zeros; gate green.

**Commit**: `[llama-proxy][P1][T3][stats] reorder and pad compact stats columns`

### T4: Remaining stats tests (full suite)

**File**: `src/stats/formatter.rs`, `src/stats/collector.rs`

**What to do**: Run the full stats test suite and fix any remaining assertions that reference the old column order, old values, or the inflated prefill rate. In particular `test_request_metrics_from_response_with_timings` (`collector.rs:764`) has `prompt_per_second: 198.0` with `cache_n: 10` — the corrected rate is ~633.66, so that assertion must change.

**Verify**:
```bash
cargo test --workspace --lib
```
→ exit 0.

**Expected observation**: all tests pass; no assertions reference the old order, old values, or the inflated rate.

**Commit**: `[llama-proxy][P1][T4][stats] update remaining stats tests`

### T5: Document breaking compact format change in README

**File**: `README.md`

**What to do**: Regenerate the compact-format example from real output; document the new column order, the right-aligned widths, the dropped/kept cells (keep `queue=`), and that the prefill TPS is now corrected for prefix-cache hits. Rule B: a change in observable behavior updates `README.md` in the same commit.

**Verify**: README compact example matches output rendered by the updated formatter (derive from unit-test output; no backend needed).

**Expected observation**: README example is consistent with the new compact line; breaking change is called out.

**Commit**: `[llama-proxy][P1][T5][docs] document breaking compact stats format change`

## Critical Files

| File | Change | Priority |
|---|---|---|
| `src/stats/collector.rs` | Corrected prefill TPS when prefix-cache hits are present | P0 |
| `src/stats/formatter.rs` | Reorder + pad compact columns; update affected tests | P0 |
| `README.md` | Regenerate compact example; document breaking change | P1 |

## Open Questions / Risks

1. **Question: llama.cpp `prompt_per_second` semantics — does `prompt_n` include cache-hit tokens?** If llama.cpp already excludes cached tokens from `prompt_n`/`prompt_per_second`, subtracting `cache_n` double-subtracts and understates prefill TPS. *How we'll settle it*: check llama.cpp source or probe a backend with prefix caching before T2; if it already excludes, do not subtract (report backend verbatim). *Mitigation*: gate the recompute on `prompt_ms > 0` and `cached_tokens > 0`; fall back to backend verbatim otherwise.
2. **Question: `queue=` cell in reorder — keep or drop?** *Mitigation*: keep it appended at the end; record the decision in T3.
3. **Question: fixed widths symmetric (6/6) or 6/4 for the two tps values?** *How we'll settle it*: reviewer guidance — right-aligned spaces, widths as minimums; choose in T3, record in tracker Notes/Deviations.

**Never delete a question** — resolve it in place (~~strike~~ + "RESOLVED (T3): …") or mark it `deferred — <reason>` when you defer it.

## Verification Checklist

- [ ] T1: `cargo check --workspace --features influxdb -- -D warnings && cargo clippy --all-targets --features influxdb -- -D warnings && cargo fmt --check && cargo test --workspace --lib` → exit 0
- [ ] T2: unit test shows corrected `prompt_tps == 1600.0` for cached case; backend verbatim for no-cache
- [ ] T3: compact line order `model, tps, dur, ctx, cache, tokens, stream, finish, concurrent`; right-aligned widths, no leading zeros
- [ ] T4: `cargo test --workspace --lib` → exit 0
- [ ] T5: README compact example matches new output; breaking change called out
- [ ] All: the project gate (Rule A) passes on the final commit
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
