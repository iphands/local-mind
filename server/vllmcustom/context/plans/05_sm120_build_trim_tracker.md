# Plan 05 — Trim the source build to what sm_120 serving uses — Tracker

## Overview

- Status: 0% complete (0/10 tasks)
- Start date: — (set when T1 starts)
- Plan: `context/plans/05_sm120_build_trim.md`
- Evidence: build logs / BuildKit history for the pre-trim image `iphands/vllm-blackwell:cu1321-sm120-main-vllm0.33.0.dev20261015-g4d9a8d6` and the trimmed rebuild; `tools/smoke-model` output per launcher; `bench/bench-results.jsonl` rows.

## Resume Instructions

1. Read `CLAUDE.md`, `context/plans/RULES.md` (in full), then `05_sm120_build_trim.md` — especially *Rejected Claims* (do not re-trim fp8-activation Marlin).
2. Environment: snapshot set (`VLLM_REF=main`, vLLM `4d9a8d6`, FlashInfer `v0.7.1`, torch `2.14.0`). The owner runs every `./container/build`; Claude edits and verifies. Standing constraints in `SERIES.md`: no model load in the per-commit gate; never push.
3. Order: Wave 1 (T1–T3) before any trim. T4–T7 land together in one owner build (T5 changes `base`, so it is a full rebuild anyway). T8 after T9 proves the default set. T10 only on owner say-so.
4. Commit per task as `[P5][T<n>][trim] <summary>`, tracker row in the same commit.
5. Restore launcher env knobs after any T9 boot (Rule P2.4).

## Open Unknowns

- Does any record of a cold per-stage build time exist (BuildKit history, a kept log)? — T1.
- Is the MoE Marlin generator the same file as the dense one (`CMakeLists.txt:649` vs `:1343`)? — T6.
- Can `fused_moe/oracle/nvfp4.py:625` reach 8-bit-activation Marlin? — T6 (plan Open Question 2).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: per-stage build-time baseline | `tools/build-times` | pending | |
| 2 | T2: smoke-model coverage check + JIT allowlist | `tools/smoke-model` | pending | |
| 3 | T3: image assertions + `_C` op diff | `container/Dockerfile`, `container/archs` | pending | |
| 4 | T4: torchaudio RNNT/align/CTC off | `container/Dockerfile:388` | pending | |
| 5 | T5: torch feature flags, drop libopenmpi-dev | `container/Dockerfile` | pending | full rebuild |
| 6 | T6: Marlin int8-activation trim | `container/patches/vllm-marlin-trim/` | pending | keep fp8-activation |
| 7 | T7: FlashInfer XQA page-128 / MLA hd512 | `container/patches/flashinfer-aot-trim/` | pending | |
| 8 | T8: opt-in slim profiles + label | `container/build`, Dockerfile | pending | |
| 9 | T9: owner rebuild, step 5, five-launcher smoke | — | pending | owner-run |
| 10 | T10: torch opt-in experiments | Dockerfile | pending | owner decides |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-16 | gate steps 1–5 at plan creation (`[P5][plan]` commit, markdown-only) | 1: no FAIL lines, **29 b** / 25 p, env ok; 2: shellcheck 0.11.0, 16 findings in 12 files (pre-existing, none in this commit); 3: 5/5 ok; 4: preflight OK both sets; 5: `last-build == 4d9a8d6…`, 1 image id (`container/archs` not run: no build) | 1 | 1b reads 29 b vs RULES' 28: `qwen3.8-flash-next/run-fp4local` was committed in `ddcd061` without a count bump — fixed in a separate `rules:` commit |

## Notes / Deviations

- Plan drafted from `trim.md` (project root, committed by the owner in `d6c9f43`; left unchanged there — this plan supersedes it) after a review on 2026-10-16 found: fp8-activation Marlin is reachable without the env var (trim would break W4A8-FP8 checkpoints); `FLASHINFER_DISABLE_JIT=1` fails every model regardless; an aten-op inventory cannot validate feature-flag trims; the 2.14 rebuild had already happened. All four are corrected in the plan.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Inspect FA2 head-dim/dtype set via `VLLM_FLASH_ATTN_SRC_DIR` | source only exists inside the build | a later plan |
| Build the release set (`v0.29.0`) with the patches | T9 builds only the snapshot | a later plan |
