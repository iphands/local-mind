# fixrate — stop the compact log line from lying about tokens/sec

Status: plan, not implemented. Owner: whoever picks up T1.
Scope: `src/stats/**`, `src/exporters/**`, docs. **No request-path behavior change** —
every existing counter, fix, and reprompt path is untouched.

---

## 1. The defect, measured

The compact line's `tps=` cell falls back to a *derived* number when the backend
reports no prefill/decode split:

```rust
// src/stats/collector.rs:166-168
if duration_ms > 0.0 && metrics.total_tokens > 0 {
    metrics.total_tps = (metrics.total_tokens as f64 / duration_ms) * 1000.0;
}
```
```rust
// src/stats/formatter.rs:162-166
let tps_str = if m.has_timing_split { format!("tps={:.2}/{:.2}", m.prompt_tps, m.generation_tps) }
              else                    { format!("tps={:.2}tot", m.total_tps) };
```

`total_tps` = (prompt + completion) / wall clock. It divides prefill and decode —
rates that differ by ~250x on the operator's box — by one duration, and it is
printed under the key `tps=`, which reads as a measured speed.

Live capture from the operator's server (`cosmo.lan:8799` → vLLM 0.33.dev +
Anthropic adapter, Qwen3.8-Flash-Next-NVFP4):

```
prompt  compl    dur | printed tot | real decode | err
   6360    157  1073 |        6074 |         146 |  -16%
   5422   1430  9315 |         736 |         154 |  -12%
   5586   2355 15019 |         529 |         157 |  -10%
  31034    207  3384 |        9232 |          61 |  -65%   <- 31k-prefill turn
```

Ground truth from the engine's own `/metrics`: `generation_tokens 57040 /
request_decode_time 325.65s = ~175 t/s`; `prompt_tokens 2031412 /
request_prefill_time 44.47s = ~45.7k t/s`. A 1195-prompt / 8-completion probe
through `/v1/messages` printed `tps≈6046` for **8 generated tokens**.

Why there is no split on that path: `timings` is llama.cpp-only, and the
Anthropic adapter on that backend **strips vLLM's `metrics` object** — while
`/v1/chat/completions` on the same backend does return it:

```json
{"time_to_first_token_ms":122.96,"generation_time_ms":24.25,
 "queue_time_ms":0.008,"mean_itl_ms":3.46,"tokens_per_second":54.34}
```

So the collector genuinely has no timing source there, and the fallback fires.
The fallback is not new and its doc comments are honest; the **label** is not.

Two secondary findings, same root cause ("cache fields are ignored"):

- `context_used` on the Anthropic shape is `usage.input_tokens` alone
  (`collector.rs:334-337`). Anthropic's `input_tokens` **excludes** cached and
  cache-written tokens, so a cache-heavy session under-reports KV occupancy by
  exactly the cached amount. OpenAI's `prompt_tokens` *includes* them — the two
  shapes need opposite treatment.
- `timings.cache_n` is parsed into `api::openai::Timings` (`api/openai.rs:284`)
  but never reaches `RequestMetrics`.

### Getting the split on the OpenAI path

Start vLLM with `--enable-per-request-metrics` and read the `metrics` object shown above. The
producer doc (`vendor/vllm/docs/features/per_request_metrics.md` — the tree is gitignored, read it
with `rg --no-ignore`) lists three caveats:

- the flag is rejected when `--disable-log-stats` is also set;
- per-request metrics add non-negligible CPU overhead at high concurrency — benchmark first;
- `metrics` is `null` for `n > 1`, since timing can't be attributed across the n sequences (token
  usage stays accurate — the field is absent, never a fabricated zero).

## 2. Design: three tiers, best available wins

| Tier | Condition | Number | Key in compact line | Nature |
|---|---|---|---|---|
| 1 | backend reported a split (`timings` / vLLM `metrics`) | `prompt_tps` / `generation_tps` | `tps=A/B` | measured (unchanged) |
| 2 | always, when the backend counted output tokens | `completion / wall` | `tps>=X` | **floor**, wall-bounded, always under-states |
| 3 | always | `(prompt+completion) / wall` | `work=Y` | cost rate, **not speed** |

Decisions and their reasons:

- **D1 — `>=` not a new word.** Tier 2 keeps the greppable `tps` key and states
  its direction in the operator's own notation. `tps>=146.32` says "at least
  this fast", which is exactly what `completion/wall` means.
- **D2 — `total_tps` keeps its name and its formula.** It is already an InfluxDB
  field (`exporters/influxdb.rs:205`) and dashboards aggregate on it. Only the
  *log key* changes (`tot` → `work=`) and the *doc comment* changes. No wire
  churn, no silent redefinition.
- **D3 — do not subtract cached tokens from `work=`.** `total_tps` stays
  reconcilable against `sum(total_tokens)`. Instead the plan surfaces `cache=N`
  so the reader can discount the number themselves.
- **D4 — `has_timing_split` stays `false` on tiers 2 and 3.** It means "the
  backend reported a split", and the floor is not one. Pinned by test.
- **D5 — tier 2 is populated even on tier-1 samples** so the InfluxDB field is
  never a sparse hole; the formatter just prefers tier 1.

Explicitly **out of scope** (see T7): letting `/v1/messages` pass through
verbatim when the backend is Anthropic-native, which is the only change that
yields a *real* TTFT/decode split on the Claude Code path.

---

## T1 — collector: new fields + cache extraction

File: `src/stats/collector.rs`

### T1.1 New fields on `RequestMetrics` (insert after `has_timing_split`, line 49)

```rust
    /// Lower bound on decode speed: completion_tokens / wall clock. Always
    /// computable once the backend counted output tokens, and the best number
    /// available when it reports no split at all -- but the denominator still
    /// contains prefill, network and (in fake mode) synthesis, so it
    /// UNDER-states the engine's decode rate and must never be printed as a
    /// measurement. The compact line renders it as `tps>=`. Populated on
    /// tier-1 samples too, so the exporter field is never sparse.
    pub decode_tps_min: f64,
    /// Prompt tokens served from the engine's prefix cache. Subsumed by
    /// `prompt_tokens` on the OpenAI/vLLM shape and by `timings.cache_n` on
    /// llama.cpp; DISJOINT from `input_tokens` on the Anthropic shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    /// Prompt tokens written to the prefix cache this request. Only the
    /// Anthropic shape reports it (`cache_creation_input_tokens`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
```

- Add the three initializers to `RequestMetrics::new()` (`:101-137`):
  `decode_tps_min: 0.0, cached_tokens: None, cache_creation_tokens: None`.
- Rewrite the `total_tps` doc comment (`:44-46`) — it currently calls it "the
  only throughput figure available", which is what let a cost rate read as
  speed:
  ```rust
    /// (prompt + completion) / wall clock. A COST rate: one duration covering
    /// prefill and decode together. It is not a speed and must never be printed
    /// under a bare `tps=` key -- the compact line shows it as `work=`. Kept
    /// because it is the only throughput figure that spans every token the
    /// request moved, and it reconciles with `sum(total_tokens)` in InfluxDB.
  ```

### T1.2 Compute the floor in `from_response` (after line 168)

```rust
        // Tier 2: the honest floor. The wall clock still contains prefill, so
        // this under-states decode speed; it can never over-state it, which is
        // the property `total_tps` lacked.
        if duration_ms > 0.0 {
            if let Some(completion) = metrics.completion_tokens.filter(|c| *c > 0) {
                metrics.decode_tps_min = finite_rate(
                    completion as f64 / duration_ms * 1000.0,
                    "decode_tps_min",
                    &metrics.model,
                );
            }
        }
```

Placement matters: it sits **after** `extract_timings` (`:163`) so the
timings-based token-count fallback (`:315-328`) has already run, and it reuses
the existing `finite_rate` gate (`:269`) for the hostile-body class.

`has_throughput_signal` (`:203-207`): **no change.** Add one comment line noting
that `decode_tps_min > 0` implies `completion_tokens > 0` implies
`total_tokens > 0`, so the existing disjunction already covers it and a fourth
term would be dead code.

### T1.3 Cache fields in `extract_usage` (`:229-262`)

Inside the OpenAI branch (after `:241`):
```rust
            metrics.cached_tokens = usage
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(|t| t.as_u64());
```
Inside the Anthropic branch (`:245-248`, where `input` is in scope), plus the
KV-occupancy correction:
```rust
            metrics.cached_tokens = usage
                .get("cache_read_input_tokens")
                .and_then(|t| t.as_u64());
            metrics.cache_creation_tokens = usage
                .get("cache_creation_input_tokens")
                .and_then(|t| t.as_u64());
            // Anthropic's input_tokens EXCLUDES cache tokens, so the engine's
            // KV occupancy is the sum. Set here, where the shape is known; the
            // generic fallback in extract_timings (T1.5) defers to a value that
            // is already set.
            metrics.context_used = Some(
                input
                    .saturating_add(metrics.cached_tokens.unwrap_or(0))
                    .saturating_add(metrics.cache_creation_tokens.unwrap_or(0)),
            );
```
Both reads go through `as_u64()` only — a negative or fractional literal is
treated as absent, matching how every other count in this file is read.

### T1.4 `timings.cache_n` in `extract_timings` (at `:308-310`)

```rust
        metrics.cached_tokens = timings.get("cache_n").and_then(|t| t.as_u64());
```
Order note: `extract_timings` runs after `extract_usage` (`:161-163`), so on a
body that somehow carries both spellings `timings.cache_n` wins. That is the
right precedence — llama.cpp is authoritative for its own extension, and vLLM
never sends `timings`.

### T1.5 Make the `context_used` fallback defer (at `:334-337`)

```rust
        // Only when the shape-specific code above did not already set it: on
        // the Anthropic shape `prompt_tokens` alone omits the cached prefix.
        if metrics.context_used.is_none() {
            if let Some(prompt_tokens) = metrics.prompt_tokens.filter(|p| *p > 0) { ... }
        }
```
OpenAI/llama.cpp behavior is byte-for-byte unchanged (nothing else sets
`context_used` before this point).

### T1.6 Tests (`#[cfg(test)] mod tests`, append near `:1190`)

| Test name | Body / args | Assertions |
|---|---|---|
| `decode_tps_min_is_a_wall_bounded_floor` | `usage` 6360/157, no `timings`, dur 1073.0 | `(decode_tps_min - 146.32).abs() < 0.01`; `(total_tps - 6073.63).abs() < 0.1`; `!has_timing_split`; `prompt_tps == 0.0` |
| `decode_tps_min_absent_without_completion_tokens` | `usage` with `completion_tokens: null`, dur 500 | `decode_tps_min == 0.0` |
| `decode_tps_min_survives_the_timings_token_fallback` | no `usage`, `timings {prompt_n:1000, predicted_n:400, predicted_ms:2000}`, dur 2000 | `decode_tps_min ≈ 200.0`; `completion_tokens == Some(400)` |
| `decode_tps_min_zero_duration_stays_zero` | completion 100, dur 0.0 | `decode_tps_min == 0.0` |
| `decode_tps_min_clamps_hostile_division` | `completion_tokens: u64::MAX`, `duration_ms: f64::MIN_POSITIVE` | `!is_infinite()`, `== 0.0` (mirrors `derived_rate_overflow_is_clamped_to_not_reported`) |
| `anthropic_cache_fields_are_extracted` | `usage {input_tokens:100, output_tokens:20, cache_read_input_tokens:30000, cache_creation_input_tokens:500}` | `cached_tokens == Some(30000)`; `cache_creation_tokens == Some(500)`; `context_used == Some(30620)`; `total_tokens == 120` |
| `openai_cached_tokens_do_not_inflate_context_used` | `prompt_tokens:5000`, `prompt_tokens_details.cached_tokens:4096`, no timings | `cached_tokens == Some(4096)`; `context_used == Some(5000)` — **pins the asymmetry: not 9096** |
| `llama_cache_n_populates_cached_tokens` | `timings {prompt_n:2000, cache_n:1500, ...}` | `cached_tokens == Some(1500)`; `context_used == Some(2000)` |
| `json_format_omits_cache_fields_when_absent` (formatter) | default metrics | serialized JSON has no `cached_tokens` key, does have `decode_tps_min` |

Update existing tests:
- `test_no_timings_reports_total_only` (`:1188-1203`): add `decode_tps_min ≈ 84.34`
  (146 tok / 1731 ms) and keep `total_tps ≈ 22753.32` + `prompt_tps == 0.0`.
- `test_timings_present_keeps_split` (`:1206-1222`): add `decode_tps_min > 0.0`
  (D5) and keep `has_timing_split`.
- `test_request_metrics_serialize` (`:864`): confirm the new `f64` appears; no
  cache keys.

---

## T2 — compact format: three tiers in one cell

File: `src/stats/formatter.rs`, `format_compact` — replace the block at
**`:158-166`** and add one cell to the `format!` at `:179-205`.

```rust
    // Three tiers, best available wins (see fixrate.md §2). A backend-reported
    // split is the only measured pair. With none, print the honest FLOOR on
    // decode speed and the cost rate, and mark the floor with `>=` so a bare
    // speed reading is impossible. Never invent a split: that was the 20/80 bug.
    let tps_str = if m.has_timing_split {
        format!("tps={:.2}/{:.2}", m.prompt_tps, m.generation_tps)
    } else if m.decode_tps_min > 0.0 {
        format!("tps>={:.2} work={:.2}", m.decode_tps_min, m.total_tps)
    } else {
        format!("tps=n/a work={:.2}", m.total_tps)
    };

    // Lets a reader discount `work=` for a prompt the engine never recomputed.
    let cache_str = m
        .cached_tokens
        .filter(|c| *c > 0)
        .map(|c| format!(" cache={c}"))
        .unwrap_or_default();
```

Add `cache_str` to the format string immediately after the `tps_str` cell:
`"model={}{} tokens={}/{} {}{} {} {} finish={} dur={:.1}ms{}{}"`.

Resulting lines:

```
model=cosmo-6000 tokens=6360/157 tps>=146.32 work=6073.63 ctx=6360/262144 sync finish=tool_use dur=1073.0ms concurrent=1
model=cosmo-6000 tokens=100/50   tps=1698.07/33.13 ctx=100/262144 sync finish=stop dur=1676.0ms concurrent=1
model=cosmo-6000 tokens=5000/800  tps>=210.53 work=1184.21 cache=4096 ctx=5000/262144 sync finish=stop dur=4900.0ms
```

### T2 tests (same file)

- **Rewrite** `test_format_compact_without_timing_split` (`:266-279`): set
  `m.decode_tps_min = 50.0 / 1.2` alongside the existing `total_tps = 125.0`,
  then assert `contains("tps>=41.67 work=125.00")`,
  `!contains("tot")`, `!contains("/0.00")`.
- **Rewrite** `test_no_split_is_invented_from_duration` (`:283-299`): keep the
  `!contains("113344")` guard, replace `contains("tps=22753.32tot")` with
  `contains("work=22753.32")` and add `contains("tps>=84.34")`.
- **New** `compact_cache_cell_appears_only_when_nonzero`: `cached_tokens =
  Some(0)` → no `cache=`; `Some(4096)` → `cache=4096`; `None` → absent.
- **New** `compact_split_path_prints_no_floor`: tier-1 sample must not contain
  `work=` or `>=` (guards against the two tiers leaking into one line).
- **Check, likely unchanged**: `test_format_compact` (`:252`),
  `test_format_compact_all_fields` (`:591`), `absent_token_counts_print_n_a`
  (`:361`).

---

## T3 — pretty format + JSON + InfluxDB wire

### T3.1 `pretty_performance_lines` (`formatter.rs:62-74`), no-split branch

```rust
    } else {
        box_row(&format!(
            "   Decode (floor):    {:8.2} tokens/sec (wall-bounded)",
            m.decode_tps_min
        )) + &box_row(&format!(
            "   Work rate:         {:8.2} tokens/sec (all tokens/wall)",
            m.total_tps
        )) + &box_row("   (no backend split: decode is a floor, work is cost)")
    }
```
Label columns are 22 cells wide, matching `   Prompt Processing: ` and
`   Generation:        `, so the value column stays aligned. Worst case with an
11-char `{:8.2}` value: 63 / 62 / 54 cells, inside `BOX_INNER = 66`
(`formatter.rs:27`) — verified, because `pretty_rows_all_share_the_frame_width`
(`:614`) fails on any overflow.

### T3.2 JSON (`formatter.rs:140`) — no code change

`RequestMetrics` derives `Serialize`; `decode_tps_min` appears always, the two
cache fields are `skip_serializing_if = "Option::is_none"`, so existing JSON
consumers see only an additive key. Update the README sample (T6).

### T3.3 InfluxDB (`exporters/influxdb.rs:203-217`)

```rust
        .field("decode_tps_min", metrics.decode_tps_min)   // always: never a sparse hole
```
and in the conditional block near `:224-231`:
```rust
    if let Some(cached) = metrics.cached_tokens { point = point.field("cached_tokens", cached as f64); }
    if let Some(wrote) = metrics.cache_creation_tokens { point = point.field("cache_creation_tokens", wrote as f64); }
```
`total_tps`, `prompt_tps`, `generation_tps`, `has_timing_split` are unchanged,
so every existing dashboard keeps working. The correct speed query is now
documented as `mean("generation_tps") filter has_timing_split` for measured
speed and `mean("decode_tps_min")` for the floor.

### T3.4 Tests

- `influxdb.rs` (`:305`, `:650` fixtures): extend the existing `build_data_point`
  assertions to cover `decode_tps_min` present, `cached_tokens` omitted when
  `None` and present when `Some`.
- `formatter.rs`: `test_format_pretty_basic` (`:416`) keeps the split path;
  **new** `pretty_no_split_names_both_numbers` asserts the output contains
  `Decode (floor):`, `Work rate:`, `wall`, and does **not** contain the string
  `Total throughput` (the retired label).

---

## T4 — log the stats line under the stats target

File: `src/exporters/mod.rs`, `log_sample_and_should_export` (`:38`).

Both `tracing::info!` calls (`:42`, `:44`) and the skip `tracing::warn!` (`:55`)
get `target: "llama_proxy::stats"`. Today the line prints as
`INFO llama_proxy::exporters:` purely because the macro happens to live in that
module — the message is a stats line, and an operator looking for it greps
`exporters` or filters `exporters=warn` and finds nothing relevant.

Migration note to include in README: `RUST_LOG=llama_proxy::exporters=info`
still captures exporter internals (its own `warn!`s at `:146,159,178` keep the
default target) but no longer the per-request line; use
`RUST_LOG=llama_proxy::stats=info` or the existing `--hide-requests` flag.

Test: **new** `log_target_names_stats_not_exporters` in `exporters/mod.rs`, using
the repo's existing log-capture harness pattern — a `CaptureWriter` behind
`tracing_subscriber::fmt().with_ansi(false).with_max_level(TRACE)` plus
`tracing::subscriber::set_default`, exactly as `src/backends/node.rs:405-440`
does it (also `src/augment.rs:1104`, `src/fixes/registry.rs:983`). Assert the
captured text contains `llama_proxy::stats:` and does NOT contain
`llama_proxy::exporters:` for the per-request line. Target display is on by
default in `fmt`, so no extra builder call is needed. This is the fourth copy of
`CaptureWriter`; keep it local to the test module like the other three rather
than starting a shared harness in this task.

---

## T5 — docs

- `README.md`: pretty sample (perf rows `:244-245`, block `:236-252`) and
  compact sample (`:278`). Add a no-split compact example line and a short
  "Three throughput numbers" table mirroring §2 of this file, including the
  migration note that `tps=<n>tot` is now `work=<n>`.
- `config.yaml.default`, stats block (`:145-148`): one paragraph — what `tps=`,
  `tps>=`, `work=`, `cache=` mean, and that a split needs llama.cpp `timings`
  or vLLM `--enable-per-request-metrics`.
- `CLAUDE.md` (note: `AGENTS.md` is a symlink to it — edit once, both update):
  - `stats/collector.rs` bullet (`:155`): mention `decode_tps_min` and the cache
    fields.
  - "New Metric" checklist (`:278-283`): step 2 currently names
    `from_response()`/`merge_stream_stats()`; add "declare the field in
    `RequestMetrics::new()` and gate non-finite values with `finite_rate()`".
  - Add one bullet under "Honest Local Answers" (`:75`): a derived rate must
    carry its direction in the key (`>=`), never a bare `=`.

---

## T6 — verification

1. `cargo test --workspace --lib` (justfile:6) — full green, including
   `pretty_rows_all_share_the_frame_width`.
2. `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`.
3. Live, against a vLLM + Anthropic-adapter backend (no split available):
   ```bash
   curl -sS -X POST http://localhost:8799/v1/messages -H 'content-type: application/json' \
     -H 'anthropic-version: 2023-06-01' \
     -d '{"model":"X","max_tokens":200,"messages":[{"role":"user","content":"write a haiku"}]}'
   ```
   Expect `tps>=<n> work=<m>`, and `tps>=` within ~15% of
   `vllm:generation_tokens_total / vllm:request_decode_time_seconds_sum`.
4. Live, OpenAI path on the same backend (split available):
   expect `tps=A/B` — proving tier 1 still wins and the fallback did not become
   the default path.
5. `/proxy/metrics` unchanged; no new endpoint counters in this plan.

## Acceptance criteria

- No code path prints `(prompt+completion)/wall` under a key that implies a
  measured speed.
- Every logged line carries at least one number that is either backend-measured
  or explicitly marked as a bound.
- `has_timing_split` semantics unchanged; pinned by test.
- `total_tps` wire value unchanged (assert `≈ 22753.32` in the pre-existing
  regression tests).
- `cached_tokens` never changes `context_used` on the OpenAI/llama.cpp shapes
  (test `openai_cached_tokens_do_not_inflate_context_used`).

## T7 — out of scope, but the only real fix for the Claude Code path

`handler.rs:925` refuses Anthropic streaming outright:

```rust
let allow_stream = streaming_mode.is_passthrough() && !is_anthropic_api;
```

The stated reason (`:927-935`) is "there is no OpenAI->Anthropic SSE translator
yet" — true, and irrelevant when the **backend** already speaks Anthropic SSE,
which the operator's adapter does (verified: `message_start` /
`content_block_delta` / `message_stop` on `/v1/messages` with `stream:true`).
A per-node `native_anthropic: true` that permits verbatim passthrough would let
the proxy timestamp first and last token itself and report a real TTFT + decode
rate with no backend change.

Not bundled here because it changes behavior, not labeling: on a passthrough
path fixes only DETECT (no repair) and reprompt cannot run at all, so it must be
opt-in per node and needs its own plan. Track separately.

## Order of work

T1 → T2 → T3 → T4 → T5 (T6 is the gate, T7 is a separate plan), each task with
its tests in the same commit.

T1 lands cleanly on its own: nothing printed changes for the compact and pretty
formats, the only visible effect is one additive key (`decode_tps_min`) in
`stats.format: json` output and one new InfluxDB field once T3.3 ships — so it
is safe to land first if the formatter change wants a separate review.

---

# 5. Implementation record (T1-T5 applied 2026-09-26)

T1-T5 are implemented. Deviations from the plan, all of them forced by what the
code actually turned out to be:

1. **`total_tps` is now gated through `finite_rate` too.** Writing
   `decode_tps_min_clamps_hostile_division` surfaced the same overflow on the
   *existing* derived rate: `u64::MAX` tokens over `f64::MIN_POSITIVE` ms makes
   `total_tps` `+inf`, which serde_json emits as a literal `inf` and InfluxDB
   400s for the whole batch. `finite_rate` exists for exactly this; the formula
   (D2) is untouched, only its exit is gated. The hostile-division test asserts
   BOTH rates, not just the new one.
2. **`json_omits_cache_fields_when_absent` landed in `formatter.rs`** as
   `json_format_carries_the_floor_and_omits_absent_cache_fields` (that is where
   `format_json` lives), with a companion serialization-level assertion in
   collector's `serialized_sample_always_carries_the_floor_and_only_reports_cache_counts`.
   There is no `test_json_format_valid_json` in this repo to extend.
3. **T2.1's new test name was folded into the existing
   `test_format_compact_without_timing_split`** -- that test is the one that
   asserted `tps=125.00tot`, so rewriting it in place keeps the retired-key
   assertion next to the memory of why the key was retired.
4. **T4.2's capture test needs `crate::fixes::pin_interest_cache_for_tests()`**
   (documented requirement for every scoped-capture test in this repo:
   tracing-core caches per-callsite `Interest` process-wide) and an explicit
   `EnvFilter::new("off,llama_proxy::stats=info")`, which is what makes the
   "unrelated event from this module must not survive" assertion mean anything.
5. **The Anthropic `context_used` sum moves the fallback, it does not remove
   it.** `extract_usage` now writes the value for that shape, so the generic
   fallback in `extract_timings` is guarded rather than dead. The stale message
   in `anthropic_usage_without_output_tokens_yields_none_completion` ("the
   context_used fallback still fires") was updated, since the value it asserts
   (Some(10)) now comes from the shape-specific branch.
6. **`pretty_performance_lines` no-split branch is two value rows**, so the
   box's fixed-width invariant test (`pretty_rows_all_share_the_frame_width`)
   was extended to render the no-split branch too, with an 11-char value to
   prove the budget at the wide end.

**Not done here: `cargo test`.** This machine has no Rust toolchain on PATH (no
`cargo`, no `rustup`, no `~/.cargo`), so `just ci` / `cargo test --features
influxdb` could not be run. Every numeric assertion in the new tests was
computed by hand against the formulas (146.32, 6073.63, 84.34, 41.67, 22753.32)
and the box-row widths were verified against `BOX_INNER = 66`, but the code is
UNVERIFIED until someone runs T6 on a machine with a toolchain.
