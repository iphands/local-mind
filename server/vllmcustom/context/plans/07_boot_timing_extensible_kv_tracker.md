# Plan 07 — boot-timing: account for vLLM's extensible KV cache — Tracker

## Overview

- Status: 0% complete (0/2 tasks)
- Start date: — (set when T1 starts)
- Plan: `context/plans/07_boot_timing_extensible_kv.md`
- Evidence:
  - the owner's pasted boot report (2026-11-03, `iphands/vllm-blackwell:cu1321-sm120-main-vllm0.34.0.dev20261103-gae525e9`, `./qwen3.8-flash-next/run-fp4local` defaults: util 0.975, fp8 KV, MTP K=3, humming);
  - that boot's `docker logs vllm`;
  - its mem marks, quoted in the plan.

## Resume Instructions

1. Read `CLAUDE.md`, `context/plans/RULES.md` (in full), then `07_boot_timing_extensible_kv.md`.
2. Environment:
   - no launcher knobs;
   - the `…gae525e9` image for the throwaway-container seam check (`--rm`, no GPU);
   - SERIES Standing Constraints: shellcheck, no `sudo`, and no model load in the per-commit gate.
3. Ordering:
   - T1 is all host-side;
   - T2 needs an owner boot after T1 lands. Never stop or restart the running `vllm` container yourself.
4. Commit per task as `[P7][T<n>][boot-timing] <summary>`, with **this tracker's row in the same commit**.
5. Nothing to restore: no host state is touched.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: hook `extend_kv_cache`, render from `mem.kvext` | `container/patches/boot-timing/` | pending | |
| 2 | T2: owner boot agrees with vLLM's log | — | pending | needs the owner |

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-03 | first `…gae525e9` boot, `run-fp4local` defaults; report + `docker logs vllm` | report: KV 0.0, outside 14.7, hard 0.93, cross-check differs by 17.7; vLLM: extended to 373 blocks (16.86 GiB), 6.51 GiB free after warmup | 1 | accounting, not timing |

## Notes / Deviations

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| `KV cache (~Nk tokens)` label: `_kvcfg_detail` expects a list but `initialize_from_config` takes one `KVCacheConfig`; and blocks × `block_size` isn't tokens on hybrid models | absent since before 2026-10-18; see the plan's Rejected Claims | a later boot-timing plan |
