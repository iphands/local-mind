# Plan 03 — columnar compact stats + honest vLLM prefill — Tracker

## Overview

- Status: 0% complete (0/7 tasks)
- Start date: — (set when T1 starts)
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
   different build. ~~strike~~ when T1 pastes the captured key list + a queue-vs-ttft reading here.
2. **The one-sided-absent shape of `tok=`** (`tok=186417/   n/a` vs a different filler). Decide in T6,
   record the choice and the test that pins it.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Establish what vLLM's `metrics` object actually contains | `captures/` | pending | gate baseline green at plan creation: exit 0, 915 passed |
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

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Extra compact cells: `reason=` (reasoning tokens are in pretty/json but not compact), `node=<host:port>` for per-node attribution, a `req_id` tail cell, a startup column legend | raised in review; each is a real operator gap, none is this plan's deliverable, and each widens the head | Plan 03 Open Question 6 → a later stats plan |
| Exercise the llama.cpp `timings` prefill branch against a real llama.cpp backend, or delete it | it is unexecuted on this box and still cites Plan 1's unresolved Open Unknown 1 (`collector.rs:372-380`) | Plan 03 Open Question 4; needs a llama.cpp backend |
| Flip `stats.max_plausible_prefill_tps` to a `const` if it is never tuned | both reviewers: a config field is a permanent surface for a constant operators will not touch | Plan 03 Open Question 2 |
| Reconcile the two `SERIES.md` dead `../context/opencode_claude_llama_notes.md` references while touching docs anyway | already a Standing Constraint; T7 is the nearest docs touch | human decision (SERIES Standing Constraints) |
