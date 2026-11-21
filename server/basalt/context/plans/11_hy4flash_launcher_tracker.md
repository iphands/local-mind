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
| 3 | T3: owner boot of `./run-hy4`, with a real reply | tracker Evidence | blocked | **Half recorded; waits on the owner (2026-11-20): a local copy of the pack, a boot from it, and a quoted reply.** The owner booted `./run-hy4` from the NFS pack at 21:02 PST (not pasted; read from `docker inspect`, `docker logs` and the engine log). It loaded on the `ad274aa` image with no flag refused, every expert in VRAM, and answered a 456-token prompt (30 tokens; the reply text is not in any log, so nothing is quoted). **The plan's "Refutes if" then fired:** a 14,150-token prompt made no progress for 60 s, 256 threads in `nfs_file_direct_read`, and the watchdog stopped the engine; the server reloaded it and the client sent the prompt again — Evidence rows 8–9. The launcher and its flags stand; the NFS default pack does not serve real prompts. `run-hy4`'s note and `README.md` now carry the measurement; `pitfalls.md` and `distilled.md` have the entries. |
| 4 | T4: harvest and close | `context/` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-20 | project gate 1–6 before the plan commit, image `iphands/basalt-blackwell:cu1343-sm120` (id `2c2253095d35`, the `ad274aa` build) | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=4 AUDIT_B=5`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 158 engine flags known)`; `imports ok (42 serve modules)` | 1 | the baseline T1 is compared against; gate scratch paths pointed at the session's scratch directory |
| 2026-11-20 | the engine's no-load check on the Hy4 pack, in the image, `--network none`: `basalt --model /models/Hy4-flash-NVFP4.basalt <words> --gpu-plan` | `--max-context 362144 --kv int8 --ple-io ram` → `basalt: error: unknown argument: --ple-io`, exit 2; `--max-context 362144 --kv int8` → `basalt hy4flash: error: --kv int8: the latent caches are fp16 (the default) or fp8`, exit 2; `--max-context 362144 --kv fp16` → a plan line with `"arch":"hy4flash"`, `"spec":6`, `"error":""`, exit 0; `--kv fp8 --batch 3` → exit 0; `--max-context 2000000 --kv fp16` → exit 0 | 1 each | reproduces the owner's refusal without a load; the context range is not checked here |
| 2026-11-20 | section tables of both packs (`od`, `dd` over the 64 KiB header) and `grep -a -o` over each `meta` section | Hy4: version 2, 14 sections, `meta` at 158741823488 + 4344821 = 158746168309, the file's size; `"general.architecture":"hy4flash"`, `"context_length":1048576`. Flash-Next: 18 sections; `"general.architecture":"qwen4exp"`, `"qwen4exp.context_length":262144` | 1 | the read of each `meta` section took about 10 ms from NFS |
| 2026-11-20 | T1: `shellcheck -x run-hy4 lib/common.sh`; `DRY_RUN=1 ./run-hy4` and its refusals; `pack_arch` under `set -euo pipefail` on both packs, `README.md`, a missing path, `/dev/null`, a pack cut at 70000 bytes | lint: no output. Dry run: `== mode:  1 slot × 362144, kv=fp16, vision=dynamic, http://0.0.0.0:8700/v1 as 'cosmo-6000'`, three `== note:` lines (pack on `nfs`), `serve /models/Hy4-flash-NVFP4.basalt --host 0.0.0.0 --port 8700 --alias cosmo-6000 --allowed-host cosmo.lan --allowed-host cosmo --gpu 0 --max-context 362144 --kv fp16 --parallel 1 --vision dynamic`, `== (DRY_RUN) nothing started`, exit 0. Exit 1 each: the Flash-Next pack (`…is a Flash-Next pack (architecture qwen4exp).` / `serve it with ./run instead.`), `KV=int8`, `MAX_CONTEXT=1000`, `MAX_CONTEXT=1048577`, `VISION=cpu`, `PARALLEL=9`, a `.ninfer` path, a missing path, `README.md`, the cut pack (`architecture 'unreadable'`). `PARALLEL=3 KV=fp8 MAX_CONTEXT=1048576 API_KEY=k3y` → `3 slots × 1048576 (slot-full-context)`, `--api-key REDACTED`. `pack_arch`: `hy4flash`, `qwen4exp`, then empty ×4, exit 0 ×6 | 1 | `storage/` existed before the dry runs; none created anything |
| 2026-11-20 | T1: project gate 1–6 with 5b, image `2c2253095d35` | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=5 AUDIT_B=5`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 158 engine flags known)`; `pack flags ok (./run: qwen4exp takes --max-context 362144 --kv int8 --ple-io ram --batch 3)`; `pack flags ok (./run --single: qwen4exp takes --max-context 362144 --kv int8 --ple-io ram)`; `pack flags ok (./run-hy4: hy4flash takes --max-context 362144 --kv fp16)`; `imports ok (42 serve modules)` | 1 | `PRODUCT_B` 4 → 5 is `run-hy4`; step 5 still counts 3 engine flags (`--max-context`, `--kv`, `--ple-io` over the three dry runs) |
| 2026-11-20 | T1 negative controls: gate steps 4, 5, 5b under `EXTRA_ARGS` | `--ple-io ram`: step 4 `FAIL(dry:hy4) passes --ple-io`; 5b `FAIL(pack-flags) ./run-hy4: unknown argument: --ple-io` with both `./run` lines still ok. `--kv int8`: 5b `FAIL(pack-flags) ./run-hy4: --kv int8: the latent caches are fp16 (the default) or fp8`, both `./run` lines ok. `--no-such-flag`: step 5 `FAIL(flags) --no-such-flag`; 5b `FAIL(pack-flags) … unknown argument: --no-such-flag` for all three | 1 each | 5b reproduces the owner's 2026-11-20 refusal from a dry run, with no load |
| 2026-11-20 | T2: `{ DRY_RUN=1 ./run; DRY_RUN=1 ./run --single; }` captured before and after the edit, `cmp`; `DRY_RUN=1 ./run ./models/vllm/jesdga/Hy4-flash-NVFP4.basalt`; `DRY_RUN=1 ./run <a pack cut at 70000 bytes>`; `shellcheck -x run`; project gate 1–6 with 5b | `cmp`: no output (18 lines each). The Hy4 pack: `…is a Hy4-flash pack; it refuses the Flash-Next flags ./run passes (--ple-io, --kv int8).` / `serve it with ./run-hy4 instead.`, exit 1. The cut pack (architecture unreadable): dry run completes, exit 0. Lint: no output. Gate: `PRODUCT_B=5 AUDIT_B=5`; `env ok`; both `shellcheck ok`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 158 engine flags known)`; three `pack flags ok`; `imports ok (42 serve modules)` | 1 | `before.txt` / `after.txt` in the session's scratch directory, not kept |
| 2026-11-20 | T3, the owner's `./run-hy4` (container `basalt`, started 21:02:00 PST, NFS pack); agent: `docker inspect -f '{{.Image}} {{.State.StartedAt}} {{join .Args " "}}' basalt`, `docker logs basalt`, `storage/cache/basalt/9f5b71de33f93963/engine.log` (lines 1–389), `nvidia-smi`, `free -g` | image `sha256:2c2253095d35…`; args `serve /models/Hy4-flash-NVFP4.basalt --host 0.0.0.0 --port 8700 --alias cosmo-6000 --allowed-host cosmo.lan --allowed-host cosmo --gpu 0 --max-context 362144 --kv fp16 --parallel 1 --vision dynamic`. Server: `cosmo-6000  ctx=362144 kv=fp16 spec=6 temp=0.9 top_p=1.0 vision=gpu0`; `auto     ram-experts=0.0GB pool=15`; `ram      floor 2.0 GiB of 96.9 GiB free`; `loaded   86.2GiB 105.8s` (495–874 MB/s on the progress bar); `weights  loaded, ctx=362144 images=on`. Engine: `1255 dense tensors, 7.09 GiB on the GPU`; `drafter: 5 layers reading target layers 25..41, 0.87 GiB on the GPU`; `expert homes of 512 per layer: main card 512, second card 0, RAM 0, file 0`; `VRAM expert tier 77.84 GiB, 512 of 512 experts per layer (100%); CPU pool 15 workers`; `elastic KV: about 362144 cells fit in the 1 session (362144 at full context) …`; `6395 MiB of VRAM free with everything loaded`. First request: `s1 done  pp=456/0c 8.6s  tg=30 24.3t/s acc=54% dl=3.0  ttft=8.7s  end=stop  hit=100.0%  pcie=0.0%`; engine: `prompt 456 tokens = 0 reused + 456 read in 8617 ms (52.9 tok/s), 30 generated in 1233 ms (24.3 tok/s), drafts accepted 21 of 39`, with `8326 ms in the Engram's host step`; then `no room for the expert ring (4.1 GB): the prompt stages its experts per layer` | 1 | **The build that loaded:** the image id is the one labelled `ai.basalt.ref=ad274aa…` (Plan 10 tracker, Evidence row 5). No flag refused; the context was not capped. Host after the first request: GPU 0 95355 of 97887 MiB used; RAM 30 GiB used, 95 GiB available — upstream's "RAM holds every expert" did not happen on this card (`RAM 0`). 30 generated tokens is too few to call a decode speed. 456 tokens × 64 rows in 8.3 s is about 3.5k reads/s, the same order as the ~2.9k IOPS `README.md` records for this NFS |
| 2026-11-20 | T3, the second request on that boot (`docker logs basalt`; engine log lines 390–402) | `thinking budget 182016 tokens, output cap 242688`; `s1 read  pp=14150 10s` … `pp=14150 60s`; `basalt serve: error: no progress for 60 s during a request (reading the prompt, from token 0) - stopping the engine so the server starts it again`; `stall report (engine 0.5.0): stage "reading the prompt, from token 0" for 60 s; 0 layers served since the last finished step`; `threads waiting on the disk (state D): 256 of 280 (nfs_file_direct_read x256) - the engine is waiting for the drive: a rotational or failing disk with --ple-io direct (--ple-io ram), or a drive too slow for the reads asked of it`; `memory: 3374 MiB resident, 0 MiB in swap, 99539 MiB RAM available`; server: `s1 done  pp=14150/0c  tg=0  end=error`; `the engine was killed by SIGABRT`; `loaded   86.2GiB 33.8s`; `engine running again`; then `thinking budget 182016 tokens, output cap 242688` again | 1 | the plan's Open Question 1, observed. 14,150 tokens × 64 rows is about 906k reads; at 3.5k reads/s that is over 4 minutes against a 60 s watchdog, so by this arithmetic the limit is near 3,300 cold prompt tokens (an estimate from one boot, not measured). The stall report's advice, `--ple-io ram`, is the flag this model does not have. The reload took 33.8 s against 105.8 s for the first load. The client re-sent the prompt after the restart; what happened next was not read |
| 2026-11-20 | after the owner stopped the server: `fio --readonly --rw=randread --bs=4k --direct=1 --ioengine=libaio --offset=92641689600 --size=63360615936 --randrepeat=1 --randseed=<s> --iodepth=<qd> --number_ios=<n>` on the NFS pack (the `engram` section only) | seed 4242, QD 256, 30000 reads, first touch: **1053 IOPS**, mean latency 243 ms. The same 30000 offsets again: **163934 IOPS**, 1.55 ms. The same offsets at QD 1 (5000 reads): 18382 IOPS, 0.05 ms a read. Fresh offsets (seed 777) at QD 1, 300 reads: 80 IOPS, **12.4 ms** a read | 1 each | the link, RDMA and NFS are not the limit: a page noir already holds in RAM comes back in 0.05 ms. A page it does not hold costs a seek on noir's spinning array. `O_DIRECT` skips cosmo's page cache, so cosmo's RAM never helps; noir's would, but nothing had read the 59 GiB `engram` section — the load reads the other sections. A direct read from cosmo does fill noir's cache (the second pass shows it) |

## Notes / Deviations

- **2026-11-20 (T1) — step 5b passes no GPU to `model_plan`, where the plan's sketch passed `a.gpu`.**
  The step is about flags; with `None` the engine picks its own card and the check does not depend on
  which cards the container sees. The plan line it prints still listed the RTX PRO 6000: on this host
  a container sees the cards without a `--gpus` flag. Not explored further.
- **2026-11-20 (T3) — the plan's default pack does not serve real prompts.** Open Question 1 was
  written as a risk with a note for mitigation; the first boot showed a 14,150-token prompt cannot be
  read from NFS inside the watchdog's 60 s. The launcher's flags are right and the boot is clean, so
  T1 stands; what is wrong is the `PACK` default, and by the plan's own T3 text that is the owner's
  decision, not an edit made here. `run-hy4` still defaults to the NFS copy and now says what was
  measured.
- **2026-11-20 (T3) — "NFS is too slow" was half the story, as it was in Plan 01.** Rows 8–9 put the
  stall on NFS; row 10 splits it: 164k IOPS for pages in noir's RAM, about 1k for pages that are not.
  So there are two fixes, not one: a local copy, or reading the pack's `engram` section once so noir
  holds it (`dd … iflag=direct,skip_bytes,count_bytes skip=92641689600 count=63360615936`, from cosmo;
  not run — a 59 GiB read of noir's array is the owner's step). The second lasts until noir evicts
  the pages or reboots. Neither has been booted against.
- **2026-11-20 (T3) — the owner booted before being asked and pasted nothing.** Everything in rows
  8–9 was read by the agent from the running container and its log; `./run-hy4`'s `==` lines are
  inferred from the container's arguments, not seen. No reply text exists in the logs.
- **2026-11-20 (T3) — a plan Key Fact did not hold on this card:** upstream's design figure "RAM holds
  every expert (NVFP4 77.9 GiB)". Here the expert tier is all VRAM (`main card 512 … RAM 0`), the
  engine's resident memory was 3374 MiB at the stall, and Open Question 2 (host memory) did not bite.
- **2026-11-20 (T3) — the Evidence row numbers cited by T1 and T2 were one low until this commit.**
  Their rows were inserted above the section-tables row, which made it sixth and then seventh. It is
  third again, so "rows 4–6" (T1), "row 7" (T2) and "rows 8–9" (T3) now point where they say.
- **2026-11-20 (T1) — `./run` still takes a Hy4-flash pack to the engine.** T2 adds its refusal;
  `README.md` says so until then.

- **2026-11-20 — the plan was written from source and docs at the pin plus no-load probes; nothing
  here has booted Hy4-flash.** Every memory and speed figure in the plan's Key Facts is upstream's.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Move the launcher body `run` and `run-hy4` share (docker args, the masked print, the exec) into `lib/common.sh` | about 60 duplicated lines; left alone while `./run`'s boots are owed to Plans 08 and 10 | after those boots; a plan (it rewrites `./run`) |
| Copy the pack to `models.local/` and decide `run-hy4`'s default `PACK` (the local copy when it exists, or always) | T3: the NFS copy stalls past a few thousand prompt tokens; `/home` had 299 GB free on 2026-11-20 and the pack is 148 GiB. `/home` is btrfs with `compress-force=zstd:1`, and Basalt reads with `O_DIRECT` — check the copy's extents (`filefrag`) before trusting a boot time | the owner (copy, decision); then T3 of this plan |
| Boot `PARALLEL=3 ./run-hy4` | upstream lists no parallel slots for Hy4-flash; the engine accepts the flag (plan Open Question 4) | owner decision; a later plan or Plan 05 |
| Choose Hy4-flash's context and KV format on measurements (`MAX_CONTEXT=1048576`, `KV=fp8`) | the defaults are starting values (plan Open Question 3) | owner decision; Plan 05 |
