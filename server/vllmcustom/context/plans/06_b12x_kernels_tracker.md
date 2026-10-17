# Plan 06 — b12x SM12x kernels as an opt-in overlay — Tracker

## Overview

- Status: 33% complete (1/3 tasks)
- Start date: 2026-10-16
- Plan: `context/plans/06_b12x_kernels.md`
- Evidence: the launcher's echoed `docker … serve …` line (T2), owner boot logs + `bench/bench-results.jsonl`
  rows (T3). Image `iphands/vllm-blackwell:cu1321-sm120-main-vllm0.33.0.dev20261015-g4d9a8d6`
  (`ai.vllmcustom.vllm.ref` = `VLLM_MAIN_SHA` `4d9a8d6`), b12x 1.5.1.

## Resume Instructions

1. Read `CLAUDE.md`, `context/plans/RULES.md` (in full), then `06_b12x_kernels.md`.
2. Environment: `CACHE_DIR` (default `/mnt/noir/scratch/ai/vllm/cache`) is where the wheel unpacks;
   `B12X`, `MOE_BACKEND`, `NVFP4_BACKEND` are the knobs. SERIES Standing Constraints: no `sudo`; no
   model load in the per-commit gate (T3 is step 6, owner-run); `shellcheck` absent → step 2 `skip`.
3. Ordering: T1 → T2 (T2's refusal path needs T1's `fetch` to name) → T3 (owner boots).
4. Commit per task as `[P6][T<n>][<topic>] <summary>`, with **this tracker's row in the same commit**.
5. Restore the system state at the end of the session (Rule P2.4): unset `B12X` / backend knobs.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: The pinned fetch | `container/patches/b12x/` | done | `fetch` → `fetched`, rerun → `already present`, bad-sha copy → refused rc 1, no `site/`, no temp left; tree owned `iphands:iphands`; `shellcheck -x` clean |
| 2 | T2: Launcher plumbing | `common/_run-lib.sh`, `README.md` | pending | |
| 3 | T3: Owner A/B on both Flash-Next checkpoints | tracker only | pending | gate step 6, owner boots |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-16 | T1: `./container/patches/b12x/fetch` ×2, then a copy with `B12X_WHEEL_SHA256=0…0` and `CACHE_DIR=<scratch>` | `fetched` → `/mnt/noir/scratch/ai/vllm/cache/b12x/1.5.1/site`, stamp `8731e235…`; then `already present`; bad sha: `sha256 mismatch … Nothing was unpacked.` rc 1, `find` shows only `b12x/1.5.1/` | 1 each | behavior check, not a number |
| 2026-10-16 | Plan-time probe: wheel unpacked to scratch, mounted `:ro` + `.pth`, `--gpus device=0`, image `…g4d9a8d6` | all 6 `vllm.utils.b12x` submodules import; `B12xMxfp8LinearKernel`, `B12xNvFp4LinearKernel`, `B12xFp8BlockScaledMMKernel`, `B12xTensorFP8ScaledMMLinearKernel` `is_supported() == (True, None)`; `B12xExperts._supports_current_device() == True` | 1 | capability check only — no GEMM was executed |

## Notes / Deviations

- 2026-10-16, plan commit gate: steps 1 (`30 b` / `25 p`, `env ok` — 30 not 29 because `b9aa163` added
  `lfm2.5/run` without bumping RULES.md), 3 (5/5 ok), 4 (`## preflight OK` ×2; dirty-tree guard printed
  nothing), 5 (`last-build == 4d9a8d6…`, `1`; `container/archs` not re-run — no build happened), 7a–7e
  PASS. Step 2 is **red, not skipped**: see plan Risk 5.

- 2026-10-16, T1 gate: step 1 `31 b` / `25 p`, `env ok`, no FAIL; step 2 still red (`muse/run:355` SC2145, `scripts/muse-quality-diff:25` SC2034 — pre-existing, untouched; Risk 5), `fetch` itself `shellcheck -x` clean; 3 5/5 ok; 4 `## preflight OK` ×2, no DIRTY; 5 `last-build == 4d9a8d6…`, `1`, archs not re-run (no build); 7a–7e PASS.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Bake `b12x==1.5.1` into `container/Dockerfile` (image has no `pip`: use the same wheel + unzip, or `uv`) | only if T3 shows a win; the overlay then goes inert by `.pth` ordering | a later plan |
| Port `--gdn-decode-kernel b12x` | community-fork only, not upstream at `4d9a8d6` | watch upstream |
| Gate step 2: fix `muse/run:355` SC2145 + `scripts/muse-quality-diff:25` SC2034, make the step print FAIL (not `SKIP(shellcheck not installed)`) when shellcheck exits non-zero, and drop SERIES' "shellcheck is not installed" constraint | shellcheck 0.11.0 is installed now; the step misreports a red run as a skip (plan Risk 5) | a small follow-up commit or plan |
