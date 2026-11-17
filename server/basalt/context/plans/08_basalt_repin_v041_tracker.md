# Plan 08 — Repin Basalt to v0.4.1 and rebuild the image — Tracker

## Overview

- Status: 0% complete (0/5 tasks)
- Start date: 2026-11-16
- Plan: `context/plans/08_basalt_repin_v041.md`
- Evidence: the table below; build logs in `captures/build_<date>_<sha7>.log` (gitignored); boot
  start blocks pasted by the owner. Test environment: host `cosmo`, RTX PRO 6000 (GPU 0), docker
  daemon 29.8.1, the pack on NFS (`/mnt/noir/scratch/ai/llm/models/vllm/jesdga/`).

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), then `08_basalt_repin_v041.md`.
2. Environment: docker as yourself (no `sudo`); `shellcheck` 0.11.0 on `PATH`. The gate's scratch
   files default to `/tmp/opencode/`, which does not exist in every shell — create it or point the
   two paths at a directory that does. Check `id -nG` lists `docker` before claiming steps 3–6.
3. Ordering: T1 → T2 → T3 → T4 → T5. **T2 commits nothing until the owner's `./build` has produced
   the image** — gate steps 5–6 fail with no image for `BASALT_SHA`. T2 steps 2 and 4 and all of T3
   are the owner's to run; mark the task `blocked` (with the reason) while it waits.
4. Commit per task as `[basalt][P8][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**. Never push.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: let a pack argument override the NFS default again | `run`, `README.md` | pending | |
| 2 | T2: repin to `d3b7574`, adapt the recipe, owner build, gate on the new image | `versions.env`, `Dockerfile`, `README.md` | pending | |
| 3 | T3: owner boots, default and `--single`, a real reply each | tracker Evidence | pending | |
| 4 | T4: retire the `dc047a7` audit plans, register the re-audit | `SERIES.md`, `abandoned/`, `audit/index.md` | pending | |
| 5 | T5: harvest and close | `context/` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|

## Notes / Deviations

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
