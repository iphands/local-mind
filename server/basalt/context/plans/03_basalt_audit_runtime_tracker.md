# Plan 03 — Basalt audit, runtime half — Tracker

## Overview

- Status: 0% complete (0/5 tasks)
- Start date: — (set when T1 starts)
- Plan: `context/plans/03_basalt_audit_runtime.md`
- Evidence: `context/audit/runtime.md` (T4) and raw output under `context/audit/wave-runtime/`; captures in a
  durable path chosen in T1, **never `/tmp`** (a reboot wiped Plan 02's batch on 2026-11-05)

## Resume Instructions

1. Read the project's root `AGENTS.md`/`CLAUDE.md`, `context/plans/RULES.md` (in full), then
   `03_basalt_audit_runtime.md`. **Pending plan = hypothesis:** re-check its Key Facts before T1.
2. Environment: docker as yourself (no `sudo`); one server on `:8700` at a time; the owner runs the
   model boot (T3). Plan 02's execution rule 7 (one command + its output, pasted once) applies here too.
3. Ordering: T1 → T2 (independent, agent-runnable) → T3 (owner) → T4 → T5.
4. Commit per task as `[basalt][P3][T<n>][<topic>] <summary>`, with **this tracker's row in the same commit**.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Prove the no-sudo capture method on a dummy workload | `--internal` network, capture sidecar | pending | |
| 2 | T2: Static inspection of the shipped binaries and venv hooks | image `/opt/basalt` | pending | agent-runnable now |
| 3 | T3: Owner-run capture session | owner | pending | needs the `basalt` container stopped |
| 4 | T4: Read the captures → `runtime.md` | `context/audit/runtime.md` | pending | |
| 5 | T5: Hand to Plan 02's verdict, harvest, close | — | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|

## Notes / Deviations

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
