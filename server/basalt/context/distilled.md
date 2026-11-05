# Distilled — confirmed facts for basalt

Compact, confirmed learnings: formats, protocols, tool/library behavior, measured limits. Read
before new work; add an entry whenever something is confirmed — **above the `## Open Questions`
section**, which always stays last. Keep it dense.

**Provenance discipline:** every entry carries a tag — `[SOURCE]` (upstream code, cite
`path:line`), `[DOC]` (documentation), `[REPO]` (this repo's own code/config), `[LIVE YYYY-MM-DD]`
(observed on a running system). **Do not upgrade a tag without doing the work.**

<!-- Entry shape:

## <Topic — the rule in one line> [TAG][TAG]

- The fact, stated so it can be checked.
- The evidence: command + verbatim output, or `path:line`.
- What it means for this project.
-->

## Basalt loads only Qwen3.8-Flash-Next, from single-file `.basalt` packs [DOC 2026-11-04]

- Basalt (jesdga95/basalt, a fork of Strata, MIT) is "a local inference engine for the
  **Qwen3.8-Flash-Next** mixture-of-experts model" (`README.md` at `bbf2f5bb7`). A pack is one
  `.basalt` file: weights, tokenizer, MTP drafter, 8-bit hyper-connection copies, vision
  projector. Expert profiles ship with the engine (`data/profiles/qwen4exp-48x512.prof`), keyed
  `<general.architecture>-<block_count>x<expert_count>` (`docs/model-file.md:83`).
- Ready packs: HF `jesdga/Qwen3.8-Flash-Next-Basalt` — `Qwen3.8-FN-NVFP4-DQ6_K` (default, faster),
  `-NVFP4-DQ8` (~7% slower on prose, lower KLD), `-UD-Q4/5/6_K_XL`, `-Q8_0`, `-GSQ-RCO-*`.
- **Not loadable:** `models/vllm/jesdga/Qwen3.8-27B-QUASAR-DFlash2-nvfp4-NInfer/qwen3_8_27b_quasar.ninfer`
  (19,624,516,612 bytes) — a dense Qwen3.8-27B artifact for NInfer (its `README.md`:
  `library_name: ninfer`, tested at NInfer `d44ab584`). Same HF author, different engine. It belongs
  in `../ninfer`.
- No `.basalt` file exists under `/mnt/noir/scratch/ai/llm/models` yet
  (`find … -iname '*basalt*'`, 2026-11-04).

## Building Basalt: CUDA 13, sm_120, CMake ≥ 3.24, GCC ≥ 13, Python ≥ 3.10; nothing fetched [DOC 2026-11-04]

- Requirements: Linux x86-64 with AVX2 (AVX-512 used when present), compute capability 12.0,
  CUDA 13, CMake 3.24+, GCC 13+ (`bbf2f5bb7` "docs: the Linux build needs GCC 13 or newer"),
  Python 3.10+ (`README.md`).
- Build: `cmake -S . -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build -j` →
  `bin/basalt`, `bin/basalt-bench`, `bin/basalt-probe`, `bin/basalt-vision`. Server deps:
  `pip install -r requirements.txt` (pinned: numpy, jinja2, regex, pillow, psutil, …).
- "The build needs nothing outside this repository and fetches nothing" (`docs/building.md`).
- An install or container image **ships `data/` with the binaries, `serve/` and `tools/`**
  (`docs/building.md`); `--expert-profile FILE` names the profile explicitly (0.2.1).
- `basalt-vision` is built for one card at configure time (`-DCMAKE_CUDA_ARCHITECTURES=<cc>`,
  `docs/running.md:247`).
- Releases: tags `vX.Y.Z`, no binaries; 0.2.1 on 2026-11-03. Upstream keeps **no shims**: a
  removed or renamed flag is just an unknown-flag error (`AGENTS.md`, changelog index).

## Serving: `bin/basalt serve --config basalt_config.json`, binds 127.0.0.1 by default [DOC 2026-11-04]

- Config shape (`README.md`): `{"server": {"model": "<pack>", "gpu": "0", "port": 8080},
  "engine": {"expert-cache": "auto", "prefill": "auto", "kv": "int8", "resident-experts": true}}`.
  Every setting: upstream `docs/configuration/index.md`.
- "The server listens on 127.0.0.1 by default" — inside a container that means unreachable from
  the host unless it binds `0.0.0.0` or the container uses host networking.
- OpenAI- and Anthropic-compatible API plus a web UI/stats page (`serve/`).
- Experts that do not fit in VRAM live in system RAM (optionally a second Blackwell GPU as a peer
  tier). `--gpu auto` uses every visible Blackwell GPU (`docs/building.md`).

## DQ6_K pack on this card: every expert fits in VRAM; 40 s boot from local NVMe [LIVE 2026-11-04]

- Boot 3 (`./run` default: 4 slots × 262144, `kv int8`, `vision dynamic`, pack in `models.local/`):
  `loaded 67.8GiB 40.1s cuda0=67.8GiB` at ~2.7 GB/s; `expert cache 24576 slots, 63.29 GiB of VRAM` —
  48 layers × 512 experts, **all of them**, so the decode never touches the CPU tier (start warning:
  `the device-planned MoE tail is off: every expert is resident`). The 5950X's missing AVX-512 does
  not matter while that holds.
- The 4 full-context slots: `kv 1.5/15.2GiB cuda0 elastic slots=4` — 4096 cells per session backed at
  start; the other 13.73 GiB is the expert cache's last 5,334 slots, taken back as long conversations
  grow (up to 22% of the experts then run on the CPU).
- GPU 0 idle after load: 73,091 / 97,887 MiB, 73 W. One short reply: 320.7 t/s, 74% drafts accepted,
  TTFT 0.1 s (one run — not a benchmark).
- The local copy loaded fine although cosmo's `/home` (`compress-force=zstd:1`) stored 464,061 of its
  555,406 extents compressed.

## Basalt loads weights with `O_DIRECT`, and there is no switch [SOURCE][LIVE 2026-11-04]

- `include/basalt/core/direct_upload.hpp:1-6` at `dc047a77`: a load phase copies file ranges to VRAM
  through block-aligned `O_DIRECT` reads into pinned staging buffers (falls back to buffered reads
  only when the filesystem refuses `O_DIRECT`; NFS accepts it). `src/platform/direct_file.cpp:384`
  opens with `O_RDONLY | O_DIRECT`. No CLI flag or env var turns it off (`getenv` in `src/`:
  `CUDA_VISIBLE_DEVICES`, `CUDA_MODULE_LOADING` only).
- Live: boot 2 of the DQ6_K pack from `/mnt/noir` (NFS 4.2/RDMA) showed 6 engine threads in
  `nfs_file_direct_read` and a load of ~5–10 MB/s. Buffered `dd` of the same file measured 28.9 MB/s
  during the boot, so the NFS link was slow too that evening (not separated — Plan 01 tracker).
- Means: expect basalt boots from NFS to be slow; local NVMe is the layout upstream targets.

## The built image: `iphands/basalt-blackwell:cu1343-sm120-dc047a7` [LIVE 2026-11-04]

- Owner build (Plan 01 T4): 3.42 GB, `ai.basalt.ref=dc047a77…`; Basalt's compile is 507 ninja steps
  (≈ 420 s at `JOBS=16`, 0 warnings) — `BUILD_TESTING=OFF` still builds the parity programs, but
  only `bin/` ships. `bin/basalt` is 94.1 MB.
- `bin/basalt --help` and `serve.server --help` run with **no GPU** (`--network none`, no `--gpus`),
  so flag checks need no card.
- `basalt serve --mock-engine` runs the whole server with a scripted engine and no model or GPU —
  the cheapest end-to-end check of the image layout, the Host check and the alias.

## Host check and alias behave as documented [LIVE 2026-11-04]

- With `--host 0.0.0.0 --allowed-host cosmo.lan --allowed-host cosmo`: `Host: cosmo.lan:8700`,
  `cosmo:8700`, `localhost:8700` → 200; `evil.example:8700` → **403** (mock engine, T4).
- With `--alias cosmo-6000`, `/v1/models` lists only `cosmo-6000`; a chat naming `claude-heavy` →
  **404**.
- The start line prints `ready http://127.0.0.1:8700/v1` even when bound to `0.0.0.0`; the
  `network` line gives the container address. Without `--api-key`:
  `[server] warning  no api key, open to the network (--api-key KEY)`.
- `docker stop` ends the server in < 1 s (SIGTERM handled; `serve/server.py:301`).

## Image build inputs: BuildKit honors `Dockerfile.dockerignore`; the runtime base has no Python [LIVE 2026-11-04]

- The build context is `vendor/basalt`, which has no `.dockerignore`. BuildKit reads
  `<Dockerfile>.dockerignore` beside the `-f` Dockerfile: with ours, the context is 11.75 MB and
  holds no `.git` (scratchpad probe build, Plan 01 T2 Evidence).
- `nvidia/cuda:13.4.3-runtime-ubuntu24.04@sha256:f58bd7a0…` has no `python3`; apt's candidate is
  3.12.3 (`python3`, `python3-venv`). The devel base: cmake 3.28.3, g++ 13.2.0, ninja 1.11.1,
  nvcc 13.4.92 — all above Basalt's minimums.
- `PREFLIGHT_ONLY=1 ./build` → `preflight ok` at `dc047a77`; `docker build --check` sends no context.

## This host: one sm_120 card, and one Basalt server per card [LIVE 2026-11-05][REPO]

- Booting a second server on GPU 0 is not possible while one runs, in **any** mode: the load is the
  expert cache, and `--single` shrinks KV slots, not the cache. Read on the owner's live server:
  `expert_slots_primary 24576`, `expert_cache_primary_mib 64806`, `arena_mib 23353`, against
  `vram_free_mib 24435`; `nvidia-smi` showed 77,117 / 97,887 MiB used. Consequence for plans: a
  comparison of two modes is **sequential** — stop, load, measure, stop, load — never concurrent.
- `./run` enforces it before docker starts: `refuse_busy "$PORT" vllm ninfer ignis basalt` (`run:96`),
  so a clashing boot exits 1 with `## busy:` and touches nothing. In `DRY_RUN` it *reports* the clash
  instead of refusing (`run:92-97`) — which makes `DRY_RUN=1 ./run` the port oracle for this project.
- Expert residency at 4×262144 is **tiered, not "all in VRAM"**: a 24,576-slot / 63.29 GiB primary cache
  resident on cuda0 (`expert cache 24576 slots, 63.29 GiB of VRAM`) *and* `resident RAM: 22.81 GiB of
  experts in RAM, 0 exchanged with the VRAM tier`; `basalt verify: warning: the device-planned MoE tail
  is off: every expert is resident`. No starvation warning at 4 slots, and the extra slots are cheap:
  `--batch 4: 0.12 GiB of KV cache and state per slot (262144 tokens), 0.49 GiB in all; the slots beyond
  the solo one take 0.37 GiB of VRAM on CUDA0 that the expert cache would otherwise hold`.
- **Provenance limit on the three bullets above**: they were read from the **owner's running
  daily-driver** on 2026-11-05 (`/metrics` + its `engine.log`), launch provenance unconfirmed — the
  card held three `--batch 4` boots logged that morning (*UTC morning = 2026-11-04 evening PST; the
  "daily-driver" was Plan 01's T6 test boot 3 — host agent session, 2026-11-04*), none `--single`. They corroborate Plan 01's
  boot 3; they are not a substitute for T6's directed boots, and `--single`'s geometry is still unknown.

## This host: one sm_120 card, 125 GiB RAM [LIVE 2026-11-04]

- `nvidia-smi`: GPU 0 RTX PRO 6000 Blackwell Workstation, 97,887 MiB, cc 12.0; GPU 1 RTX 4060,
  8,188 MiB, cc 8.9 (not usable by Basalt). `free -g`: 125 GiB total.
- docker 29.8.1, shellcheck 0.11.0, python3 3.14.10; no `gh`, no `hadolint`.
- **Tool versions here are the 2026-11-04 shell's view, not a host constant** (checked 2026-11-05 from
  the agent container, which reports docker client 29.9.2 and keeps `shellcheck` at `~/.local/bin`, and
  has no `ss`). Re-check in the shell that will use the answer — see
  `context/pitfalls.md` § *Empty output from a failed command was recorded as a measurement*.

---

## Open Questions

- Does an NVFP4 Flash-Next pack fit entirely in 96 GiB VRAM, or which share lands in RAM, and
  what `ulimit -l` / `--ulimit memlock` does the container need for the RAM tier to page-lock?
  (`docs/running.md` covers RAM floors; `platform_memory_test` notes the locked-memory limit.)
  Settle in the first plan.
- Which of the image's upstream defaults (`expert-cache`, `prefill`, `kv`, `--spec` depth) are
  already right for this card? `bin/basalt-probe` "suggests values" (`docs/probe.md`) — owner run.

<!-- Things we believe but have not confirmed. Check here before trusting a related assumption;
     move an item up (with its tag) once it is confirmed, or into pitfalls.md if it bit. -->
