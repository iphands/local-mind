<!-- hands-plan:v1 2026-10-16 -->
# Plan NN — [Title] — Tracker

## Overview

- Status: 0% complete (0/N tasks)
- Start date: — (set when T1 starts)
- Plan: `context/plans/NN_name.md`
- Evidence: <where logs, captures or outputs for this plan live; the test environment used>

## Resume Instructions

1. Read the project's root `AGENTS.md`/`CLAUDE.md` (and the parent's, in a sub-project),
   `context/plans/RULES.md` (in full), then `NN_name.md`.
2. Environment: <anything a fresh session needs set up — env vars, services, test data>.
3. Ordering: <which task is next and why; constraints between tasks>.
4. Commit per task as `[proxy][P<n>][T<n>][<topic>] <summary>`, with **this tracker's row in the same commit**; the scope tag must match RULES.md Rule B.

## Open Unknowns

**(optional) Things that must be resolved before or during the work. Never delete one:
~~strike~~ it and add "RESOLVED (T2): <answer, with evidence>".**

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: … | `path/file.ext` | pending | |
| 2 | T2: … | `path/file.ext` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

**(required when the plan produces numbers or live checks; optional otherwise — remove it.
A claim not in this table has no provenance and cannot be cited by a later plan.)**

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|

## Notes / Deviations

**Anything the plan asserted that turned out to be wrong, and what was done instead
(Rule E). Negative and inconclusive results too. Be blunt — a wrong Key Fact
recorded honestly is worth more than a clean-looking tracker.**

## Follow-ups

**Everything left below the bar becomes a row here, naming this plan or a later one.**

| Follow-up | Why | Lands in |
|-----------|-----|----------|
