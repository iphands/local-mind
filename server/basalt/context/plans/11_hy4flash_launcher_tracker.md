# Plan 11 — Hy4-flash launcher: `./run-hy4` — Tracker

## Overview

- Status: 0% complete (0/4 tasks)
- Start date: — (set when T1 starts)
- Plan: `context/plans/11_hy4flash_launcher.md`
- Evidence: the table below; boot start blocks pasted by the owner. Test environment: host `cosmo`,
  RTX PRO 6000 (GPU 0), image `iphands/basalt-blackwell:cu1343-sm120-ad274aa`, both packs on NFS
  (`/mnt/noir/scratch/ai/llm/models/vllm/jesdga/`).

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), then `11_hy4flash_launcher.md`.
2. Environment: docker as yourself (no `sudo`); `shellcheck` 0.11.0 on `PATH`; `/mnt/noir` mounted
   (gate steps 4, 5 and 5b read both packs' headers). The gate's scratch files default to
   `/tmp/opencode/`, which does not exist in every shell — create it or point the two paths at a
   directory that does. Check `id -nG` lists `docker` before claiming steps 3–6.
3. Ordering: T1 → T2 → T3 → T4. T3 is the owner's to run; mark it `blocked` (with the reason) while
   it waits. T2 does not wait on T3.
4. Commit per task as `[basalt][P11][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**. Never push.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `./run-hy4`, `pack_arch`, and the gate that covers them | `run-hy4`, `lib/common.sh`, `RULES.md`, `README.md`, `AGENTS.md` | pending | |
| 2 | T2: `./run` refuses a Hy4-flash pack | `run`, `RULES.md`, `README.md` | pending | |
| 3 | T3: owner boot of `./run-hy4`, with a real reply | tracker Evidence | pending | |
| 4 | T4: harvest and close | `context/` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-20 | project gate 1–6 before the plan commit, image `iphands/basalt-blackwell:cu1343-sm120` (id `2c2253095d35`, the `ad274aa` build) | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=4 AUDIT_B=5`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 158 engine flags known)`; `imports ok (42 serve modules)` | 1 | the baseline T1 is compared against; gate scratch paths pointed at the session's scratch directory |
| 2026-11-20 | the engine's no-load check on the Hy4 pack, in the image, `--network none`: `basalt --model /models/Hy4-flash-NVFP4.basalt <words> --gpu-plan` | `--max-context 362144 --kv int8 --ple-io ram` → `basalt: error: unknown argument: --ple-io`, exit 2; `--max-context 362144 --kv int8` → `basalt hy4flash: error: --kv int8: the latent caches are fp16 (the default) or fp8`, exit 2; `--max-context 362144 --kv fp16` → a plan line with `"arch":"hy4flash"`, `"spec":6`, `"error":""`, exit 0; `--kv fp8 --batch 3` → exit 0; `--max-context 2000000 --kv fp16` → exit 0 | 1 each | reproduces the owner's refusal without a load; the context range is not checked here |
| 2026-11-20 | section tables of both packs (`od`, `dd` over the 64 KiB header) and `grep -a -o` over each `meta` section | Hy4: version 2, 14 sections, `meta` at 158741823488 + 4344821 = 158746168309, the file's size; `"general.architecture":"hy4flash"`, `"context_length":1048576`. Flash-Next: 18 sections; `"general.architecture":"qwen4exp"`, `"qwen4exp.context_length":262144` | 1 | the read of each `meta` section took about 10 ms from NFS |

## Notes / Deviations

- **2026-11-20 — the plan was written from source and docs at the pin plus no-load probes; nothing
  here has booted Hy4-flash.** Every memory and speed figure in the plan's Key Facts is upstream's.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Move the launcher body `run` and `run-hy4` share (docker args, the masked print, the exec) into `lib/common.sh` | about 60 duplicated lines; left alone while `./run`'s boots are owed to Plans 08 and 10 | after those boots; a plan (it rewrites `./run`) |
| Boot `PARALLEL=3 ./run-hy4` | upstream lists no parallel slots for Hy4-flash; the engine accepts the flag (plan Open Question 4) | owner decision; a later plan or Plan 05 |
| Choose Hy4-flash's context and KV format on measurements (`MAX_CONTEXT=1048576`, `KV=fp8`) | the defaults are starting values (plan Open Question 3) | owner decision; Plan 05 |
