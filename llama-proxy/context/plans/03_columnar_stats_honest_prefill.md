# Plan 03 — columnar compact stats + honest vLLM prefill

> **Status**: pending
> **Created**: 2026-10-16
> **Depends on**: Plan 1
> **Goal**: Make the compact stats line genuinely columnar (every head cell a fixed display width, conditional cells moved to a tail) and fix the vLLM-path prefill rate that divides cached tokens by a window that excludes them.
> **Agent**: implementation agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Plan 1 claimed the compact stats line was columnar and that prefill TPS was corrected for prefix-cache hits. Neither is true on the live backend. This plan makes the line actually columnar (and adds the test that would have caught Plan 1), and applies the prefix-cache correction to the **vLLM** path, which Plan 1 never touched.

**Deliverables**:
1. Prefill rate on the vLLM `metrics` path corrected to `(prompt_tokens - cached_tokens) / ttft * 1000` — your lines drop from 48k–580k tok/s to a ~6.4k–13k cluster.
2. `prompt_tps` / `generation_tps` become `Option<f64>`, so an unmeasured rate is **absent** rather than a fabricated `0` in InfluxDB, `n/a`-less in pretty, and `0.00` in the compact cell.
3. A rewritten `format_compact`: `pp= 13.021k`, `tg= 195.5`, `dur=   3163ms`, labelled `tok=186417/   498`, and `cache=` (with every other conditional cell) moved to a free-form tail.
4. `compact_head_columns_are_position_invariant` — a test that pins the head to a constant display width across value magnitudes and all three `tps` tiers. This test is what let Plan 01 ship a false "columnar" claim.
5. A plausibility canary on prefill: first-occurrence WARN (then 1-in-100) + `llama_proxy_stats_prefill_implausible_total` on `/proxy/metrics`.
6. Docs regenerated from a test-pinned sample; the stale `reprompt=N/2` claim removed.

**Estimated effort**: Medium (1–2 days)

---

## Scope

**In**: `src/stats/formatter.rs`, `src/stats/collector.rs`, `src/exporters/mod.rs`, `src/exporters/influxdb.rs`, `src/proxy/handler.rs` (counter render only), `src/config/mod.rs` + `config.yaml.default` (canary threshold), `README.md`, `CLAUDE.md` (the stats block), `context/pitfalls.md`.

**Out**:
- The **JSON** format's *shape*: it serializes the struct, so `Option` fields disappearing is the intended consequence (T4), but no field is renamed and no number is reformatted. Machine consumers get raw floats, not `13.021k`.
- The **pretty** box layout: it gains correct `n/a` behaviour from T4 and nothing else. Its frame width is already tested (`formatter.rs:705`).
- The **vLLM `timings` (llama.cpp) branch's** prefill formula — Plan 1's `collector.rs:381-393` stays as written. This plan *documents* it as dead code on the live backend; it does not delete or re-derive it (Open Question 4).
- Anything the **AnythingLLM shim**, reprompt engine, or context probe does.
- Adding new cells (`reason=`, `node=`, `id=`, a startup legend). Proposed in review, deliberately deferred — see Open Question 6.

If the work must cross this line, **stop** and record a blocker in the tracker instead of expanding scope.

---

## Context

Plan 1 (`completed/01_stats_columnar_prefill.md`) delivered a compact line that is not columnar and a prefill fix that never runs on this deployment. Both failures share one cause: **the plan's verification was string-`contains` tests plus one eyeballed log line, and the eyeballed line was from a backend the code path does not serve.** The user's live lines are the reproduction for both defects.

### Pre-Identified Bug/Issue

**(1) The backend is vLLM, so Plan 1's prefill correction is dead code.**

```bash
$ curl -s -m 3 http://cosmo.lan:8700/v1/models | head -c 400
{"object":"list","data":[{"id":"cosmo-6000","object":"model","created":1792126641,"owned_by":"vllm","root":"/models/nvidia/Qwen3.8-Flash-Next-FP4-Local","parent":null,"max_model_len":262144,
$ curl -s -m 3 http://cosmo.lan:8700/props | head -c 300
{"detail":"Not Found"}
$ curl -s -m 3 http://cosmo.lan:8700/version
{"version":"0.33.0.dev20261015+g4d9a8d6"}
```

No `/props` and `"owned_by":"vllm"` ⇒ `response.get("timings")` is never `Some`, so `extract_timings` always falls through to `extract_vllm_usage` (`collector.rs:425-440`).

**(2) The inflated prefill, verbatim from the live log** (`stats.format: "compact"`, 2026-10-16, trimmed to the columns under discussion):

```
tps=48334.16/146.16 dur=3144.0ms ctx=59043/262144  cache=51200 59043 201 stream=ok finish=stop   concurrent=1
tps=15098.44/ 89.78 dur=8371.0ms ctx=21001/262144  cache=3200 21001 622 stream=ok finish=tool_calls   concurrent=1
tps=208795.26/232.18 dur=2390.0ms ctx=105798/262144 cache=99200 105798 401 stream=ok finish=tool_calls   concurrent=1
tps=313431.57/247.16 dur=1182.0ms ctx=162906/262144 cache=156800 162906 101 stream=ok finish=tool_calls   concurrent=1
tps=579685.47/195.54 dur=3163.0ms ctx=186417/262144  cache=182400 186417 498 stream=ok finish=stop   concurrent=1
```

`prompt_tps` is computed as `prompt_tokens / ttft * 1000` (`collector.rs:493`) while `extract_usage`'s own comment (`collector.rs:289-292`) states `prompt_tokens` **already contains** the cached prefix that `cached_tokens` reports. ttft is the prefill window, and a prefix-cache hit is not recomputed inside it. So the numerator counts tokens the engine never processed.

`ttft` is recoverable from the line itself (`ttft = prompt_tokens / prompt_tps * 1000`), which makes the correction falsifiable against data already on disk:

| prompt | cache | reported pp | ⇒ ttft | computed = prompt−cache | **corrected pp** |
|---|---|---|---|---|---|
| 186417 | 182400 | 579685.47 | 321.6 ms | 4017 | **12,491** |
| 105798 | 99200 | 208795.26 | 506.7 ms | 6598 | **13,021** |
| 162906 | 156800 | 313431.57 | 519.7 ms | 6106 | **11,748** |
| 59043 | 51200 | 48334.16 | 1221.5 ms | 7843 | **6,421** |
| 21001 | 3200 | 15098.44 | 1390.9 ms | 17801 | **12,797** |

The fifth row is the control: it is barely cached (3200 of 21001), so its raw rate (15098) was never obviously wrong, and its corrected value (12797) lands inside the cluster the other four converge on. Five independent requests, one formula, one band — and the band is the user's stated expectation ("if we see prefill over 15k its probably a bug").

**(3) The line is not columnar.** `formatter.rs:219-233`:

```rust
format!(
    "model={}{} {} dur={:>6.1}ms {} {} {} {} {} finish={} {} {}",
    m.model, group_str, tps_str, m.duration_ms, context_str, cache_str,
    token_cell(m.prompt_tokens), token_cell(m.completion_tokens), stream_str, m.finish_reason, queue_str, concurrent_str)
```

`{:>6.2}` is a **minimum** width, so `tps=579685.47` blows straight through it; `model=` is unpadded; and `group_str`/`cache_str`/`queue_str`/`concurrent_str` are empty strings when absent, shifting every cell after them. `token_cell` (`formatter.rs:18-20`) prints a bare number with no key — hence the user's `cache=99200 105798 401`, where the last two numbers are `prompt_tokens` and `completion_tokens` wearing no label at all.

**(4) An unmeasured rate is published as a measured zero.** `prompt_tps` and `generation_tps` are plain `f64` (`collector.rs:37,39`), so they always serialize and always print. `has_timing_split` is one bool set from `prompt_ms > 0.0 || generation_ms > 0.0` (`collector.rs:503`), so a vLLM body carrying only `time_to_first_token_ms` yields a tier-1 line printing `tg=  0.00`, and `influxdb.rs:204`+`:213` write `prompt_tps`/`generation_tps` alongside `has_timing_split=true` — certifying the zero as a backend measurement.

### Why [Approach]

**Fix the numerator, do not suppress the cell.** Every input (`prompt_tokens`, `cached_tokens`, ttft) is backend-reported, so the corrected rate is a measurement of the tokens the engine actually processed; blanking it would delete the signal that produced this bug report — the cache-heavy long agent turns *are* the requests under investigation. Rejected alternative: print `pp=n/a` whenever `cache=` is present (reviewer suggestion) — rejected because it hides prefill throughput precisely when it is most interesting, and because it cannot distinguish "the prefix cache made this cheap" from "the engine is broken".

**Add the alignment test, do not eyeball the line.** Rejected alternative: change the format specifiers and read a rendered line, which is what Plan 1 did. Its tests (`formatter.rs:291,311,341-343,388-389`) assert substrings, so a line that is ragged on every real value passes.

**Keep the fixed head short and put conditionals in a tail.** A cell is columnar iff its width is constant; a conditional cell has no constant width. Rejected alternative: keep `cache=` where it is and pad around its absence — padding a cell that is usually absent adds permanent whitespace to buy nothing.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| Live backend is vLLM 0.33.0.dev, not llama.cpp | `owned_by":"vllm"`, `/props` → `{"detail":"Not Found"}` | `curl` above, 2026-10-16 |
| Therefore `extract_vllm_usage` is the only rate path in use | `collector.rs:425-440` | read 2026-10-16 |
| `prompt_tokens` subsumes `cached_tokens` on the OpenAI shape | `collector.rs:289-296` | read 2026-10-16 |
| `cached_tokens` is block-aligned ⇒ vLLM's `num_cached_tokens`, subsumed | every `cache=` in the log is a multiple of 16 (51200, 99200, 182400, 156800, 3200) while no `prompt` value is | log lines above, 2026-10-16 |
| Anthropic's `cached_tokens` is **disjoint**, not subsumed | `collector.rs:305-315` | read 2026-16; T4 pins it with a test |
| `prompt_tps`/`generation_tps` are `f64`, `has_timing_split` is one `bool` | `collector.rs:37,39,71` | read 2026-10-16 |
| InfluxDB writes both rates unconditionally + the split flag | `influxdb.rs:204,213,214` | read 2026-10-16 |
| A zeroed rate does **not** drop the sample | `has_throughput_signal` (`collector.rs:247-254`) admits `total_tokens > 0` | read 2026-10-16 |
| The project gate is **green** at plan creation | `RUSTFLAGS='-D warnings'` + check/clippy/fmt/test → exit 0, **915 passed / 0 failed** | run 2026-10-16 |
| `RULES.md`'s literal gate command is broken | `-- -D warnings` rejected by `cargo 1.99.1` | `context/pitfalls.md` "Toolchain", 2026-10-16 |
| `truncate_to_width` exists and is display-cell aware | `formatter.rs:239` | read 2026-10-16 |
| pretty has an alignment test; compact has none | `formatter.rs:705` vs `formatter.rs:291,311,341,388` | read 2026-10-16 |
| README advertises a cell that does not exist | `grep -rn 'reprompt=' src/` → **zero hits**; `README.md:334` claims it | run 2026-10-16 |
| README's compact example is not a real sample | `README.md:302` shows a tier-1 vLLM split at 57 prompt tokens | read 2026-10-16 |
| `format_metrics` is called exactly once per logged request | only non-test caller is `log_sample_and_should_export` (`exporters/mod.rs:40`), itself called once per request at `handler.rs:1821` and `streaming.rs:863` | read 2026-10-16 |
| What `time_to_first_token_ms` actually excludes on **this build** | — | **unconfirmed — confirm in T1** |

### Rejected Claims

- **"The prefill rate is genuinely 580k tok/s."** Rejected: it scales exactly with the cache-hit ratio, and correcting only the numerator puts all five requests inside a 6.4k–13k band that the near-uncached control row independently agrees with.
- **"Plan 1's fix handles this."** Rejected: it lives in the `timings` branch (`collector.rs:381-393`), which a vLLM backend never enters (Key Facts).
- **"The compact line is columnar because the widths are declared."** Rejected: `{:>6.2}` is a minimum, not a fixed width — the observed 6-digit rates are the counter-example.
- **"The `reprompt=N/2` cell exists; just re-order it."** Rejected: zero hits in `src/`. README is stale (Key Facts); T7 deletes the claim rather than implementing the cell.
- **"The k-scale marker `pp~=` should distinguish derived from measured."** Rejected, with the reviewer objection recorded: the corrected rate divides backend-reported tokens by a backend-reported window, which is not the invented 20/80 split the "never print a derived number as a measurement" rule targets. A second key was declined as surface growth for a distinction the README documents. Revisit if a corrected rate is ever shown beside a raw one.
- **"The line is formatted twice per request."** Rejected: single call site (Key Facts). No dedup work.

---

## Step-by-Step Tasks

### T1: Establish what vLLM's `metrics` object actually contains

**File**: no source change; captures go to `captures/` (gitignored, RULES P2), named `vllm_metrics_2026-10-16_<what-varies>.json`.

**What to do**: Every downstream task assumes the field set and the meaning of `time_to_first_token_ms`. Both come from a code comment (`collector.rs:469-472`) written about a different build; this backend is a dev fork (`0.33.0.dev20261015+g4d9a8d6`). Capture **≥3 real per-request `metrics` objects** — via `--dump ./debug-dumps` against a running proxy or a direct `curl` to `cosmo.lan:8700` — and record, verbatim, which keys are present: `time_to_first_token_ms`, `generation_time_ms`, `queue_time_ms`, `mean_itl_ms`, and anything this fork adds (`prefill_time_ms`, `prompt_tokens_per_second`, …). Then settle the denominator: if `queue_time_ms` is nonzero anywhere, decide whether ttft is `scheduled → first_token` (subtract nothing) or `arrival → first_token` (subtract `queue_ms`, already parsed at `collector.rs:487`). If the fork exposes a true prefill-duration field, prefer it over ttft and say so here.

**Before**: field semantics asserted by a comment; no captured payload on disk.

**After**: ≥3 captured payloads in `captures/`, the key list and the ttft verdict written into the tracker's Evidence table with the build string.

**Verify**:
```bash
ls -1 captures/vllm_metrics_2026-10-16_*.json | wc -l   # → >= 3
python3 -c "import json,glob;[print(sorted(json.load(open(f)).get('metrics') or {})) for f in glob.glob('captures/vllm_metrics_2026-10-16_*.json')]"
```
then the project gate (unchanged — no code touched; run it to prove the baseline is still green).

**Expected observation**: *(written before running — RULES)*
- **Confirms if**: the four keys the parser reads are present and `queue_time_ms` is ~0 on an idle box, i.e. the current denominator is defensible; corrected rates computed by hand from the payload land in the 6k–15k band of the Context table.
- **Refutes if**: a dedicated prefill-duration field exists (use it instead — T4 changes), or `queue_time_ms` is routinely large relative to ttft (T4 must subtract it), or `metrics` is absent entirely (then tier 1 is unreachable on this box and this plan's headline reproduction must be re-derived).
- **Within noise if**: individual corrected rates vary ±20% between runs — prefill rate depends on batch composition, so compare clusters, not pairs (RULES P1: name the spread, do not call a delta a win).

**Commit**: `[proxy][P3][T1][stats] capture live vllm per-request metrics payload`

### T2: Write the columnar contract as a red test

**File**: `src/stats/formatter.rs` (`mod tests`)

**What to do**: Add the test Plan 1 lacked. Build **≥4** samples that vary every magnitude at once — model names 10 and 40+ display cells (one with a CJK or wide char), `pp` 4-digit vs 6-digit, `tg` 2-digit vs 4-digit, `dur` 3-digit vs 6-digit, `ctx` 5/6 digits, `tok` 3/6 digits — across **all three rate tiers** (split / floor+work / no-output). Assert that the head — everything up to and including the `finish=` cell — has an identical `UnicodeWidthStr::width` in every sample and tier, and that the byte offset of each of `pp=`, `tg`, `dur=`, `ctx=`, `tok=`, `stream=`, `finish=` is identical across samples. Model the test on `pretty_rows_all_share_the_frame_width` (`formatter.rs:705`). Add it `#[ignore = "columnar rewrite lands in T6 — red until then"]` so the gate stays green now (RULES Rule A) and remove the attribute in T6. Also add, **not** ignored, the width helpers' unit tests (`pp` k-cell renders exactly 9 cells for a 6-digit rate and for `n/a`; `dur` cell renders 7 for 3163 and for 600000).

**Before**: no test can fail for a ragged line; `formatter.rs:291` asserts `contains("tps=200.50/ 42.50")`.

**After**: one ignored contract test asserting constant head width + fixed label offsets; unit tests for the two new cell renderers, passing.

**Verify**:
```bash
cargo test --lib stats::formatter -- --ignored   # → FAILS, red baseline, capture verbatim
cargo test --lib stats::formatter                # → passes (contract test is ignored)
```
then the gate.

**Expected observation**: the ignored test fails on the **current** formatter with a width mismatch naming the cell that moved (expected: `model=`, then the rate group); the new cell-renderer unit tests pass. This red output is the evidence T6 must invert — put it in the tracker's Evidence table.

**Commit**: `[proxy][P3][T2][stats] add red columnar contract test for the compact line`

### T3: Make an unmeasured rate absent, not zero

**File**: `src/stats/collector.rs`, `src/exporters/influxdb.rs`, `src/stats/formatter.rs`

**What to do**: Change `prompt_tps: f64` → `Option<f64>` (`collector.rs:37`) and `generation_tps: f64` → `Option<f64>` (`:39`), following the file's existing absence idiom (`prompt_tokens: Option<u64>`, `cached_tokens: Option<u64>`) with `#[serde(skip_serializing_if = "Option::is_none")]`. Leave `total_tps` and `decode_tps_min` as `f64`: they are derived, and their `0.0 == not measured` convention is documented (`collector.rs:46`, `:57`) — do not widen this task to them. Update the writers (`collector.rs:361-393`, `:490-502`) to assign `Some(..)` only through `finite_rate` and only when the input is usable, so `extract_vllm_usage` on a body with only `time_to_first_token_ms` leaves `generation_tps = None` instead of `Some(0.0)`. InfluxDB (`influxdb.rs:204,214`) switches to the existing conditional pattern (`influxdb.rs:222-227`) so an absent rate **omits the field** rather than writing `0`; keep `has_timing_split` on the wire unchanged for dashboard compatibility. `format_pretty` prints `n/a` (reuse the `token_cell` philosophy at `formatter.rs:16-20`). Keep `has_timing_split` set as today (`:503`) — T6 renders from the Options, not from the flag.

**Before**:
```rust
metrics.generation_tps = finite_rate(
    timings.get("predicted_per_second").and_then(|t| t.as_f64()).unwrap_or(0.0),
    "generation_tps", &metrics.model);
// influxdb.rs:204  .field("prompt_tps", metrics.prompt_tps)
```

**After**: `Some(finite_rate(..))` only when the source value is present and usable; `influxdb` writes the field only under `if let Some(..)`.

**Verify**:
```bash
cargo test --lib stats::            # → all pass
cargo test --lib exporters::influxdb -- --nocapture   # → the no-ttft/no-gen_ms case does NOT contain "generation_tps"
```
Then assert by hand: a vLLM-shaped body with `time_to_first_token_ms` and **no** `generation_time_ms` renders `tg=n/a` in pretty and omits `generation_tps` in the Influx line.

**Expected observation**: no `0`/`0.00` rate for a source the backend never sent; a body carrying both timing keys is unchanged (its rates are identical to before, to 2 dp). **Noise**: `has_throughput_signal` must still admit these samples — assert a cache-only sample is exported (it survives on `total_tokens > 0`, `collector.rs:254`).

**Commit**: `[proxy][P3][T3][stats] make unmeasured rates absent instead of zero`

### T4: Correct the vLLM prefill rate for prefix-cache hits

**File**: `src/stats/collector.rs` (`extract_vllm_usage`, `:446-519`)

**What to do**: At `collector.rs:490-495` the numerator is the full `prompt_tokens`. Subtract the cached prefix, mirroring the llama.cpp branch's shape (`:381-388`) but using this path's inputs — `metrics.cached_tokens` is already populated by `extract_usage`, which runs first (`from_response`, `:186-188`):

- When `cached_tokens == Some(c)`, `c > 0`, `prompt_tokens == Some(p)`, `p > c`, and the denominator is usable: `prompt_tps = Some(finite_rate((p - c) as f64 / denom_ms * 1000.0, ..))`.
- `denom_ms` is `ttft`, **or** `ttft - queue_ms` if T1 established that ttft includes queue wait. Record which in the comment, citing the captured payload — not the old code comment.
- Otherwise (`cached_tokens` `None`/`Some(0)`, or no usable denominator): the current verbatim `prompt_tokens / ttft` path, unchanged.
- `p <= c` is a **backend data error**, not a zero: keep the verbatim value, `tracing::debug!` the raw `p`, `c`, `ttft`, `queue_ms`. Do not saturate and do not blank (a `None` here is indistinguishable from "no split" on the wire). Reuse `finite_rate` so a subnormal denominator still cannot emit `inf`.
- Leave the `timings` branch (`:381-393`) untouched; extend its stale comment (`:372-380`, still citing Plan 1's unresolved Open Unknown 1) to state that the live backend is vLLM and this branch is unexercised here.

**Before**:
```rust
if let Some(ttft) = f("time_to_first_token_ms") {
    metrics.prompt_ms = ttft;
    if let Some(prompt_tokens) = metrics.prompt_tokens.filter(|p| *p > 0) {
        metrics.prompt_tps = finite_rate(prompt_tokens as f64 / ttft * 1000.0, "prompt_tps", &metrics.model);
    }
}
```

**After**: numerator is `prompt_tokens - cached_tokens` under the guards above; denominator per T1's verdict.

**Verify**:
```bash
cargo test --lib stats::collector -- --nocapture   # → all pass
```
New unit tests must use the five rows of the Context table as data — e.g. `prompt_tokens:186417`, `prompt_tokens_details.cached_tokens:182400`, `metrics:{time_to_first_token_ms:321.6, generation_time_ms:2546.0}` ⇒ `prompt_tps == Some(12491.0)` within 1% — plus: no cache ⇒ verbatim; `cached_tokens: Some(0)` ⇒ verbatim; `cached >= prompt` ⇒ verbatim + the debug line; `cached` present but no `ttft` ⇒ `None`.

**Expected observation**: the five reproduction rows render 6,421 / 11,748 / 12,491 / 12,797 / 13,021 tok/s (±1%), **none above 15,000**, and the control row (cache 3200) moves only from 15,098 to ~12,797 — i.e. the correction shrinks the inflated rows hard and the near-uncached row barely, which is the signature of a correct discount. **Refutes if** any corrected rate exceeds the raw one, or the near-uncached row moves as much as the heavily-cached ones. **Within noise if** repeat runs of the same request vary < ±5% (same input, same math — anything larger means the test is reading a different field).

**Commit**: `[proxy][P3][T4][stats] discount cached prompt tokens from vllm prefill rate`

### T5: Add the prefill plausibility canary

**File**: `src/exporters/mod.rs`, `src/proxy/handler.rs`, `src/config/mod.rs`, `config.yaml.default`

**What to do**: After T4 a prefill regression is silent again. Add a plausibility check on the rate as it is about to be logged, in `log_sample_and_should_export` (`exporters/mod.rs:38`) — the single choke point every logged sample passes (`handler.rs:1821`, `streaming.rs:863`), and it runs for all three formats. On `prompt_tps == Some(v)` with `v > threshold`: increment a process-global `AtomicU64` and WARN **unconditionally on the first occurrence, then every 100th** — copy the shape and the explanatory voice of `exporters/mod.rs:54-72` (`EXPORTS_SKIPPED_TOTAL` / `DROP_WARN_INTERVAL`), and put `model`, `group`, the raw `prompt_tokens`, `cached_tokens`, `prompt_ms` and the offending rate in the fields, so an operator can see *why* without a re-run. Render the counter on `/proxy/metrics` with HELP/TYPE text in the voice of its neighbours (`handler.rs:~1189-1230`, e.g. `llama_proxy_context_probe_timeout_total`), naming it `llama_proxy_stats_prefill_implausible_total` and stating it is per-process and resets on restart. Threshold: `stats.max_plausible_prefill_tps`, defaulting to `15000.0` (see Open Question 2 — reviewers want a `const`; the human chose config). `0.0` disables. Note in the HELP text and in README that with `stats.enabled: false` the canary never runs, so silence is not evidence.

**Before**: no threshold, no counter, no WARN for an implausible rate.

**After**: one counter on `/proxy/metrics`, first-then-1-in-100 WARN, config key with default.

**Verify**:
```bash
cargo test --lib exporters:: -- --nocapture      # → the over-threshold case increments and logs; under-threshold does neither
curl -s localhost:8066/proxy/metrics | grep prefill_implausible   # → llama_proxy_stats_prefill_implausible_total 0 at idle
```
Expected: the counter moves exactly once per over-threshold sample (exact counter, sampled log — the same split `exporters/mod.rs:20-22` documents) and the WARN fires on occurrence 1 and 100, not 2.

**Expected observation**: a synthetic `prompt_tps = 20_000` sample yields one WARN and `counter += 1`; the same sample at 14,000 yields neither. **Noise**: a global atomic under a parallel test harness can only be asserted by delta in one test (`exporters/mod.rs:386-390`) — follow that file's existing convention rather than racing it.

**Commit**: `[proxy][P3][T5][stats] warn and count implausible prefill rates`

### T6: Rewrite `format_compact` against the contract

**File**: `src/stats/formatter.rs` (`format_compact`, `:151-234`)

**What to do**: Replace the body with a **fixed head of pinned-width cells followed by a free-form tail**. A cell is columnar only if its width is constant, so: no conditional cell may sit inside the head, and every head cell is padded to a display-cell width (measure with `unicode-width`, as `box_row` already does at `formatter.rs:30-33`; never trust `{:<N}` on the model name — `{:<N}` counts chars, which is the CJK bug already fixed for pretty).

Head, in order, with widths in **display cells**:

| cell | spec | width | note |
|---|---|---|---|
| `model=` | `{:<16}` via `truncate_to_width(&m.model, 16)` (`:239`) | 22 | truncation is deliberate: a 50-cell GGUF name currently shifts every column |
| rate group | built per tier as one `String`, then padded to a constant | **23** | keeps `tg>=` from shifting everything after it |
| `dur=` | `{:>7.0}ms`, saturating above `9_999_999` | 13 | integer (the user's ask); 7, not 6, because a reprompt loop against `timeout_seconds: 300` overflows 6 digits and re-breaks every column to its right |
| `ctx=` | `{:>6}/{:>6}`, absent side renders `n/a` in the same 6 cells | 17 | always present — replaces `ctx=null` |
| `tok=` | `{:>6}/{:>6}` (`prompt_tokens`/`completion_tokens`) | 17 | labelled, closing the `105798 401` mystery; decide and test the one-sided-absent shape (`tok=186417/   n/a`) |
| `stream=` | `stream={:<7}` (`ok`/`trunc`/`stalled`/`gone`/`sync`) | 13 | bare `sync` (`:213`) becomes `stream=sync` — greppable and a column |
| `finish=` | `finish={:<10}` (`tool_calls` = 10) | 17 | `content_filter` (14) overflows by design: rare and self-annotating; empty/unknown renders `n/a`, never a blank cell |

Rate group per tier (right-aligned numbers, `n/a` inside the same width so the `k` never moves):
- tier 1: `pp={:>9.3}k` of `prompt_tps/1000` (user's `NN.NNNk`; 9 cells holds `579.685k`, the value that motivated the plan) + ` tg={:>6.1}` (user's `NNNN.N`).
- tier 2: `pp=      n/a tg>={:>6.1}` + `work={:>9.3}k` — `work=` moves to the **tail** (least trustworthy number, furthest right).
- tier 3: `pp=      n/a tg=   n/a`.
Do **not** clamp a rate to its field: a 1.2M tok/s prefill overflows the cell *and* trips T5's canary. That is the designed response; silently capping is fabrication.

Tail, no padding, in this order: `group=`, `work=` (tier 2/3 only), `queue=`, `concurrent=`, `cache=` **last** (the user's explicit ask, and correct: a cell present only on cache hits must sit after everything that has to line up).

**Before**: `formatter.rs:219-233` (quoted in Context 3).

**After**: pinned-width head + free tail; `contains`-only assertions replaced by the T2 contract test.

**Verify**:
```bash
cargo test --lib stats::formatter                # → the contract test now PASSES (remove its #[ignore])
cargo test --lib stats::formatter -- --ignored   # → no longer any ignored contract test (report 0 filtered/ignored for it)
```
Then **look at it** (Rule A4): render the five Context-table rows plus a narrow-value row and a CJK-model row through `format_compact` and paste the block into the tracker. Columns must line up by eye, not by assertion alone.

**Expected observation**: `… pp= 13.021k tg= 195.5 dur=   3163ms ctx=186417/262144 tok=186417/   498 stream=ok     finish=stop       concurrent=1 cache=182400`, with the `finish=` cell starting at the identical offset in every row including the narrow-value and tier-2 rows. **Refutes if** any row's `dur=`/`ctx=`/`tok=`/`stream=` starts at a different offset. **Within noise if** nothing here is timing-dependent — this is pure formatting, so any variance is a bug.

**Commit**: `[proxy][P3][T6][stats] pin compact head cells and move conditional ones to the tail`

### T7: Regenerate the docs from the rendered line and harvest the lesson

**File**: `README.md`, `CLAUDE.md`, `context/pitfalls.md`

**What to do**: (a) README compact section (`:296-334`): replace the fabricated example (`README.md:302`, Key Facts) with a line copied byte-for-byte from T6's rendered block, add a column table giving each cell's meaning — including `cache=N` = prompt tokens served from the prefix cache, i.e. the discount on `pp=` that T4 removed from the numerator — and rewrite the tier table for `pp=`/`tg=`/`work=`. **Delete the `reprompt=N/2` sentence (`README.md:334`)** — the cell does not exist. Document the breaking renames (`tps=A/B` → `pp=`/`tg=`; `sync` → `stream=sync`), the new `stats.max_plausible_prefill_tps`, and that the canary is inert when `stats.enabled: false`. (b) Make the example **pinned**: a test that asserts the README's compact sample equals `format_compact` on a fixed sample, so a doc line can never again describe output nothing produces. (c) Update the stats/`tps` block in `CLAUDE.md` (edit `CLAUDE.md`, never `AGENTS.md` — `context/pitfalls.md` "Editing the agent docs"). (d) Harvest per Rule D: a `pitfalls.md` entry for *the correction lived only in the llama.cpp branch while the deployment is vLLM* — symptom → why → do this → don't, tagged `[OBSERVED 2026-10-16]` — and a second for *a `contains` test cannot fail for a ragged line*.

**Verify**:
```bash
cargo test --lib stats::formatter -- --nocapture   # → the README-pin test passes
grep -n 'reprompt=' README.md                      # → no match
grep -n 'tps=' README.md CLAUDE.md                 # → no stale compact-key references (vLLM prose about the split is fine)
```
Expected: README's example is byte-identical to the pin test's rendered string.

**Commit**: `[proxy][P3][T7][docs] document the columnar compact line and the prefill discount`

---

## Critical Files

| File | Change | Priority |
|---|---|---|
| `src/stats/collector.rs` | vLLM prefill discount; `prompt_tps`/`generation_tps` → `Option<f64>` | P0 |
| `src/stats/formatter.rs` | Pinned-width head + free tail; contract test; cell renderers | P0 |
| `src/exporters/mod.rs` | Plausibility canary (first-then-1-in-100 WARN + counter) | P1 |
| `src/exporters/influxdb.rs` | Omit an absent rate instead of writing `0` | P1 |
| `README.md` | Regenerated example + column table + breaking renames | P1 |
| `src/proxy/handler.rs` | Render the new counter on `/proxy/metrics` | P2 |
| `src/config/mod.rs`, `config.yaml.default` | `stats.max_plausible_prefill_tps` | P2 |
| `CLAUDE.md`, `context/pitfalls.md` | Stats block; harvested lessons (Rule D) | P2 |

---

## Open Questions / Risks

1. **Question: does `time_to_first_token_ms` include queue wait on this vLLM fork?** The denominator is wrong by the queue fraction under saturation if it does, and the current log lines cannot tell (the `queue=` cell hides values < 1ms, `formatter.rs:194-197`). *How we'll settle it*: T1 captures ≥3 live payloads; if `queue_time_ms` is material, subtract the already-parsed `metrics.queue_ms` and record the payload as the citation.
2. **Question: config knob or `const` for the 15k threshold?** Both reviewers argued for a `const MAX_PLAUSIBLE_PREFILL_TPS` beside the formatter (operators never tune it; a config field is a permanent surface — default fn + docs + tests). The human chose a config field, so T5 ships one. *How we'll settle it*: if it is still unset at the next plan's close, flip it to a `const` — a one-line change with no other fallout.
3. **Risk: the 15k default is hardware-dependent and, post-T4, mostly fires on legitimately fast hardware.** Observed corrected cluster is 6.4k–13k on this box, so 15k is ~15% headroom; a smaller model or a faster GPU warns on every request forever. *Mitigation*: the WARN is first-then-1-in-100 while the counter stays exact, the threshold is configurable, `0.0` disables it, and the WARN carries the raw inputs so a false positive is diagnosable in one line.
4. **Risk: the llama.cpp `timings` branch keeps a correction that nothing on this box executes**, and its own comment still cites Plan 1's unresolved Open Unknown 1. *Mitigation*: T4 extends that comment with its status rather than deleting it (append-only ethos); deleting it needs a llama.cpp backend and is its own plan. Do not "harmonize" the two branches — the Anthropic path's `cached_tokens` is disjoint (`collector.rs:305-315`) and subtracting there under-counts; T3 adds a test asserting it is not.
5. **Risk: `Option`-ifying the two rates is a visible schema change** to the JSON format and the InfluxDB field set, and existing tests sweep these keys (`collector.rs:933`, `formatter.rs:771`, `influxdb.rs:395`). *Mitigation*: it is the intended fix (an absent measurement must not ship as `0`), `has_timing_split` stays on the wire, and T3 lists the exact files so no consumer is missed by accident.
6. **Question: which extra cells, if any?** Reviewers proposed a startup legend line, `reason=` for `reasoning_tokens` (present in pretty/json, absent from compact), `node=<host:port>` for multi-backend attribution, and a `req_id` tail cell. Each is a genuine gap; none is this plan's deliverable, and every added head cell widens the 148-cell line. *How we'll settle it*: recorded here, not implemented; promote to a follow-up row at close.
7. **Risk: head width ~148 display cells** before the tail — wider than today's ~110, so more horizontal scroll. *Mitigation*: `model=` at 16 is the first knob to turn (14 buys 2 cells and truncates `haiku-4-5-20251001`); record the chosen widths in the tracker so a later plan does not rediscover them.

**Never delete a question** — resolve it in place (~~strike~~ + "RESOLVED (Tn): …") or mark it `deferred — <reason>` when you defer it.

---

## Verification Checklist

Tick a box only against evidence produced on disk.

- [ ] T1: ≥3 payloads in `captures/`, key list + ttft verdict + build string in the tracker's Evidence table
- [ ] T2: `cargo test --lib stats::formatter -- --ignored` fails on the pre-T6 formatter with a named offending cell, output pasted in the tracker
- [ ] T3: a vLLM body with `time_to_first_token_ms` and no `generation_time_ms` renders `tg=n/a` in pretty and omits `generation_tps=` in the Influx line; a body with both keys renders rates identical to today to 2 dp
- [ ] T4: the five Context rows render 6421 / 11748 / 12491 / 12797 / 13021 tok/s (±1%) and **none exceeds 15000**; the control row (cache 3200) moves least
- [ ] T5: a synthetic 20k rate yields one WARN and `counter += 1`; 14k yields neither; `/proxy/metrics` serves `llama_proxy_stats_prefill_implausible_total`
- [ ] T6: the T2 contract test passes un-ignored, and the pasted rendered block has `dur=`, `ctx=`, `tok=`, `stream=`, `finish=` starting at identical offsets in every row (tier 1, tier 2, narrow, CJK)
- [ ] T6: `cache=` is the last cell of the line and `stream=sync` replaces bare `sync`
- [ ] T7: README's example equals the pin test's rendered string; `grep -n 'reprompt=' README.md` is empty
- [ ] T7: the two `pitfalls.md` entries exist on disk and were re-read after writing (Rule D4)
- [ ] All: the project gate passes on every task commit, using the `RUSTFLAGS='-D warnings'` form (`context/pitfalls.md` — the `RULES.md` command with `-- -D warnings` is rejected by cargo 1.99.1)
- [ ] All: RULES P1 respected — every number in the tracker names its input capture/build and its spread
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
