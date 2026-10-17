# Plan 06 — b12x SM12x kernels as an opt-in overlay — Tracker

## Overview

- Status: 67% complete (2/3 tasks)
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
| 2 | T2: Launcher plumbing | `common/_run-lib.sh`, `README.md` | done | stub-docker on `run-fp4local`: (a) both `-v` b12x lines + `--moe-backend b12x --linear-backend b12x`; (b) defaults: 0 mounts, `--moe-backend humming`; (c) `flashinfer_b12x`: 0 mounts; (d) `B12X=1` unfetched: rc 1 naming `fetch`, no `serve` line; (e) `B12X=0`+b12x: warning, no mount; (f) bogus: rc 1; (g) `ENV_EXTRA` still after the mounts. `run` (NVFP4) with `ONLINE_FP8=1 SM120_SKINNY=1 MOE_BACKEND=b12x`: mounted. Step 6 not run — runtime is T3 |
| 3 | T3: Owner A/B on both Flash-Next checkpoints | tracker only | in-progress | arm `MOE_BACKEND=b12x NVFP4_BACKEND=b12x` on FP4-Local: **boot failed** in encoder profiling (`ValueError: A16 requires stored K32 and N8`, vision `linear_fc2` K=4304); MoE never reached. Next: MoE-only, then MXFP8-only via `linear_backend_per_quant` |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-16 | T1: `./container/patches/b12x/fetch` ×2, then a copy with `B12X_WHEEL_SHA256=0…0` and `CACHE_DIR=<scratch>` | `fetched` → `/mnt/noir/scratch/ai/vllm/cache/b12x/1.5.1/site`, stamp `8731e235…`; then `already present`; bad sha: `sha256 mismatch … Nothing was unpacked.` rc 1, `find` shows only `b12x/1.5.1/` | 1 each | behavior check, not a number |
| 2026-10-17 | T3 boot (owner): `SM120_SKINNY=1 MOE_BACKEND=b12x NVFP4_BACKEND=b12x ./qwen3.8-flash-next/run-fp4local`, image `…g4d9a8d6`, b12x 1.5.1 mounted | weights loaded (72.66 GiB, 387.9 s); b12x prepared MXFP8 `1152→3456`, `1152→1152` and NVFP4-A16 `1152→4304`, `4304→1152` at `num_tokens=65536`; then `EngineCore failed to start` in `profile_encoder_cache` → `qwen3_vl.py:440` `linear_fc2` → `kernels/linear/nvfp4/b12x.py:191` → `b12x/gemm/blockscaled/_tuning.py:191` `ValueError: A16 requires stored K32 and N8` | 1 | deterministic shape check, not noise: K=4304, 4304 % 32 = 16 |
| 2026-10-16 | T2: `run-fp4local` + `run` under gate 7a's stub `docker`, `IMAGE_TAG=…g4d9a8d6`, scratch `CACHE_DIR`, `B12X_SITE=` the T1 tree | cases (a)–(g) as in the Progress row; the echoed `serve` line is the evidence | 1 each | emitted-command check; nothing booted |
| 2026-10-16 | Plan-time probe: wheel unpacked to scratch, mounted `:ro` + `.pth`, `--gpus device=0`, image `…g4d9a8d6` | all 6 `vllm.utils.b12x` submodules import; `B12xMxfp8LinearKernel`, `B12xNvFp4LinearKernel`, `B12xFp8BlockScaledMMKernel`, `B12xTensorFP8ScaledMMLinearKernel` `is_supported() == (True, None)`; `B12xExperts._supports_current_device() == True` | 1 | capability check only — no GEMM was executed |

## Notes / Deviations

- 2026-10-16, plan commit gate: steps 1 (`30 b` / `25 p`, `env ok` — 30 not 29 because `b9aa163` added
  `lfm2.5/run` without bumping RULES.md), 3 (5/5 ok), 4 (`## preflight OK` ×2; dirty-tree guard printed
  nothing), 5 (`last-build == 4d9a8d6…`, `1`; `container/archs` not re-run — no build happened), 7a–7e
  PASS. Step 2 is **red, not skipped**: see plan Risk 5.

- 2026-10-16, T1 gate: step 1 `31 b` / `25 p`, `env ok`, no FAIL; step 2 still red (`muse/run:355` SC2145, `scripts/muse-quality-diff:25` SC2034 — pre-existing, untouched; Risk 5), `fetch` itself `shellcheck -x` clean; 3 5/5 ok; 4 `## preflight OK` ×2, no DIRTY; 5 `last-build == 4d9a8d6…`, `1`, archs not re-run (no build); 7a–7e PASS.

- 2026-10-16, T2 gate: identical to T1's — step 1 `31 b` / `25 p`, `env ok`, no FAIL/DIRTY; step 2 red only on the same two pre-existing findings (`common/_run-lib.sh` itself `shellcheck -x` clean); 3 5/5; 4 OK ×2; 5 `last-build == 4d9a8d6…`, `1`; 7a–7e PASS.

- 2026-10-17, T3 first boot: **`NVFP4_BACKEND=b12x` cannot serve FP4-Local with vision on.** b12x A16 requires `padded_in_features % 32 == 0 and out_features % 8 == 0` (`b12x/gemm/blockscaled/_tuning.py:189-190`); vLLM's `B12xNvFp4LinearKernel.can_implement` returns `True, None` unconditionally (`vllm/model_executor/kernels/linear/nvfp4/b12x.py:71-73` @ `4d9a8d6`), so the selector hands it a layer it cannot run and the failure lands at first use instead of falling back. Shape scan of every MXFP8 / W4A16_NVFP4 layer in the checkpoint (safetensors headers, 2026-10-17): only `model.visual.blocks.N.mlp.linear_fc2` (27×, N=1152, K=4304) violates it; every MXFP8 layer passes K%32 and N%8. MXFP8-via-b12x ran in vision block 0's attention before the MLP failed.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Bake `b12x==1.5.1` into `container/Dockerfile` (image has no `pip`: use the same wheel + unzip, or `uv`) | only if T3 shows a win; the overlay then goes inert by `.pth` ordering | a later plan |
| Port `--gdn-decode-kernel b12x` | community-fork only, not upstream at `4d9a8d6` | watch upstream |
| Upstream: `B12xNvFp4LinearKernel.can_implement` should reject A16 shapes with K%32 or N%8 (fall back instead of raising at first use) | the FP4-Local vision `linear_fc2` (K=4304) kills the boot under `--linear-backend b12x` | vLLM issue/PR (owner) |
| Gate step 2: fix `muse/run:355` SC2145 + `scripts/muse-quality-diff:25` SC2034, make the step print FAIL (not `SKIP(shellcheck not installed)`) when shellcheck exits non-zero, and drop SERIES' "shellcheck is not installed" constraint | shellcheck 0.11.0 is installed now; the step misreports a red run as a skip (plan Risk 5) | a small follow-up commit or plan |
