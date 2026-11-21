# Plan 10 — Repin Basalt to v0.5.0 and rebuild the image — Tracker

## Overview

- Status: 0% complete (0/3 tasks)
- Start date: — (set when T1 starts)
- Plan: `context/plans/10_basalt_repin_v050.md`
- Evidence: the table below; build logs in `captures/build_<date>_<sha7>.log` (gitignored); boot
  start blocks pasted by the owner. Test environment: host `cosmo`, RTX PRO 6000 (GPU 0), the pack on
  NFS (`/mnt/noir/scratch/ai/llm/models/vllm/jesdga/`).

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), then `10_basalt_repin_v050.md`.
2. Environment: docker as yourself (no `sudo`); `shellcheck` 0.11.0 on `PATH`. The gate's scratch
   files default to `/tmp/opencode/`, which does not exist in every shell — create it or point the
   two paths at a directory that does. Check `id -nG` lists `docker` before claiming steps 3–6.
   `GIT_NO_LAZY_FETCH=1` on every git command against `vendor/basalt` (a `blob:none` clone).
3. Ordering: T1 → T2 → T3. **T1 commits nothing until the owner's `./build` has produced the image**
   — gate steps 5–6 fail with no image for `BASALT_SHA`. T1 steps 2 and 4 and all of T2 are the
   owner's to run; mark the task `blocked` (with the reason) while it waits.
4. Commit per task as `[basalt][P10][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**. Never push.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: repin to `ad274aa`, owner build, read the tree, gate on the new image | `versions.env`, `README.md` | pending | |
| 2 | T2: owner boots, default and `--single`, a real reply each | tracker Evidence | pending | |
| 3 | T3: harvest and close | `context/` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-20 | project gate 1–6, old pin `d3b7574`, image `iphands/basalt-blackwell:cu1343-sm120` (id `41619e9056f8`, the `d3b7574` build), before the plan commit | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=4 AUDIT_B=5`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 153 engine flags known)`; `imports ok (40 serve modules)` | 1 | the baseline T1 is compared against; `shellcheck` 0.11.0 at `/usr/local/bin` in this shell; gate scratch paths pointed at the session's scratch directory |

## Notes / Deviations

- **2026-11-20 — the pre-read of v0.5.0 is paraphrase.** Upstream's changelog, `Dockerfile`,
  `requirements.txt`, `serve/config.py` and two docs pages were read at the tag through a fetch tool
  that returns summaries and short quotes, not file bytes; the compare API's file list was read only
  in part. The plan's Key Facts mark each such row. Nothing was fetched into `vendor/basalt`.
- **2026-11-20 — the v0.4.1 image has served.** `storage/cache/basalt/aefd5d98dc650ed4/engine.log`
  holds four `session is up (engine 0.4.1)` lines (lines 81588–86137 of 86715; file last written
  2026-11-19 10:05), one followed by a `stall report (engine 0.4.1)` at prompt token 0. That is not
  Plan 08 T3's evidence (no `./run` `==` lines, no quoted reply, modes unknown) and is not recorded
  there; it is noted here because the pin now moves on.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Plan 08's open rows: T3 (boots of the `d3b7574` image) and T5 (harvest and close) | owner decision 2026-11-20: later. Once the pin moves, a plain `./run` boots the v0.5.0 image, so T3 as written needs `IMAGE_OVERRIDE` or a `skipped` with the reason | owner decision; Plan 08's tracker |
| Re-target Plan 09 (audit) from `d3b7574` to `ad274aa`; fix `context/AGENTS.md` (the `audit/` row) and the note opening `context/audit/index.md`, which call `d3b7574` the built pin | owner decision 2026-11-20: later. Stale from T1's commit on | owner decision; `SERIES.md` row 09 |
| Is `v0.5.0` a signed tag, and does the pin's tree equal GitHub's tree object for it? | plan Open Question 6; same question Plan 08 left for v0.4.1 | the audit plan |
| Build only Flash-Next (`-DBASALT_MODELS=qwen4exp`) | four models are compiled and shipped, one is served; a build-flag change is a plan | owner decision; a new plan if wanted |
| `./publish` header still shows `dc047a7` in its example tags | comment only; carried from Plan 08's Follow-ups | a doc fix outside a plan |
