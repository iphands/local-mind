# Plan 01 — NInfer Flash-Next: pinned sm_120a image, containerized conversion, `./run`, measured — Tracker

## Overview

- Status: 42% complete (5/12 tasks)
- Start date: 2026-10-27
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
4. Commit per task as `[ninfer][P1][T<n>][<topic>] <summary>` (the `[ninfer]` prefix on every
   commit since 2026-10-27, RULES Rule B), with **this tracker's row in the same commit**. Never push; report the hash and that you did not run `git push`.
5. Restore the system state after the session (P2.4): no leftover `ninfer`/`ninfer-convert`
   containers, the NVMe artifact copy deleted unless T11 made it the default, `/tmp/ninfer-gate*`
   removed.

## Open Unknowns

- Do all 48 × 512 main experts fit in the GPU cache on 96 GB with the T5 defaults? (T8)
- Does the converter run under Python 3.12 / torch 2.14? (T4 import, T7 real run) — **import: yes**
  (T4: every `tools.convert` / `tools.artifact` module imports, torch 2.14.0 / CUDA 13.2, NumPy 2.2.6);
  the real run is still T7.
- Is `ignore_eos` ignored or honored by NInfer's chat endpoint? (T9) — docs say "Unknown top-level
  fields are ignored" (`docs/serving.md:210` @ `64d10fbf`, found in T1); T9 still checks it live.
- How long does the conversion take, and what does it peak at? (T7)
- ~~Which moby tag is docker 29.8.0's seccomp source? (T2)~~ RESOLVED (T2): `docker-v29.8.0`
  (`docker-v29.8.0^{}` = `3ce5872b…` = the server's `GitCommit`).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Pins, shared guards, README, `models`, knowledge seed | `versions.env`, `lib/common.sh`, `README.md`, `context/*` | done | Both sudo arms, blank and array `DOCKER`, and a missing binary → rc 1; both CUDA digests resolve; gate 1 `1 b` + `env ok`, gate 2 `shellcheck ok`. `MOBY_SECCOMP_REF` moved to T2 (see Notes). |
| 2 | T2: seccomp profile — docker default + io_uring | `scripts/seccomp-profile`, `seccomp/ninfer.json` | done | Gate 3 `seccomp ok`: default blocks both, ours `uring ok` + `keyctl` blocked, unconfined `keyctl ok`. Regeneration byte-identical (sha256 ×3). Source path invalid as written (see Notes). |
| 3 | T3: `Dockerfile` + `build` | `Dockerfile`, `build` | done | `PREFLIGHT_ONLY=1 ./build` rc 0 (clean at `64d10fbf`, both bases resolve, `--check` no warnings, tag `cu1343-sm120a-64d10fb`); `DIRTY` file → refusal rc 1; 0 images built. Gate 1–4 pass. **Runtime tier not run: the image has not been built** (T6). |
| 4 | T4: `scripts/convert` — conversion in a container | `scripts/convert` | done | Today's tree → BASE-missing refusal + download command, rc 1; NVFP4-as-base → names missing `gate_up_proj` and `F8_E4M3` PLE, rc 1; real NVFP4 passes the `nvidia` check; real `vllm` → busy refusal; `tools.convert --help` rc 0 in the vLLM image. Gate 1–5 pass. Two premises invalid as written (see Notes). **Conversion not run** (T7). |
| 5 | T5: `run` — the launcher | `run` | done | Gate 6 `launcher dry-run ok`: 17 wanted flags, `DRAFT_TOKENS=0` → no `--spec`, `EXTRA_ARGS` last, both sudo arms / running `vllm` / no image / bad knob / missing part refused. Today's tree → "run ./build first", rc 1. Gate 1–6 pass. **Runtime tier not run: nothing has booted** (T8). |
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
| 2026-10-27 | T2 gate step 3 (`docker run --rm --network none … --entrypoint python3 <vllm …g40fe396> -I -c '<syscall 425, 250>'`), docker 29.8.0, profile `seccomp/ninfer.json` sha256 `0a369b29…` | default: uring + keyctl `Operation not permitted`; ninfer.json: `uring ok`, keyctl `Operation not permitted`; unconfined: `uring ok`, `keyctl ok` | 2 | a fact, not a measurement; identical on both gate runs |
| 2026-10-27 | T4: `importlib.import_module` of `tools.convert.{pipeline,official_recipes,qwen4_exp,quantization.stored_scale,sources.modelopt_nvfp4,sources.fp8_block}` + `tools.artifact.inspect` in `…vllm…g40fe396`, `--user 1000:1000 --network none`, vendor `:ro` | all `ok`; Python 3.12.3, torch 2.14.0 (CUDA 13.2), NumPy 2.2.6; `qwen3_8_flash_next` recipe present; `tools.convert --help` rc 0 | 1 | import-level only; T7 is the real run |
| 2026-10-27 | T4: HF range reads (`curl -r`) of `Qwen/Qwen3.8-Flash-Next@de4b8e4d` shard headers 00002 and 00005 | `…layers.0.mlp.experts.gate_up_proj` BF16 `[512, 1280, 2560]`; `…layers.1.ple.ple_embedding.ngram_embedding.shard_0.weight` BF16 `[2500012, 160]`; arch `Qwen4ExpForConditionalGeneration`, 1,658 keys, 131 shards | 1 | the shapes `scripts/convert` checks |
| 2026-10-27 | T4: `DRY_RUN=1 BASE=<crafted BF16 headers> ./scripts/convert` against the real `models/vllm/nvidia/Qwen3.8-Flash-Next-NVFP4` | `== NVIDIA ok` (48 NVFP4 main-expert layers, 294,912 keys); then busy refusal on the real running `vllm` | 1 | 0.3 s for the header checks |
| 2026-10-27 | T4: `hf download Qwen/Qwen3.8-Flash-Next --revision de4b8e4d… --local-dir /dl/… --dry-run` in the vLLM image | resolves the revision and lists the files; nothing but 8 KB of lock files written (scratch, removed) | 1 | proves the printed download command |
| 2026-10-27 | header probe of `models/vllm/nvidia/Qwen3.8-Flash-Next-NVFP4` and `…-FP4-Local` (python, headers only) | NVFP4: per-expert U8/F8/F32 experts, F8 block MTP experts, F8_E4M3 PLE + 1 BF16 scale, rest BF16. FP4-Local: CSF expert scales, MXFP8 dense, U8 NVFP4 PLE | 1 | planning probe |

## Notes / Deviations

- 2026-10-27 (planning): the Reddit post that prompted this (`r/Qwen_AI` `1xgwo7o`) could not be
  read — Reddit refuses the fetcher and no mirror was found. Its engines and numbers are **not**
  in this plan; the NInfer README, two gists and upstream issues #394/#399 stand in.
- 2026-10-27 (planning): the owner moved this work out of `../vllmcustom` into its own sub-project
  (`server/ninfer`) with its own plan system.
- 2026-10-27 (T1): `MOBY_SECCOMP_REF` is **not** in `versions.env` yet. The plan lists it under T1 but
  says it is "confirmed in T2"; writing an unconfirmed tag would be a pin without provenance, so T2
  adds it. CUDA digests are the **manifest-list** digests (`…devel…@sha256:5626a7d0…`,
  `…runtime…@sha256:f58bd7a0…`), so the pin holds for any platform docker resolves.
- 2026-10-27 (T1): gate step 2 failed on the first run — SC2034 (`RUN_GID` unused: it is read by the
  scripts that source the lib) and SC1091 (a `source=` directive is cwd-relative without
  `source-path=SCRIPTDIR`). Both fixed in `lib/common.sh` before the commit.
- 2026-10-27 (T2): **premise wrong — the plan's source path is invalid as written.**
  `profiles/seccomp/default.json` is a 404 at `docker-v29.8.0` (`curl -w '%{http_code}'` → `404`,
  and `daemon/profiles/seccomp/default.json` likewise); the tag's tree (GitHub trees API) has the
  profile only at `vendor/github.com/moby/profiles/seccomp/default.json` — moby moved it to the
  `github.com/moby/profiles` module. `scripts/seccomp-profile` uses the vendored path. Source
  sha256 `536529b6…`, 13,470 B, one unconditional `SCMP_ACT_ALLOW` entry (361 names), `keyctl` in no
  allow entry. The three names are appended (upstream's list is not in Python sort order).
- 2026-10-27 (T2): gate step 2 failed once on SC2016 (Markdown backticks inside single quotes in the
  README generator); fixed with a targeted directive. `MOBY_SECCOMP_REF` added to `versions.env`
  and to `load_pins`' required list.
- 2026-10-27 (T3): Ubuntu package versions were read from the **local**
  `nvidia/cuda:13.2.0-devel-ubuntu24.04` (`apt-get update` + `apt-cache policy` as uid 1000 with
  private apt dirs), not the 13.4.3 devel image the plan names — same Ubuntu 24.04.4 archive, and
  it avoided a ~7 GB pull for a lookup. The CUDA layer does not change these packages; T6's build
  log is the real check. Candidates: cmake 3.28.3, ninja 1.11.1, pkg-config 1.8.1, g++ 13.2.0,
  liburing-dev/liburing2 2.5, libhwloc-dev/libhwloc15 2.10.0, libcurl 8.5.0, FFmpeg 7:6.1.1
  (libavcodec60/libavformat60/libavutil58/libswscale7) — every runtime package name in the
  Dockerfile exists.
- 2026-10-27 (T3): `docker build --check` failed (rc 1) on the first preflight with two
  `InvalidDefaultArgInFrom` warnings — the empty `CUDA_*_IMAGE` defaults are deliberate (a bare
  `docker build` must fail, not build unpinned). Fixed with `# check=skip=InvalidDefaultArgInFrom`
  and a stated reason, rather than a default that would duplicate a `versions.env` pin.
- 2026-10-27 (T3): the plan's Dockerfile sketch had no `ARG CUDA_RUNTIME_IMAGE` inside the final
  stage; a global ARG reaches only `FROM` lines, so the `ai.ninfer.base` label would have been
  empty. Redeclared in the stage. The `ai.ninfer.cuda` label comes from a `CUDA_VERSION_TAG`
  build-arg (`13.4.3`, parsed from the runtime tag), named to avoid the base image's own
  `CUDA_VERSION` env.
- 2026-10-27 (between T3 and T4): **owner directive** — every new commit title in this project
  starts with `[ninfer]` (e.g. `[ninfer][P1][T4][convert] …`). Written into RULES Rule B; the five
  earlier commits keep their old form (append-only history).
- 2026-10-27 (T4): **premise wrong — "`hf_quant_config.json` says NVFP4" is invalid as written.**
  Its top-level `quantization.quant_algo` is `MIXED_PRECISION`; the per-module
  `quantized_layers` map has 48 `…layers.N.mlp.experts` entries at `NVFP4`, 1 `FP8`, 1
  `FP8_BLOCK_SCALES`. The converter never reads this file (`grep` over `tools/convert/sources/`);
  `scripts/convert` checks the 48 per-layer entries instead.
- 2026-10-27 (T4): **premise wrong — the printed `hf download …` command is invalid as written:**
  the host has no `hf` (`command -v hf huggingface-cli` → nothing). The converter image does
  (`/opt/venv/bin/hf`), so the refusal prints a containerized `docker run … --entrypoint hf <image>
  download … --local-dir /dl/Qwen3.8-Flash-Next` with `models/vllm/Qwen` mounted; `--dry-run` of it
  resolved the pinned revision (Evidence).
- 2026-10-27 (T4): deviation — no host `/usr/bin/time -v`: it would measure the docker client.
  The container runs `tools.convert` as a child of a 6-line `python3 -c` wrapper that prints the
  child's peak RSS (`RUSAGE_CHILDREN.ru_maxrss`) and wall time; the host adds its own wall time.
  Both go to `captures/convert_<date>_<name>.log` (appended per attempt).
- 2026-10-27 (T4): additions beyond the plan text, each small: `-it` only when stdin is a TTY (a
  `nohup` run would otherwise die on "input device is not a TTY"); `DRY_RUN` creates no directory
  (checks the nearest existing ancestor); `MIN_FREE_GB` knob (the gate's `/tmp` is a 66 GB tmpfs);
  `INSPECT_ARGS` (default `--json`); `refuse_busy <port> <names…>` in `lib/common.sh` (refuses
  `vllm`, `ninfer`, `ninfer-convert`, anything on `:8700`; T5 reuses it). The busy check runs
  after the input checks, so a missing base is reported even while vLLM is up.
- 2026-10-27 (T5): `ninfer-serve` takes the artifact as `argv[1]` (`src/serve/serve_options.cpp:188-189`
  @ `64d10fbf`), so the launcher puts `/models/<entry>` straight after the image; every flag it
  emits exists in that file's usage text, and duplicates are assigned in order (last wins), which
  is what `EXTRA_ARGS` relies on. The `.conversion.json` `files[].path` are the converter's
  container paths (`/out/…`, `tools/convert/pipeline.py:117-123`), so the part check matches by
  basename.
- 2026-10-27 (T5): additions beyond the plan text: every knob is validated against
  `ninfer-serve`'s contract (ranges, enums, PREFILL_CHUNK % 128) before docker is called;
  `refuse_busy` also refuses a running `ninfer`/`ninfer-convert` and anything on `$PORT`; `-it`
  only with a TTY; `ENV_EXTRA` entries must be `KEY=VAL`. `ensure_dir` (DRY_RUN creates nothing)
  moved from `scripts/convert` into `lib/common.sh` for both scripts; gate 5 re-passed after the
  move. `DRY_RUN=1 ./run` refuses until an image exists (the plan's "refuse unless image_for_pin
  finds an image" applies in both modes).

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| `VISION=0`, `MAX_CONCURRENCY=1`, smaller `MAX_CONTEXT` as one-change arms | only if T8 shows experts spilling to the CPU | Plan 01 T8 → a later plan |
| `PREFILL_CHUNK=8192` arm (issue #394's setting) | TTFT/prefill, not decode; out of the north star's first cut | later plan |
| `KV_DTYPE=nvfp4` at long context | less KV bandwidth at depth; quality unmeasured | later plan |
| `--host-context-mib` above the 8 GiB default | issue #394: prefix reuse degrades once the host context budget is full | later plan |
| A Python 3.11 converter image | only if T4/T7 show the 3.12 image cannot convert | Plan 01 T7 → a later plan |
| An alias shim for `claude-heavy` etc. on `:8700` | NInfer accepts one model id; `llama-proxy` already sends `cosmo-6000` | revisit if a direct client breaks |
