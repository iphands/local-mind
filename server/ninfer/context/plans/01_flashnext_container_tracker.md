# Plan 01 — NInfer Flash-Next: pinned sm_120a image, containerized conversion, `./run`, measured — Tracker

## Overview

- Status: 0% complete (0/12 tasks)
- Start date: — (set when T1 starts)
- Plan: `context/plans/01_flashnext_container.md`
- Evidence: bench `--out` files and the conversion log under `captures/` (gitignored, P2); the
  launcher's echoed `docker run` line and boot logs pasted by the owner. Test environment per row:
  NInfer image tag + `ai.ninfer.ref`, artifact path + its `.conversion.json`, launcher env; for the
  vLLM arm, the `../vllmcustom` image tag and `run-fp4local` launch line.

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), `context/AGENTS.md`, then
   `01_flashnext_container.md`. The research behind the Key Facts is in the plan; the NInfer
   source at the pin is `vendor/ninfer` once T3 has run (a scratch clone before that).
2. Environment: work from `server/ninfer`. `docker` as yourself (no `sudo`). Wave 1 needs no GPU
   and no model. The converter image is `../vllmcustom/container/.last-build`'s image
   (`iphands/vllm-blackwell:cu1321-sm120-main-vllm0.34.0.dev20261027-g40fe396` on 2026-10-27).
   Standing Constraints in `SERIES.md` apply — especially: the owner runs every long or GPU step,
   and vLLM must be down for a NInfer boot or a conversion.
3. Ordering: T1 → T2 → T3 → T4 → T5 (each adds a gate step the next ones run). T6 needs T3; T7
   needs T4 and the owner's BF16 download; T8 needs T5–T7; T9–T11 need T8, and each arm is one
   change from the T9 NInfer arm; T12 last. Waves 2–3 wait on the owner: when blocked on them, mark
   the row `blocked` with what is needed, and stop.
4. Commit per task as `[P1][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**. Never push; report the hash and that you did not run `git push`.
5. Restore the system state after the session (P2.4): no leftover `ninfer`/`ninfer-convert`
   containers, the NVMe artifact copy deleted unless T11 made it the default, `/tmp/ninfer-gate*`
   removed.

## Open Unknowns

- Do all 48 × 512 main experts fit in the GPU cache on 96 GB with the T5 defaults? (T8)
- Does the converter run under Python 3.12 / torch 2.14? (T4 import, T7 real run)
- Is `ignore_eos` ignored or honored by NInfer's chat endpoint? (T9)
- How long does the conversion take, and what does it peak at? (T7)
- Which moby tag is docker 29.8.0's seccomp source? (T2)

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Pins, shared guards, README, `models`, knowledge seed | `versions.env`, `lib/common.sh`, `README.md`, `context/*` | pending | |
| 2 | T2: seccomp profile — docker default + io_uring | `scripts/seccomp-profile`, `seccomp/ninfer.json` | pending | |
| 3 | T3: `Dockerfile` + `build` | `Dockerfile`, `build` | pending | |
| 4 | T4: `scripts/convert` — conversion in a container | `scripts/convert` | pending | |
| 5 | T5: `run` — the launcher | `run` | pending | |
| 6 | T6: Owner builds; image identity (gate 7) | `RULES.md` gate 7 | pending | owner runs `./build` |
| 7 | T7: Owner downloads base + converts; inspect | artifact under `models/ninfer/` | pending | owner: 360 GB download, then `./scripts/convert` |
| 8 | T8: `scripts/smoke`; first boot; expert residency | `scripts/smoke` | pending | owner boots |
| 9 | T9: Same-session A/B vs FP4-Local | Evidence | pending | owner runs both arms |
| 10 | T10: MTP depth sweep | Evidence, `run` | pending | owner runs |
| 11 | T11: PLE storage A/B (NFS vs NVMe) | Evidence | pending | owner copies ~108 GB |
| 12 | T12: Lock defaults, document fastest setup | `run`, `README.md` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

**Mandatory in this project** (P1): its product is a number. A claim not in this table has no
provenance and cannot be cited by a later plan.

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-27 | io_uring probe: `docker run --rm --network none --entrypoint python3 iphands/vllm-blackwell:…g40fe396 -I -c '<syscall 425>'`, default profile vs `--security-opt seccomp=unconfined`, docker 29.8.0 | default `-1 Operation not permitted`; unconfined `3 ok` | 1 | planning probe (a fact, not a measurement) |
| 2026-10-27 | header probe of `models/vllm/nvidia/Qwen3.8-Flash-Next-NVFP4` and `…-FP4-Local` (python, headers only) | NVFP4: per-expert U8/F8/F32 experts, F8 block MTP experts, F8_E4M3 PLE + 1 BF16 scale, rest BF16. FP4-Local: CSF expert scales, MXFP8 dense, U8 NVFP4 PLE | 1 | planning probe |

## Notes / Deviations

- 2026-10-27 (planning): the Reddit post that prompted this (`r/Qwen_AI` `1xgwo7o`) could not be
  read — Reddit refuses the fetcher and no mirror was found. Its engines and numbers are **not**
  in this plan; the NInfer README, two gists and upstream issues #394/#399 stand in.
- 2026-10-27 (planning): the owner moved this work out of `../vllmcustom` into its own sub-project
  (`server/ninfer`) with its own plan system.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| `VISION=0`, `MAX_CONCURRENCY=1`, smaller `MAX_CONTEXT` as one-change arms | only if T8 shows experts spilling to the CPU | Plan 01 T8 → a later plan |
| `PREFILL_CHUNK=8192` arm (issue #394's setting) | TTFT/prefill, not decode; out of the north star's first cut | later plan |
| `KV_DTYPE=nvfp4` at long context | less KV bandwidth at depth; quality unmeasured | later plan |
| `--host-context-mib` above the 8 GiB default | issue #394: prefix reuse degrades once the host context budget is full | later plan |
| A Python 3.11 converter image | only if T4/T7 show the 3.12 image cannot convert | Plan 01 T7 → a later plan |
| An alias shim for `claude-heavy` etc. on `:8700` | NInfer accepts one model id; `llama-proxy` already sends `cosmo-6000` | revisit if a direct client breaks |
