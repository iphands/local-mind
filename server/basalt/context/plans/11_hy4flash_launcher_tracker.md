# Plan 11 — Hy4-flash launcher: `./run-hy4` — Tracker

## Overview

- Status: 50% complete (2/4 tasks)
- Start date: 2026-11-20
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
| 1 | T1: `./run-hy4`, `pack_arch`, and the gate that covers them | `run-hy4`, `lib/common.sh`, `RULES.md`, `README.md`, `AGENTS.md` | done | `run-hy4` passes `--max-context 362144 --kv fp16 --parallel 1 --vision dynamic` and no `--ple-io`; it refuses a Flash-Next pack (names `./run`), an unreadable architecture, `KV=int8`, a context outside 1024..1048576 and `VISION` other than `off`/`dynamic`, and prints an NFS note. `pack_arch` reads `hy4flash` / `qwen4exp` from the two packs and nothing from a non-pack, a missing file or a pack cut at 70000 bytes, always exit 0. Gate: Expected counts `5 b` product, `run-hy4` in 1b's identity guard, step 4 and 5 cover it, new step 5b. Gate 1a/1b/1c/2/2b/3/4/5/5b/6 pass; three negative controls fire — Evidence rows 4–6. `shellcheck -x` was clean on `run-hy4` before `git add`. **Not booted** (T3). |
| 2 | T2: `./run` refuses a Hy4-flash pack | `run`, `RULES.md`, `README.md` | done | After the magic check `run` reads `pack_arch`; `hy4flash` / `hy_v4_flash` dies naming `./run-hy4`, anything else (an unreadable architecture included) goes on as before. The default and `--single` dry runs print the same bytes before and after the edit (`cmp`, no output). Gate step 4 asserts the refusal and its pointer. Gate 1a/1b/1c/2/2b/3/4/5/5b/6 pass — Evidence row 7. Run while the owner's `basalt` container (the T3 boot) was up: the dry runs report a busy port and do not refuse. |
| 3 | T3: owner boot of `./run-hy4`, with a real reply | tracker Evidence | pending | |
| 4 | T4: harvest and close | `context/` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-20 | project gate 1–6 before the plan commit, image `iphands/basalt-blackwell:cu1343-sm120` (id `2c2253095d35`, the `ad274aa` build) | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=4 AUDIT_B=5`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 158 engine flags known)`; `imports ok (42 serve modules)` | 1 | the baseline T1 is compared against; gate scratch paths pointed at the session's scratch directory |
| 2026-11-20 | the engine's no-load check on the Hy4 pack, in the image, `--network none`: `basalt --model /models/Hy4-flash-NVFP4.basalt <words> --gpu-plan` | `--max-context 362144 --kv int8 --ple-io ram` → `basalt: error: unknown argument: --ple-io`, exit 2; `--max-context 362144 --kv int8` → `basalt hy4flash: error: --kv int8: the latent caches are fp16 (the default) or fp8`, exit 2; `--max-context 362144 --kv fp16` → a plan line with `"arch":"hy4flash"`, `"spec":6`, `"error":""`, exit 0; `--kv fp8 --batch 3` → exit 0; `--max-context 2000000 --kv fp16` → exit 0 | 1 each | reproduces the owner's refusal without a load; the context range is not checked here |
| 2026-11-20 | T1: `shellcheck -x run-hy4 lib/common.sh`; `DRY_RUN=1 ./run-hy4` and its refusals; `pack_arch` under `set -euo pipefail` on both packs, `README.md`, a missing path, `/dev/null`, a pack cut at 70000 bytes | lint: no output. Dry run: `== mode:  1 slot × 362144, kv=fp16, vision=dynamic, http://0.0.0.0:8700/v1 as 'cosmo-6000'`, three `== note:` lines (pack on `nfs`), `serve /models/Hy4-flash-NVFP4.basalt --host 0.0.0.0 --port 8700 --alias cosmo-6000 --allowed-host cosmo.lan --allowed-host cosmo --gpu 0 --max-context 362144 --kv fp16 --parallel 1 --vision dynamic`, `== (DRY_RUN) nothing started`, exit 0. Exit 1 each: the Flash-Next pack (`…is a Flash-Next pack (architecture qwen4exp).` / `serve it with ./run instead.`), `KV=int8`, `MAX_CONTEXT=1000`, `MAX_CONTEXT=1048577`, `VISION=cpu`, `PARALLEL=9`, a `.ninfer` path, a missing path, `README.md`, the cut pack (`architecture 'unreadable'`). `PARALLEL=3 KV=fp8 MAX_CONTEXT=1048576 API_KEY=k3y` → `3 slots × 1048576 (slot-full-context)`, `--api-key REDACTED`. `pack_arch`: `hy4flash`, `qwen4exp`, then empty ×4, exit 0 ×6 | 1 | `storage/` existed before the dry runs; none created anything |
| 2026-11-20 | T1: project gate 1–6 with 5b, image `2c2253095d35` | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=5 AUDIT_B=5`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 158 engine flags known)`; `pack flags ok (./run: qwen4exp takes --max-context 362144 --kv int8 --ple-io ram --batch 3)`; `pack flags ok (./run --single: qwen4exp takes --max-context 362144 --kv int8 --ple-io ram)`; `pack flags ok (./run-hy4: hy4flash takes --max-context 362144 --kv fp16)`; `imports ok (42 serve modules)` | 1 | `PRODUCT_B` 4 → 5 is `run-hy4`; step 5 still counts 3 engine flags (`--max-context`, `--kv`, `--ple-io` over the three dry runs) |
| 2026-11-20 | T1 negative controls: gate steps 4, 5, 5b under `EXTRA_ARGS` | `--ple-io ram`: step 4 `FAIL(dry:hy4) passes --ple-io`; 5b `FAIL(pack-flags) ./run-hy4: unknown argument: --ple-io` with both `./run` lines still ok. `--kv int8`: 5b `FAIL(pack-flags) ./run-hy4: --kv int8: the latent caches are fp16 (the default) or fp8`, both `./run` lines ok. `--no-such-flag`: step 5 `FAIL(flags) --no-such-flag`; 5b `FAIL(pack-flags) … unknown argument: --no-such-flag` for all three | 1 each | 5b reproduces the owner's 2026-11-20 refusal from a dry run, with no load |
| 2026-11-20 | T2: `{ DRY_RUN=1 ./run; DRY_RUN=1 ./run --single; }` captured before and after the edit, `cmp`; `DRY_RUN=1 ./run ./models/vllm/jesdga/Hy4-flash-NVFP4.basalt`; `DRY_RUN=1 ./run <a pack cut at 70000 bytes>`; `shellcheck -x run`; project gate 1–6 with 5b | `cmp`: no output (18 lines each). The Hy4 pack: `…is a Hy4-flash pack; it refuses the Flash-Next flags ./run passes (--ple-io, --kv int8).` / `serve it with ./run-hy4 instead.`, exit 1. The cut pack (architecture unreadable): dry run completes, exit 0. Lint: no output. Gate: `PRODUCT_B=5 AUDIT_B=5`; `env ok`; both `shellcheck ok`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 158 engine flags known)`; three `pack flags ok`; `imports ok (42 serve modules)` | 1 | `before.txt` / `after.txt` in the session's scratch directory, not kept |
| 2026-11-20 | section tables of both packs (`od`, `dd` over the 64 KiB header) and `grep -a -o` over each `meta` section | Hy4: version 2, 14 sections, `meta` at 158741823488 + 4344821 = 158746168309, the file's size; `"general.architecture":"hy4flash"`, `"context_length":1048576`. Flash-Next: 18 sections; `"general.architecture":"qwen4exp"`, `"qwen4exp.context_length":262144` | 1 | the read of each `meta` section took about 10 ms from NFS |

## Notes / Deviations

- **2026-11-20 (T1) — step 5b passes no GPU to `model_plan`, where the plan's sketch passed `a.gpu`.**
  The step is about flags; with `None` the engine picks its own card and the check does not depend on
  which cards the container sees. The plan line it prints still listed the RTX PRO 6000: on this host
  a container sees the cards without a `--gpus` flag. Not explored further.
- **2026-11-20 (T1) — `./run` still takes a Hy4-flash pack to the engine.** T2 adds its refusal;
  `README.md` says so until then.

- **2026-11-20 — the plan was written from source and docs at the pin plus no-load probes; nothing
  here has booted Hy4-flash.** Every memory and speed figure in the plan's Key Facts is upstream's.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Move the launcher body `run` and `run-hy4` share (docker args, the masked print, the exec) into `lib/common.sh` | about 60 duplicated lines; left alone while `./run`'s boots are owed to Plans 08 and 10 | after those boots; a plan (it rewrites `./run`) |
| Boot `PARALLEL=3 ./run-hy4` | upstream lists no parallel slots for Hy4-flash; the engine accepts the flag (plan Open Question 4) | owner decision; a later plan or Plan 05 |
| Choose Hy4-flash's context and KV format on measurements (`MAX_CONTEXT=1048576`, `KV=fp8`) | the defaults are starting values (plan Open Question 3) | owner decision; Plan 05 |
