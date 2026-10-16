# Plan 01 — stats columnar + prefill fix — Tracker

## Overview

- Status: 20% complete (1/5 tasks)
- Start date: 2026-10-16
- Plan: `context/plans/01_stats_columnar_prefill.md`
- Evidence: unit test outputs in `src/stats/collector.rs` and `src/stats/formatter.rs`

## Resume Instructions

1. Read the project's root `AGENTS.md`/`CLAUDE.md`, `context/plans/RULES.md` (in full), then `01_stats_columnar_prefill.md`.
2. Environment: Rust toolchain with `cargo` available; no live backend required — unit tests use synthetic `timings` bodies.
3. Ordering: T1 (gate green) first, then T2 (prefill fix), then T3 (reorder + pad + affected tests), T4 (remaining tests), T5 (README). Commit per task as `[llama-proxy][P1][T<n>][<topic>] <summary>`, with this tracker's row in the same commit.
4. First task: also flip plan metadata `Status` to `in-progress`, the SERIES row to `in-progress` under *Currently Active*, and the tracker `Start date` — all in T1's commit. Later tasks keep the Currently Active line current.

## Open Unknowns

1. **Question: llama.cpp `prompt_per_second` semantics — does `prompt_n` include cache-hit tokens?** If not, the fix double-subtracts. *How we'll settle it*: check llama.cpp source or probe a backend with prefix caching before T2.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Make the project gate green at init | `src/stats/formatter.rs` | done | gate green: check/clippy/fmt/test all exit 0 |
| 2 | T2: Fix inflated prefill TPS from prefix-cache hits | `src/stats/collector.rs` | done | corrected rate = (prompt_n - cache_n) / prompt_ms * 1000 when cache hits present; unit tests pass |
| 3 | T3: Reorder and pad compact stats columns + update affected tests | `src/stats/formatter.rs` | done | new column order + right-aligned widths; affected formatter tests updated; full suite 915 passed |
| 4 | T4: Remaining stats tests (full suite) | `src/stats/formatter.rs`, `src/stats/collector.rs` | pending | depends on T3 |
| 5 | T5: Document breaking compact format change in README | `README.md` | pending | depends on T4 |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` — the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
| 2026-10-16 | cargo test stats::collector (synthetic timings: prompt_n=42, cache_n=10, prompt_ms=50.5) | corrected prompt_tps ≈ 633.66 (not 198.0); no-cache case verbatim 198.0 | 1 | N/A |
| 2026-10-16 | full gate: check/clippy/fmt/test --workspace --lib --features influxdb | all exit 0; 915 passed / 0 failed | 1 | N/A |

## Notes / Deviations

**(Rule E)** Record anything the plan asserted that turned out wrong, including negative and inconclusive results.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
