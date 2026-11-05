# Plan 01 — Basalt Flash-Next container: pinned sm_120 image, `./run` on :8700 — Tracker

## Overview

- Status: 57% complete (4/7 tasks)
- Start date: 2026-11-04
- Plan: `context/plans/01_flashnext_container.md`
- Evidence: the table below; build logs in `captures/build_<date>_<sha7>.log` (gitignored); boot
  start blocks pasted by the owner into the Evidence rows. Test environment: host `cosmo`, RTX PRO
  6000 (GPU 0), 125 GiB RAM, docker 29.8.1, the pack on NFS (`models/vllm/jesdga/`).

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), then `01_flashnext_container.md`, then
   `context/distilled.md` and `context/pitfalls.md`.
2. Environment: docker as yourself (no `sudo`); `shellcheck` 0.11.0. The reference scripts are
   `../ninfer/{lib/common.sh,build,run,Dockerfile}` and `../vllmcustom/common/_run-lib.sh` — read,
   never sourced or edited.
3. Ordering: Wave 1 (T1 → T2 → T3) is agent work and needs no GPU. T4 needs the owner's `./build`;
   T5 needs T4's image; T6 needs the owner to boot with `:8700` free (stop `vllm`/ninfer/ignis
   first); T7 last. Mark a task `blocked` (with the reason) while it waits on the owner.
4. Commit per task as `[basalt][P1][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**. Never push.

## Open Unknowns

- ~~Does `bin/basalt --help` run without a GPU? (plan Q4 — T4)~~ RESOLVED (T4): yes — `docker run --rm --network none … --help` → rc=0, 198 lines, no `--gpus`.
- Does the RAM tier page-lock inside the container with `--ulimit memlock=-1`? (plan Q2 — T6)
- VRAM/RAM split and KV size for 4×262144 vs 1×262144 on this card (plan Q1, Q3 — T6)
- ~~Runtime image's Python version (Key Facts — T2)~~ RESOLVED (T2): the runtime base has no python3; apt candidate `python3`/`python3-venv` 3.12.3-0ubuntu2.1 (Evidence).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Pins, shared guards, project skeleton | `versions.env`, `lib/common.sh`, `.gitignore`, `models`, `README.md` | done | Digests unchanged since ../ninfer pinned them; sudo / missing-docker guards refuse (rc=1); gate `1 b`, `env ok`. |
| 2 | T2: Dockerfile + `./build` | `Dockerfile`, `Dockerfile.dockerignore`, `build` | done | Preflight ok at `dc047a77`; dirty (untracked) checkout refused; context probe: 11.75 MB, no `.git`; no image built. Gate step 3 added. |
| 3 | T3: `./run` + its dry-run gate step | `run`, `README.md`, RULES gate | done | Gate step 4 added and passing with `image: stub:none` (no image yet); negative control `KV=fp16` → 2 FAIL lines; busy/ninfer/missing/not-a-pack all rc=1. |
| 4 | T4: Owner build; verify the image is the pin | tracker Evidence, `distilled.md` | done | Owner built `cu1343-sm120-dc047a7` (420 s compile, 0 warnings); label = pin; `--help` runs without a GPU; mock-engine serve proves layout, Host check (403 for unknown names) and alias (404). |
| 5 | T5: Check `./run`'s flags against the built image | RULES gate step 5 | pending | needs T4 |
| 6 | T6: Owner boots — default and `--single` — with a real reply each | tracker Evidence, `distilled.md` | pending | owner boots |
| 7 | T7: Harvest | `context/*.md`, `README.md` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-04 | `docker buildx imagetools inspect` on both `nvidia/cuda:13.4.3-{devel,runtime}-ubuntu24.04` tags | `sha256:5626a7d0…` / `sha256:f58bd7a0…` — identical to `../ninfer/versions.env` (2026-10-27) | 1 | — |
| 2026-11-04 | throwaway `docker run --rm <runtime digest>`: `command -v python3`; `apt-cache policy python3 python3-venv` | no python3 in base; candidates 3.12.3-0ubuntu2.1 | 1 | — |
| 2026-11-04 | throwaway `docker run --rm <devel digest>`: `apt-cache policy cmake g++ ninja-build`; `nvcc --version` | cmake 3.28.3-1build7, g++ 13.2.0-7ubuntu1, ninja 1.11.1-2; nvcc 13.4.92 | 1 | — |
| 2026-11-04 | `PREFLIGHT_ONLY=1 ./build` (fresh clone) | `vendor/basalt` at `dc047a77…`, both bases resolve, `Check complete, no warnings found.`, `preflight ok`, rc=0 | 2 | identical |
| 2026-11-04 | `touch vendor/basalt/.dirty-probe && PREFLIGHT_ONLY=1 ./build` | `## vendor/basalt is dirty … ?? .dirty-probe`, rc=1; probe file removed after | 1 | — |
| 2026-11-04 | context probe: scratchpad `Probe.Dockerfile` + a copy of `Dockerfile.dockerignore`, context `vendor/basalt`, untagged | `transferring context: 11.75MB`; `/c` lists no `.git`, 13M | 1 | — |
| 2026-11-04 | `DRY_RUN=1 IMAGE_OVERRIDE=x:y ./run` / `--single` | default: `--parallel 4 --slot-full-context`; single: `--parallel 1`, no `--slot-full-context`; pack 131681979197 bytes; `storage/` not created | 1 each | — |
| 2026-11-04 | `./run` refusals: the QUASAR `.ninfer` path, `/nonexistent/x.basalt`, `README.md`, no image, `PORT=99999`, `--single` + `PARALLEL=2` | each rc=1 with its message (`.ninfer` names `../ninfer`; `README.md`: first bytes `# basalt`, want `BASALTMF`) | 1 each | — |
| 2026-11-04 | sleeper container named `basalt` (`--network none`, `sleep 60`), then `DRY_RUN=1 ./run` and `./run` | dry: "a real run would refuse", rc=0; real: `## busy: … basalt …`, rc=1, nothing created | 1 | — |
| 2026-11-04 | project gate steps 1–4 (step 4 from RULES verbatim) | `3 b`, `env ok`, `shellcheck ok`, `preflight ok`, `dry-run ok (image: stub:none)`; `KV=fp16` → `FAIL(dry:default)`/`FAIL(dry:single) missing: --kv int8` | 1 (+1 negative) | — |
| 2026-11-04 | owner `./build` → `captures/build_2026-11-04_dc047a7.log` | `iphands/basalt-blackwell:cu1343-sm120-dc047a7` (`27770926a3f6`, 3,418,642,358 bytes); `basalt: CUDA arch 120`; 507 ninja steps, compile step `DONE 420.2s`; 0 lines matching `warning`; `bin/`: basalt 94.1 MB, basalt-bench, basalt-probe, basalt-vision; pip installed upstream's 8 pins | 1 | apt layers cached from earlier probes |
| 2026-11-04 | `docker image inspect` labels; `docker run --rm --network none` → `bin/basalt --help`, `.venv/bin/python -m serve.server --help`, `ls -A /opt/basalt /opt/basalt/data/profiles`, `id` | `ai.basalt.ref=dc047a77…`; both `--help` rc=0 (no GPU); `.venv bin data serve LICENSE THIRD_PARTY_LICENSES requirements.txt`; `qwen4exp-48x512.prof`; uid 1000 (ubuntu), HOME=/home/ubuntu, Python 3.12.3 | 1 | — |
| 2026-11-04 | mock engine: `docker run -d … -p 127.0.0.1:18700:8700 <img> serve --mock-engine --host 0.0.0.0 --port 8700 --alias cosmo-6000 --allowed-host cosmo.lan --allowed-host cosmo`; curl with `Host:` headers; `docker stop` | start lines `[server] hosts cosmo.lan,cosmo`, `ready http://127.0.0.1:8700/v1`, `warning no api key, open to the network`; `/v1/models` → `cosmo-6000`; Host `cosmo.lan`/`cosmo`/`localhost` → 200, `evil.example` → 403; chat as `cosmo-6000` via `cosmo.lan` → a reply; model `claude-heavy` → 404; `docker stop` 0.66 s | 1 | the start line prints `127.0.0.1` even when bound to `0.0.0.0` |
| 2026-11-04 | T1 guards: `DOCKER='sudo docker'`, `DOCKER=/usr/bin/nonexistent-docker` sourcing `lib/common.sh` | both refuse, rc=1, with the fix line | 1 each | — |

## Notes / Deviations

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Tune past the owner's defaults (`basalt-probe` suggestions, `expert-profile-save`, `prefill-cpu-share`, `spec`, `kv` q4_0/k8v4) | Ordering principle: boot before tune | a later plan |
| `sm_120a` build A/B | Basalt accepts `120a` but tests only `120` (plan *Rejected Claims*) | a later plan |
| More than one served model name (vLLM's `claude-*` aliases) | `--alias` takes one name (plan Q6) | later, if a client needs it |
