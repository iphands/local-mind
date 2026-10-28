# Plan 02 — `./run` profile for Qwen3.8-27B NVFP4 — Tracker

## Overview

- Status: 0% complete (0/2 tasks)
- Start date: — (set when T1 starts)
- Plan: `context/plans/02_qwen38_27b_profile.md`
- Evidence: dry-run output and the artifact's `sha256sum` (T1); the owner's pasted boot log and
  curl output (T2). Test environment per row: image tag + `ai.ninfer.ref`, the artifact path, the
  launcher's echoed `docker run` line.

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), `context/AGENTS.md`, then
   `02_qwen38_27b_profile.md`. NInfer source at the pin: `vendor/ninfer`.
2. Environment: work from `server/ninfer`. `docker` as yourself (no `sudo`). T1 needs no GPU and
   no image (gate 6 uses a stub `docker`). Standing Constraints in `SERIES.md` apply.
3. Ordering: T1 → T2. T2 needs Plan 01 T6's image and the owner; when blocked, mark it `blocked`
   with what is needed, and stop.
4. Commit per task as `[ninfer][P2][T<n>][<topic>] <summary>`, with **this tracker's row in the
   same commit**. Never push; report the hash and that you did not run `git push`.
5. Restore the system state: no leftover `ninfer` container, `/tmp/ninfer-gate*` removed.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `MODEL` profiles, `SPEC`/`LM_HEAD_DRAFT`, manifest-aware identity check | `run`, `RULES.md` gate 6, `README.md` | pending | |
| 2 | T2: Owner boots `MODEL=27b ./run`; record it | Evidence | pending | needs Plan 01 T6's image |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

**Mandatory in this project** (P1).

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|

## Notes / Deviations

- 2026-10-27 (planning): requested by the owner mid-Plan 01 ("wire the ./run script up to use
  these"), while downloading `neroued/Qwen3.8-27B-nvfp4-NInfer`. A launcher change needs a plan
  (RULES § When Does a Change Need a Plan?), hence this one.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| 27B bench vs the vLLM 27B and vs Flash-Next on NInfer; C / KV dtype / MTP vs DFlash2 arms | this plan makes it bootable, not tuned | a later plan |
