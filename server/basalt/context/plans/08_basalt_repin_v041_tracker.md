# Plan 08 — Repin Basalt to v0.4.1 and rebuild the image — Tracker

## Overview

- Status: 20% complete (1/5 tasks)
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
| 1 | T1: let a pack argument override the NFS default again | `run`, `README.md` | done | `./run`'s unconditional `PACK=` line folded into the default (NFS path kept); header, missing-pack hint and README now state the defaults the code has (`362144`, the NFS pack). Gate 1a/1b/1c/2/2b/3/4/5/6 pass on the old pin — Evidence rows 1–3. |
| 2 | T2: repin to `d3b7574`, adapt the recipe, owner build, gate on the new image | `versions.env`, `Dockerfile`, `README.md` | pending | |
| 3 | T3: owner boots, default and `--single`, a real reply each | tracker Evidence | pending | |
| 4 | T4: retire the `dc047a7` audit plans, register the re-audit | `SERIES.md`, `abandoned/`, `audit/index.md` | pending | |
| 5 | T5: harvest and close | `context/` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-16 | `DRY_RUN=1 ./run /tmp/x.ninfer >/dev/null 2>&1; echo "ninfer rc=$?"` at `6275af2`, before T1 | `ninfer rc=0` — a `.ninfer` path was accepted; gate step 4 red | 1 | the argument was overwritten by `run` line 54 |
| 2026-11-16 | the same command after T1; `PACK=README.md DRY_RUN=1 ./run 2>&1 \| tail -1` | `ninfer rc=1`; `## README.md is not a .basalt pack (first bytes '# basalt', want 'BASALTMF').` | 1 | argument and `PACK=` both honoured |
| 2026-11-16 | project gate 1–6, old pin `dc047a7`, image `iphands/basalt-blackwell:cu1343-sm120` (id `63d25a81f328`, the `dc047a7` build) | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=4 AUDIT_B=5`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok`; `flags ok (3 engine, 8 server: 142 engine flags known)`; `imports ok (32 serve modules)` | 2 | the baseline T2 is compared against; `shellcheck` 0.11.0 at `/usr/local/bin` in this shell |

## Notes / Deviations

- **2026-11-16 — the plan commit (`[basalt][P8][plan]`) was made on a gate that is red at its own
  tree.** Gate step 4 has been red since `6275af2`; the gate was run in the working tree, where T1's
  fix already sat unstaged, and passed there. The plan commit's message says so. T1's commit is the
  first tree where step 4 is green.
- **2026-11-16 — gate scratch paths.** `/tmp/opencode/` does not exist in this shell; the step 1b
  block was run with its two scratch paths pointed at another existing directory, otherwise as written.
- **2026-11-16 — `image_for_pin` names the moving tag.** It printed `…:cu1343-sm120`, not
  `…:cu1343-sm120-dc047a7`: three tags share image `63d25a81f328` and `docker images` lists that one
  first. Same image, so steps 4–6 ran against the pinned build; after T2 the new image will carry
  only its `-d3b7574` tag until `./publish` moves the others.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
