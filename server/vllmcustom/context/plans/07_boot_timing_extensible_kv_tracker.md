# Plan 07 — boot-timing: account for vLLM's extensible KV cache — Tracker

## Overview

- Status: 100% complete (2/2 tasks)
- Start date: 2026-11-03
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
| 1 | T1: hook `extend_kv_cache`, render from `mem.kvext` | `container/patches/boot-timing/` | done | Host test passes, including the new cases. A throwaway `--rm` container of `…gae525e9` (uid 1000, no GPU) wraps the real `Worker.extend_kv_cache` with `missing: []`. Gate steps 1–5 below. **Runtime not verified: gate step 6 is T2.** |
| 2 | T2: owner boot agrees with vLLM's log | — | done | Owner boot 2026-11-03 (container start 02:20 UTC 11-04, pid 240). KV row 16.8 GiB vs vLLM's "Extended KV cache to 372 blocks (16.82 GiB)"; outside 1.7; hard 0.79; no `hooks NOT applied`; `worker.kv_extend` not in "spans not seen". |

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-03 | first `…gae525e9` boot, `run-fp4local` defaults; report + `docker logs vllm` | report: KV 0.0, outside 14.7, hard 0.93, cross-check differs by 17.7; vLLM: extended to 373 blocks (16.86 GiB), 6.51 GiB free after warmup | 1 | accounting, not timing |
| 2026-11-03 | `test_boot_timing.py`: the real boot's 13 mem marks (verbatim bytes) plus a synthetic `mem.kvext` (16.86 GiB, free reduced by the 3.80 GiB growth vLLM logged) | KV 16.9, outside 1.7, hard 0.79, safe 0.83; note "profile estimate 17.7 GiB → committed 16.9 GiB, 373 blocks". Without `mem.kvext`: KV/outside/headroom withheld with a note. | 1 | **replay, not a boot**: the `mem.kvext` row is built from vLLM's rounded log figures |
| 2026-11-03 | `docker run --rm --user 1000:1000 … gae525e9` importing `gpu_worker` under the hook finder | `wrapped: True`; 7 gpu_worker targets in `hooked()`; `missing: []`; `ExtensibleKVCache.physical_bytes` is a property | 1 | seam check only, no GPU |
| 2026-11-03 | owner boot of `./qwen3.8-flash-next/run-fp4local` on `…gae525e9` with the T1 tree mounted (gate step 6); pasted report + `docker logs vllm` + `docker exec vllm grep mem\. /tmp/vllm-boot-timing/marks.log` | `mem.kvext`: `kv_bytes=18056478720` (16.82 GiB), `blocks=372`, `free=2959015936`; vLLM: "Extended KV cache to 372 blocks (16.82 GiB)", "Available KV cache memory: 17.64 GiB". Report: KV 16.8, outside 1.7 (= 1.65 by hand), hard 0.79, safe 0.83, note "profile estimate 17.6 GiB → committed 16.8 GiB, 372 blocks" | 1 | accounting identity, not a timing; one boot settles it |

## Notes / Deviations

- Open Question 3 settled on the T2 boot: `model_runner` is the V2 runner when `extend_kv_cache` runs, and the attribute read worked (`kv_bytes` present).
- The `[P7][plan]` commit was made before running the gate. It was docs-only (plan, tracker, SERIES), and the full gate ran before T1.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| `KV cache (~Nk tokens)` label: `_kvcfg_detail` expects a list but `initialize_from_config` takes one `KVCacheConfig`; and blocks × `block_size` isn't tokens on hybrid models | absent since before 2026-10-18; see the plan's Rejected Claims | a later boot-timing plan |
| `gpu_memory_utilization=0.97` is printed for 0.975 (`:.2f` on 0.975 rounds down in binary float) | cosmetic, pre-existing; seen in both 2026-10-18 and 2026-11-03 reports | a later boot-timing plan |
