<!-- hands-plan:v1 2026-10-13 -->
# Plan NN — [Title] — Tracker

## Overview

- Status: 0% complete (0/N tasks)
- Start date: — (set when T1 starts)
- Plan: `context/plans/NN_name.md`
- Evidence: <where the artifacts for this plan live — `bench/bench-logs/<run>/`, the launcher's
  echoed `docker … serve …` line, a pasted boot report. Name the test environment: which image
  tag, which `VLLM_REF`, which model>

## Resume Instructions

1. Read the project's root `AGENTS.md`/`CLAUDE.md` (and the parent's, in a sub-project),
   `context/plans/RULES.md` (in full), then `NN_name.md`.
2. Environment: <env knobs the plan depends on (`SPEC_TOKENS`, `NVFP4_BACKEND`, `IMAGE_TAG`), plus any
   **Standing Constraint** in `SERIES.md` this plan touches — cite the bullet, do not restate it>.
3. Ordering: <which task is next and why; constraints between tasks>.
4. Commit per task as `[P<n>][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**.
5. Restore the system state at the end of the session (Rule P2.4) — power limits, launcher env,
   debug flags.

## Open Unknowns

<!-- (optional) Things that must be resolved before or during the work. Never delete one:
     ~~strike~~ it and add "RESOLVED (T2): <answer, with evidence>". -->

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: … | `path/file.ext` | pending | |
| 2 | T2: … | `path/file.ext` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

**Mandatory in this project** (Rule P1): its product is a number, and a number without provenance
gets cited. A claim not in this table has no provenance and cannot be cited by a later plan.

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|

Record the build every row was taken on: image tag, `ai.vllmcustom.vllm.ref`, and the launcher
env. `bench/bench-results.md` is gitignored — it is not evidence a later plan can cite; the jsonl
row or a named file under `bench/` is.

## Notes / Deviations

<!-- Anything the plan asserted that turned out to be wrong, and what was done instead
     (Rule E). Negative and inconclusive results too. Be blunt — a wrong Key Fact
     recorded honestly is worth more than a clean-looking tracker. -->

## Follow-ups

<!-- Everything left below the bar becomes a row here, naming this plan or a later one. -->

| Follow-up | Why | Lands in |
|-----------|-----|----------|
