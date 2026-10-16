# Plan 03 — columnar compact stats + honest vLLM prefill — Tracker

## Overview

- Status: 14% complete (1/7 tasks)
- Start date: 2026-10-16
- Plan: `context/plans/03_columnar_stats_honest_prefill.md`
- Evidence: `captures/vllm_metrics_2026-10-16_*.json` (gitignored, RULES P2) for the backend payload;
  rendered compact blocks and `cargo test` output for the formatter; the corrected-rate table below.
  Test environment: live vLLM `0.33.0.dev20261015+g4d9a8d6` at `cosmo.lan:8700`
  (`model=cosmo-6000`, `max_model_len=262144`); formatter unit tests need no backend.

## Resume Instructions

1. Read the project's root `AGENTS.md`/`CLAUDE.md` (and the parent's, in a sub-project),
   `context/plans/RULES.md` (in full), then `03_columnar_stats_honest_prefill.md`, then
   `context/pitfalls.md` (both entries bind this plan: the gate command and the `AGENTS.md` symlink).
2. Environment: `cargo 1.99.1`. `RULES.md`'s literal gate command (`-- -D warnings`) is **rejected**
   by this cargo — run the gate as
   ```bash
   export RUSTFLAGS='-D warnings'
   cargo check --workspace --features influxdb && \
   cargo clippy --all-targets --features influxdb && \
   cargo fmt --check && \
   cargo test --workspace --lib
   ```
   `export` once (an env prefix on the first command only leaves the rest unchecked).
   Baseline at plan creation (2026-10-16): exit 0, **915 passed / 0 failed**.
   T1 and T5 also need the live backend reachable (`curl -m 3 http://cosmo.lan:8700/version`).
3. Ordering: **T1 before T4** (T4's denominator choice is T1's output — do not code the correction on
   the stale code comment). **T2 before T6** (T2 lands the red contract test `#[ignore]`d so the gate
   stays green; T6 removes the attribute and makes it pass — keep both outputs for the Evidence table).
   **T3 before T4 and T6** (both consume the `Option<f64>` rates). Then T5, T6, T7.
   One task per commit; T6 and T7 both touch `README.md`, so T6 does not touch it at all.
4. Commit per task as `[proxy][P3][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**; the scope tag is `[proxy]`, never `[llama-proxy]` (RULES Rule B addendum).
5. First task: also flip the plan metadata `Status` to `in-progress`, this SERIES row to
   `in-progress` under *Currently Active*, and `Start date` above — all in T1's commit.

## Open Unknowns

1. **What does `time_to_first_token_ms` exclude on this vLLM fork, and what other keys does the fork
   serve?** The whole correction's denominator depends on it and the current code comment describes a
   different build. **RESOLVED by T1, 2026-10-16** (`captures/vllm_metrics_2026-10-16_*.json`, live
   vLLM `0.33.0.dev20261015+g4d9a8d6`). `metrics` carries exactly six keys: `time_to_first_token_ms`,
   `generation_time_ms`, `queue_time_ms`, `mean_itl_ms`, `tokens_per_second`, `speculative_decoding`
   (the last is `null` on this build). There is **no** `prefill_time_ms` and **no**
   `prompt_tokens_per_second`, so ttft is the only prefill window the fork exposes and T4's
   denominator is correct as planned. `metrics.tokens_per_second` is
   completion/(ttft+generation_time) — an **end-to-end** rate that includes prefill, so the proxy must
   never read it as a decode rate. `queue_time_ms` measured 0.0077–0.0118 ms over n=5, i.e. noise:
   **not subtracted**. Whether ttft contains it is unmeasurable at that load and immaterial; if it
   does, the corrected rate is *understated*, not inflated (a too-large denominator deflates — the
   reasoning in the first draft of this task was inverted). Correctness of the discount itself, same
   9074-token request cold vs warm: cold `cached=0 ttft=666.28` → 13,619 (raw ≡ corrected); warm
   `cached=3200 ttft=449.55` → raw **20,184** vs corrected **13,066**. Over 4 warm repeats of that
   identical request the corrected figure holds at mean 12,858 (12,770–13,066, 2.3% spread) while the
   raw figure swings 19,727–20,184 — the inflation tracks cache depth, which is the bug's signature.
   Numerator-independent corroboration: those 3200 tokens moved from compute to cache between the cold
   and warm runs and ttft fell 216.72 ms, so the engine's own rate is 3200/0.21672 = **14,765 tok/s**
   — 8% from the corrected number, 46% from the raw one.
2. **The one-sided-absent shape of `tok=`** (`tok=186417/   n/a` vs a different filler). Decide in T6,
   record the choice and the test that pins it.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Establish what vLLM's `metrics` object actually contains | `captures/` | done | 6 payloads captured; **no** `prefill_time_ms` ⇒ ttft confirmed as T4's denominator; `queue_time_ms` ≈0.01 ms ⇒ not subtracted; corrected pp mean 12,858 (n=4) |
| 2 | T2: Write the columnar contract as a red test | `src/stats/formatter.rs` | pending | lands `#[ignore]`d; red output is T6's evidence |
| 3 | T3: Make an unmeasured rate absent, not zero | `src/stats/collector.rs`, `src/exporters/influxdb.rs`, `src/stats/formatter.rs` | pending | `prompt_tps`/`generation_tps` → `Option<f64>`; `total_tps`/`decode_tps_min` stay `f64` |
| 4 | T4: Correct the vLLM prefill rate for prefix-cache hits | `src/stats/collector.rs` | pending | five Context rows are the test data; none may exceed 15k |
| 5 | T5: Add the prefill plausibility canary | `src/exporters/mod.rs`, `src/proxy/handler.rs`, `src/config/mod.rs` | pending | first-WARN-then-1-in-100; counter exact |
| 6 | T6: Rewrite `format_compact` against the contract | `src/stats/formatter.rs` | pending | pinned head, free tail, `cache=` last |
| 7 | T7: Regenerate the docs from the rendered line and harvest the lesson | `README.md`, `CLAUDE.md`, `context/pitfalls.md` | pending | delete `README.md:334`; pin the README example with a test |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-16 | `RUSTFLAGS='-D warnings'` + check/clippy/fmt/test --workspace --lib | exit 0; 915 passed / 0 failed | 1 | baseline at plan creation; not a measurement |
| 2026-10-16 | `curl cosmo.lan:8700/{v1/models,props,version}` | vLLM 0.33.0.dev20261015+g4d9a8d6; `/props` 404 | 1 | proves the llama.cpp `timings` branch is dead on this box |
| 2026-10-16 | derived from 5 live log lines: `corrected_pp = (prompt−cache)/(prompt/prompt_tps)` | 6,421 / 11,748 / 12,491 / 12,797 / 13,021 tok/s | 5 distinct requests | **pre-implementation prediction**, computed before any code changed; T4 must reproduce it ±1%. Cluster 11.7k–13k + one 6.4k; the cache-3200 control row moves least (15,098 → 12,797), which is the signature of a correct discount. Not N repeats of one request — do not report it as a mean |
| 2026-10-16 | `curl -m 120 $B/v1/chat/completions -d @/tmp/opencode/req{1,2}.json` × 6 → `captures/vllm_metrics_2026-10-16_*.json` | `metrics` keys = `time_to_first_token_ms`, `generation_time_ms`, `queue_time_ms`, `mean_itl_ms`, `tokens_per_second`, `speculative_decoding`; **no** `prefill_time_ms`/`prompt_tokens_per_second`; `speculative_decoding: null` | 6 payloads (1 short-cold, 1 long-cold, 4 long-warm) | build 0.33.0.dev20261015+g4d9a8d6; settles Open Unknown 1 — ttft is the only prefill window the fork serves |
| 2026-10-16 | identical 9074-token request repeated warm, n=4 | corrected `pp = (prompt−cached)/ttft`: 12,770 / 12,776 / 12,822 / 13,066 | 4 | mean **12,858**, spread 296 = **2.3%**; the *raw* `prompt/ttft` over the same runs reads 19,727–20,184 (+55%), so the bug's magnitude tracks cache depth, not request cost |
| 2026-10-16 | same request on a first-touch prompt (cold prefix cache) | `cached=0`, `ttft=666.28ms`, raw ≡ corrected = 13,619 | **1** | cold is **not repeatable**: vLLM exposes no prefix-cache flush, so this is a single run, never a mean (RULES P1) |
| 2026-10-16 | `queue_time_ms` read off every capture | 0.0077 / 0.0086 / 0.0089 / 0.0096 / 0.0098 / 0.0118 ms | 6 | ≈ 0 ⇒ **not** subtracted. Whether ttft contains it is unmeasurable at this load, and if it does the rate is *understated*, not inflated — see Follow-ups |
| 2026-10-16 | derived marginal rate: 3200 cached tokens ÷ Δttft (666.28 − 449.55 ms) | engine compute rate **14,765 tok/s** | 1 cold/warm pair | the only check that does **not** depend on the numerator choice: 8% from the corrected figure, 46% from the raw one |
| | | | | |

*(T1 adds the captured payload rows here; T4 adds the post-fix rates from a real run, with the build
SHA — RULES P1: a number without its input, build and spread cannot be cited by a later plan.)*

## Notes / Deviations

**(Rule E)** Anything the plan asserted that turned out wrong, including negative and inconclusive
results. Pre-seeded so the next session does not re-litigate them:

- Plan 1's tracker records T4 as `pending` against a `5/5` Overview, and its commits are tagged
  `[llama-proxy]` against a `[proxy]` rule. Both are history (Rule B2) — do not "fix" them; this plan
  supersedes the claims, not the commits.
- Plan 1's prefill correction is **correct code on a branch this deployment never takes**. "The fix
  shipped and the numbers did not change" is expected, not a mystery.
- The compact line's `tps=` cell being one string for two independently-sourced numbers is the reason
  `tg= 0.00` can already appear on a vLLM body carrying only ttft (`collector.rs:503` sets one bool
  from either timing). T3/T6 address it; it is not a regression introduced here.
- Reviewers (both) recommended `prefill=`/`decode=` over `pp=`/`tg=`, `out=` alone over `tok=`, and 2
  decimals over 3 on the k-scale. The human chose `pp=`/`tg=`, `tok=A/B`, and `NN.NNNk` — those win;
  the objections are recorded in the plan's Rejected Claims and Context rather than silently dropped.

**Deviation** — the round-2 review (both reviewers, on the T1+T2–T4 design) overrode the plan in five
places. Each was accepted and is implemented as below, not as the plan's body text says:

- `has_timing_split` does **not** stay as written (plan T3). It becomes
  `prompt_tps.is_some() || generation_tps.is_some()`, because `influxdb.rs:213` writes the flag to the
  wire *as the truth about timing*, and a rate-derived definition is the only one that stays honest
  once the rates are `Option`. Tier selection in `format_compact` keeps reading this same flag — no
  timing at all ⇒ both rates `None` ⇒ tier 2 (floor + work), which is the right answer.
- `finite_rate` keeps its `f64` signature and its documented `0.0 == absent` contract for the
  millisecond fields; a **sibling** `finite_rate_opt -> Option<f64>` serves the rate fields. Changing
  the signature instead would have made `Some(finite_rate(inf)) == Some(0.0)` — the fabricated zero
  smuggled *inside* an `Option`, which is the exact defect T3 exists to remove. Reachable via a
  derived division overflow (`u64::MAX / f64::MIN_POSITIVE`).
- The plan calls `has_throughput_signal` and `format_compact` "untouched" by T3. **False** — neither
  compiles against `Option<f64>`. Both are edited in T3; `format_compact` renders a per-side `n/a`,
  never `unwrap_or(0.0)`, which would preserve the very `tg= 0.00` this plan kills.
- A **second fabrication site** the plan missed: `extract_timings`'s `predicted_per_second`
  `.unwrap_or(0.0)` (`collector.rs:361-365`, the llama.cpp branch) — `None` on missing.
- T4's `p <= c` guard is **three-way**, not two-way: `c == p` is *legitimate* (a full prefix-cache hit
  on an identical retry) ⇒ `None`; `c > p` is arithmetically impossible ⇒ keep the backend value
  verbatim **and** WARN+counter (reuses T5's first-then-1-in-100 pattern); `c == 0` ⇒ verbatim.
- The canary's default is **30000, not 15000**: the two populations are far apart (bug raw 48k–580k vs
  legitimate corrected max 13,619), so 30k is >2× the real max and <⅓ the bug floor. The human's 15k
  stays as **T4's acceptance assertion** (over 15k ⇒ suspect a bug), which is where it was always
  meant to bite. `raw_pp` is included in the WARN fields so a WARN line identifies itself as the old
  bug or a new one.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Extra compact cells: `reason=` (reasoning tokens are in pretty/json but not compact), `node=<host:port>` for per-node attribution, a `req_id` tail cell, a startup column legend | raised in review; each is a real operator gap, none is this plan's deliverable, and each widens the head | Plan 03 Open Question 6 → a later stats plan |
| Exercise the llama.cpp `timings` prefill branch against a real llama.cpp backend, or delete it | it is unexecuted on this box and still cites Plan 1's unresolved Open Unknown 1 (`collector.rs:372-380`) | Plan 03 Open Question 4; needs a llama.cpp backend |
| Flip `stats.max_plausible_prefill_tps` to a `const` if it is never tuned | both reviewers: a config field is a permanent surface for a constant operators will not touch | Plan 03 Open Question 2 |
| Reconcile the two `SERIES.md` dead `../context/opencode_claude_llama_notes.md` references while touching docs anyway | already a Standing Constraint; T7 is the nearest docs touch | human decision (SERIES Standing Constraints) |
| Saturate the backend and regress `time_to_first_token_ms` against `queue_time_ms` to settle whether vLLM's ttft includes queue time | T1 measured `queue_time_ms` at ~0.01 ms (n=6), so the question is **immaterial at this load and deliberately not subtracted** — but at saturation it would matter, and the direction is a rate that reads *low*, not high. The first draft of T1 reasoned this backwards; recorded so nobody "fixes" it the wrong way | a later load-testing plan; needs a controllable load generator |
| Surface `prompt_tokens_details.created_cache_tokens` separately from `cached_tokens` | neckbeard: it is a different quantity (tokens the engine *wrote* to cache this turn) and folding it into the discount would be wrong; it is operator-interesting on its own | a later stats plan |
