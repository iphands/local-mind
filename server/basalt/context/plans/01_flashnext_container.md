# Plan 01 — Basalt Flash-Next container: pinned sm_120 image, `./run` on :8700

> **Status**: in-progress
> **Created**: 2026-11-04
> **Revised**: 2026-11-04 — owner: `./run`'s default pack is the local NVMe copy `models.local/Qwen3.8-FN-NVFP4-DQ6_K.basalt` (gitignored), not the NFS path; boot 2 from NFS loaded at ~5–10 MB/s (T6).
> **Depends on**: N/A
> **Goal**: `./run` boots the owner's `Qwen3.8-FN-NVFP4-DQ6_K.basalt` pack in our own pinned Basalt image on `:8700` — four full-context slots by default, one with `--single` — and a real chat reply comes back through the name clients use (`cosmo.lan:8700`, model `cosmo-6000`).
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## Gate status

Added 2026-11-05 (the shape Plan 02 established, and the fix `context/pitfalls.md` § *A gate
prerequisite recorded in RULES.md can silently stop being true* prescribes): so a red step is never
again mistaken for a regression someone caused.

| Step | Last run | State | Who can run it |
|---|---|---|---|
| 1a syntax | 2026-11-05 | **pass** | agent |
| 1b counts | 2026-11-05 | **pass** (`3 b`) | agent |
| 1c `versions.env` | 2026-11-05 | **pass** (`env ok`) | agent |
| 2 shellcheck | 2026-11-05 | **pass** (`shellcheck ok`, v0.11.0 at `~/.local/bin`) | agent |
| 3 build preflight | 2026-11-05 | **BLOCKED** — `image_for_pin` exit 1: `permission denied … /var/run/docker.sock` | **owner only** |
| 4 `./run` dry run | 2026-11-05 | **BLOCKED** — same docker refusal, and it fires in `DRY_RUN` too (`run:96` → `refuse_busy`) | owner only |
| 5 flags vs image | 2026-11-04 | pass on `cu1343-sm120-dc047a7`; not re-runnable here | owner only |
| 6 server imports | 2026-11-04 | pass on `63d25a81f328`; not re-runnable here | owner only |

**Why (measured 2026-11-05):** the agent session runs inside a container (Fedora 45 image,
`/.dockerenv`, hostname `d0b65a91063c`) whose `/run/docker.sock` is mode `660` numeric `0:995` while
the session is `uid=1000` with supplementary groups `1000,27`. The agent has GPU nodes (`/dev/nvidia0`,
`nvidia-smi` works) but **no docker control**. On 2026-11-04 the same project's Evidence shows the
agent running `./build` and `docker run`, so this is environment drift, not a project regression.
*Do not describe the socket by group name from here:* this sandbox's `/etc/group` resolves gid 995 to
`rtkit` and `docker` to 991, which is a sandbox fact and says nothing about the host.

---

## TL;DR

**What**: Build Basalt from a pinned commit in our own two-stage CUDA 13.4.3 image, and write
`./run`, which launches that image as a container serving the downloaded Flash-Next pack on `:8700`
with the defaults the owner chose (4 slots × 262144 tokens; `--single` = 1 slot), then prove it on
a real boot.

**Deliverables**:
1. `versions.env` (Basalt `dc047a77`, CUDA 13.4.3 devel/runtime by digest, image name), `lib/common.sh`
   (guards + helpers), `.gitignore`, `models` symlink, `README.md`.
2. `Dockerfile` + `Dockerfile.dockerignore` + `./build` (with `PREFLIGHT_ONLY=1`).
3. `./run [--single] [PACK]` with `DRY_RUN=1`, and the project-gate steps that exercise all three.
4. A recorded owner build and owner boots (default and `--single`), each with a real reply, and the
   findings in `context/`.

**Estimated effort**: Medium (1 day of agent work), plus the owner's build (~20–40 min, unmeasured)
and two boots of a 131.7 GB pack over NFS.

---

## Scope

**In**: `server/basalt/` only — new top-level scripts, `lib/`, `Dockerfile*`, `versions.env`,
`README.md`, `.gitignore`, `context/`.
**Out**: `../ninfer`, `../vllmcustom`, `llama-proxy`, `opencode` configs; tuning beyond the
owner's chosen defaults (a later plan); the QUASAR 27B `.ninfer` artifact (SERIES: out of scope).
If a fix needs to cross this line, **stop** and record the blocker in the tracker.

---

## Context

The owner downloaded `models/vllm/jesdga/Qwen3.8-FN-NVFP4-DQ6_K.basalt` (2026-11-04) and wants a
`./run` that launches a Basalt container using it, in a container "optimized for Basalt", on `:8700`,
taking hints from `../vllmcustom` (SERIES north star). This plan builds the image and the launcher;
it chooses no tuning values beyond the owner's decisions below (ordering principle: boot before
tune). Relied on: `context/distilled.md` (Basalt facts, host facts), `context/pitfalls.md` (the
`.ninfer` lookalike), `../ninfer/lib/common.sh`, `../ninfer/Dockerfile`, `../ninfer/build`,
`../ninfer/run` and `../vllmcustom/common/_run-lib.sh` (reference shapes — read, never sourced).

**Owner decisions (2026-11-04, this plan's drafting):** pin Basalt `main` at `dc047a77` (not the
`v0.2.1` tag); `./run` defaults to `--parallel 4 --slot-full-context --max-context 262144`, and
`./run --single` serves one slot at 262144; vision `dynamic`.

### Pre-Identified Bug/Issue

Feature: nothing launches Basalt today, and no image exists.

```bash
$ ./run
/bin/bash: line 1: ./run: No such file or directory          # exit 127
$ ls
AGENTS.md  CLAUDE.md  context
$ docker images --format '{{.Repository}}:{{.Tag}}' | grep -c basalt
0
```

### Why a source-built two-stage image, keeping the source-tree layout

`basalt serve` is a C++ launcher that `chdir`s into `BASALT_SOURCE_DIR` — the CMake source dir
**compiled into the binary** — and execs `python -m serve.server --engine-bin <itself>` there, with
the interpreter defaulting to `<source>/.venv/bin/python`, else `python3`
(`src/program/cli/serve.cpp:47-58`, `CMakeLists.txt:533` at `dc047a77`). The default expert profile is
`<source>/data/profiles/<model>.prof` (`docs/configuration/index.md:153`). So the runtime stage must
hold `bin/`, `serve/` and `data/` at **the same absolute path the build used**: build in
`/opt/basalt`, copy those three directories to `/opt/basalt`, and create `/opt/basalt/.venv` there.

*Rejected:* a single devel-stage image (≈ CUDA devel size, a compiler at runtime, nothing gained);
`cmake --install` to `/usr/local` (Basalt has no install target for this layout, and the compiled-in
source dir would point at a directory that is gone); running Basalt on the host without a container
(the north star and every sibling use a container; the host has no CUDA 13 toolchain to rely on).

### Why command-line flags, not a generated `basalt_config.json`

Every setting can be given on the command line (`docs/configuration/index.md:25-43`); `--api-key`
**must** be (`:60`). A generated config file would be a second place the defaults live and a file to
mount. `./run` passes flags, prints them, and Basalt's own start line `[server] engine …` lists what
it received — that line is the record. *Rejected:* a checked-in config file (a second copy of the
defaults that drifts from `./run`'s knobs).

### Why `-p 0.0.0.0:8700:8700` with `--host 0.0.0.0` inside, not `--network host`

Same exposure as `../vllmcustom` (`_run-lib.sh:262` publishes `0.0.0.0:${PORT}`), and the container
keeps its own network namespace. Basalt's own address must be `0.0.0.0` inside the container or
nothing outside reaches it (default `127.0.0.1`, `index.md:96`). Consequences, both handled:
the Host-name check (Key Facts) needs `--allowed-host cosmo.lan`; requests arrive from the bridge
gateway, so Basalt treats them as *remote* for image sources — `data:` URLs work, path/URL images
need `--image-url-host`/`--image-dir` (accepted for now; a Follow-up if a client needs them).

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| The pack | `models/vllm/jesdga/Qwen3.8-FN-NVFP4-DQ6_K.basalt`, 131,681,979,197 bytes (122.6 GiB), owner-readable | `stat -c '%s %U'` on 2026-11-04 |
| Pack magic | first 8 bytes `BASALTMF`, then byte `0x02` (format version 2) | `head -c 16 … \| od -c` on 2026-11-04; `.basalt` v2 since 0.2.0 (`docs/changelog/index.md`) |
| `models` path | `/mnt/noir/scratch/ai/llm/models` on NFS 4.2 over RDMA (`noir40.lan`) | `mount` on 2026-11-04; siblings symlink `models` there (`../ninfer/models`) |
| Basalt pin | `dc047a77beeddf1e91234d983a91d6bf929e9619` (main, 2026-11-05T00:48Z); 3 commits past `v0.2.1` = `1b3f6964` — `serve/` and docs only, engine identical | GitHub API `compare/v0.2.1...main` on 2026-11-04 |
| Engine flags are checked at start | the server asks `bin/basalt` which flags it accepts and stops on an unknown name; no shims for renamed flags | `docs/configuration/index.md:38-43`; Basalt `AGENTS.md` "No shims" |
| Build | CMake ≥ 3.24, GCC ≥ 13, CUDA ≥ 13.0, `CMAKE_CUDA_ARCHITECTURES` defaults to `120` and must be 12.x (`120a` accepted); `RUNTIME_OUTPUT_DIRECTORY` = `<source>/bin`; links `CUDA::cudart`, `CUDA::cublas`; fetches nothing | `CMakeLists.txt:2-50,528` at `dc047a77`; `docs/building.md` |
| CPU ISA | Ryzen 9 5950X, 16C/32T: AVX2, **no AVX-512, no AVX-VNNI**; Basalt picks per-file ISA objects at run time, so no `-march` is needed | `/proc/cpuinfo`, `lscpu` on 2026-11-04; `CMakeLists.txt:94-117` |
| CUDA base images | `nvidia/cuda:13.4.3-{devel,runtime}-ubuntu24.04` by digest — the pins `../ninfer/versions.env` resolved 2026-10-27; its image `iphands/ninfer-blackwell:cu1343-sm120a-64d10fb` built from them on this host | `docker images` on 2026-11-04; re-resolve in T1 |
| Ubuntu 24.04 toolchain | cmake 3.28.3, g++ 13.2 (≥ 13 ✓), Python 3.12 | `../ninfer/Dockerfile` comment (2026-10-27); Python unconfirmed — confirm in T2 |
| `basalt serve` layout | chdir `BASALT_SOURCE_DIR`, `python -m serve.server --engine-bin <exe> …`; `--python` overrides the interpreter | `src/program/cli/serve.cpp:47-58`; `tools/test_serve_launcher.py` |
| Server defaults that must change | `host` 127.0.0.1, `port` 8080, `max-context` 4096, `kv` fp16, `parallel` 1, `vision` off | `docs/configuration/index.md:96-97,129-130,203,227` |
| Slots | `--parallel N` (1..8) shares `--max-context`; `--slot-full-context` gives each slot all of it at N× the KV; above 32768 the KV is elastic (backs 4096 cells/slot at start, grows from the expert cache) | `docs/running.md:13-44`; `index.md:137-140,203-204` |
| Expected KV size | upstream's example start block: `kv 2.6/16.3GiB cuda0 elastic slots=4` with `slots par=4 ctx/slot=262144 (slot-full-context)` at `ctx=262144 kv=int8` | `docs/running.md:76-86` (another card; ours confirmed at T6) |
| Host-name check | only `localhost`, the bind address, `host.docker.internal`, the container's own LAN IPs and `--allowed-host` names pass; any IP passes. Clients use `cosmo.lan:8700` → needs `--allowed-host cosmo.lan` | `serve/security.py:117-143` at `dc047a77`; `llama-proxy/config.yaml:30`, `opencode.jsonc:46` |
| Model name | every request must name the served model (`--alias`, one name) or get a 404; clients send `cosmo-6000` | `docs/running.md:30`; `serve/config.py:374`; `llama-proxy/config.yaml:31`, `opencode.jsonc:43` |
| No api key on 0.0.0.0 | server starts, prints `[server] warning: no api key, open to the network (--api-key KEY)` | `serve/server.py:294-295` |
| SIGTERM | the server maps SIGTERM to Ctrl+C's path, so `docker stop` doesn't orphan the engine | `serve/server.py:301-302` |
| Cache dir | engine log and disk-tier conversations live in `~/.cache/basalt/<hash>/` | `docs/running.md:144-147` |
| Resident experts | experts not in VRAM go to **page-locked** RAM by default (fallback: file mapping); 0.2.1 fixed a decode stall "when the cache's copy could not be page-locked" | `index.md:149`; changelog 0.2.1 *Fixed* |
| memlock | host `ulimit -l` = unlimited; a container's limit comes from dockerd unless `--ulimit memlock=-1` (what vllmcustom passes, `_run-lib.sh:270`) | `ulimit -l` on 2026-11-04; container default unconfirmed — confirm in T6 |
| GPUs | GPU 0 RTX PRO 6000 (cc 12.0, 97,887 MiB); GPU 1 RTX 4060 (cc 8.9). Container gets `--gpus device=0` only; Basalt's `gpu` default is GPU 0 | `nvidia-smi` on 2026-11-04; `index.md:122` |
| docker | 29.8.1, buildx 0.36.1 (BuildKit: honors `<Dockerfile>.dockerignore`), default runtime `nvidia` | `docker --version`, `docker buildx version`, `docker info` on 2026-11-04 |
| Upstream `.dockerignore` | none at `dc047a77` (top-level listing) | GitHub tree API on 2026-11-04 |
| Other servers on :8700 | `vllm` (`../vllmcustom`), `../ninfer`, `../ignis` | SERIES *Standing Constraints* |

### Rejected Claims

- **"`-march=native` / AVX-512 flags would make the CPU expert path faster here."** The 5950X has
  no AVX-512 or AVX-VNNI, and Basalt already compiles each ISA rung into its own objects and picks at
  run time (`CMakeLists.txt:94-117`). Do not add host-specific `-march` flags.
- **"Build for `sm_120a` like ninfer for speed."** Basalt accepts `120a` but builds and tests `120`
  only ("One architecture only: … nothing else is tested", `CMakeLists.txt:40`). Keep the upstream
  default here; a `120a` A/B is a Follow-up, not this plan.

---

## Step-by-Step Tasks

### Wave 1 — scripts (agent). Exits when T1–T3 are committed and the gate (steps 1–4) passes.

### T1: Pins, shared guards, project skeleton

**File**: `versions.env` (new), `lib/common.sh` (new), `.gitignore` (new), `models` (new symlink),
`README.md` (new), `context/plans/RULES.md` (gate counts).

**What to do**:
- `versions.env`, plain `KEY=value` (gate 1c), comments on their own lines: `BASALT_REPO=https://github.com/jesdga95/basalt.git`,
  `BASALT_SHA=dc047a77beeddf1e91234d983a91d6bf929e9619`, `CUDA_DEVEL_IMAGE` / `CUDA_RUNTIME_IMAGE` —
  the same tag@digest as `../ninfer/versions.env` **after** re-resolving both with
  `docker buildx imagetools inspect` today, `IMAGE=iphands/basalt-blackwell`.
- `lib/common.sh`: written fresh from `../ninfer/lib/common.sh` (read, never sourced) — `die`,
  the `DOCKER` guards (array / empty / names sudo), the root refusal, the 15 s socket probe,
  `load_pins` (requires the five keys above), `owned_dir`, `nearest_existing`, `ensure_dir`
  (DRY_RUN-aware), `refuse_busy <port> <names…>`, `image_for_pin` (label `ai.basalt.ref`).
  `RUN_UID=RUN_GID=1000` constants. Every function ends `return 0`.
- `.gitignore`: `/vendor/`, `/storage/`, `/captures/`.
- `models` → `/mnt/noir/scratch/ai/llm/models` (`ln -s`, tracked, as in `../ninfer`).
- `README.md`: what this is, prerequisites, the three commands (`./build`, `./run`, `./run --single`),
  the knob table (filled by T3), "not yet built/booted" status.
- RULES gate step 1: expected counts `1 b`.

**Verify**:
```bash
docker buildx imagetools inspect "$(. ./versions.env; echo "$CUDA_DEVEL_IMAGE")"   >/dev/null && echo devel ok
docker buildx imagetools inspect "$(. ./versions.env; echo "$CUDA_RUNTIME_IMAGE")" >/dev/null && echo runtime ok
bash -c 'source lib/common.sh && load_pins && echo "pins ok $BASALT_SHA"'          # → pins ok dc047a77…
DOCKER='sudo docker' bash -c 'source lib/common.sh' ; echo "rc=$?"                  # → "## DOCKER names 'sudo'…" rc=1
readlink models                                                                     # → /mnt/noir/scratch/ai/llm/models
# + project gate steps 1–2 (RULES.md § Project gate) — counts `1 b`, `env ok`, shellcheck ok
```
Expected: both digests resolve; pins load; the sudo guard refuses; gate green with `1 b` and `env ok`.

**Commit**: `[basalt][P1][T1][pins] add Basalt dc047a77 + CUDA 13.4.3 pins, shared guards, README` —
*commit before marking done (Rule B), tracker row in the same commit.*

### T2: Dockerfile + `./build`

**File**: `Dockerfile` (new), `Dockerfile.dockerignore` (new), `build` (new), `context/plans/RULES.md`
(gate step 3 + counts `2 b`).

**What to do**:
- `Dockerfile`, two stages, `ARG`s with empty defaults (a bare `docker build` fails rather than
  building unpinned — `../ninfer/Dockerfile` header):
  - **build** `FROM ${CUDA_DEVEL_IMAGE}`: apt `cmake g++ ninja-build`; `COPY . /opt/basalt`;
    `cmake -S /opt/basalt -B /opt/basalt/build -G Ninja -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=OFF`
    (no `CMAKE_CUDA_ARCHITECTURES`: upstream's `120`); `cmake --build /opt/basalt/build --parallel ${JOBS}`
    (all targets: `basalt`, `basalt-bench`, `basalt-probe`, `basalt-vision`, helpers).
  - **runtime** `FROM ${CUDA_RUNTIME_IMAGE}`: apt `python3 python3-venv ca-certificates`;
    `COPY --from=build /opt/basalt/{bin,serve,data}` and `requirements.txt`, `LICENSE`,
    `THIRD_PARTY_LICENSES` to `/opt/basalt/`; `python3 -m venv /opt/basalt/.venv &&
    /opt/basalt/.venv/bin/pip install --no-cache-dir -r /opt/basalt/requirements.txt` (upstream's pins);
    `LABEL ai.basalt.ref/repo/cuda/base`; `ENV PYTHONDONTWRITEBYTECODE=1`; `EXPOSE 8700`;
    `STOPSIGNAL SIGTERM`; `USER 1000:1000` last; `ENTRYPOINT ["/opt/basalt/bin/basalt"]`, `CMD ["--help"]`.
  - Confirm the runtime image's Python version (Key Facts: unconfirmed) inside a throwaway
    `docker run --rm --network none <runtime digest> python3 --version` **only if** python3 is in
    the base; else from the apt candidate (`apt-cache policy` in a throwaway container) — record it.
- `Dockerfile.dockerignore` (BuildKit reads it beside the Dockerfile): `.git`, `build*/`, `bin/`,
  `.venv/`, `models/`, `pack/`, `packs/` — mirrors upstream's `.gitignore`.
- `build`, modeled on `../ninfer/build`: manage `vendor/basalt` (clone if absent, **refuse if dirty**,
  `git checkout --detach $BASALT_SHA`; never `--force`, `reset` or `clean`); resolve both base
  digests; `docker build --check` (lint, no build); tag `$IMAGE:cu<digits>-sm120-<sha7>`; build with
  `-f "$HERE/Dockerfile"` and context `vendor/basalt`, all pins as `--build-arg`, log to
  `captures/build_<date>_<sha7>.log`. `PREFLIGHT_ONLY=1` stops after the lint and prints the build
  command. `JOBS` default: half the cores.
- RULES gate: add **step 3 — build preflight** (`PREFLIGHT_ONLY=1 ./build`, pass = exit 0 and last
  line `preflight ok`), counts `2 b`.

**Verify**:
```bash
PREFLIGHT_ONLY=1 ./build                       # → checkout at dc047a77, digests ok, docker build --check clean, "preflight ok", exit 0
git -C vendor/basalt rev-parse HEAD            # → dc047a77beeddf1e91234d983a91d6bf929e9619
touch vendor/basalt/.dirty-probe && PREFLIGHT_ONLY=1 ./build; echo "rc=$?"   # → refuses: dirty checkout (untracked counts), rc=1
rm vendor/basalt/.dirty-probe                  # remove only the file this test created (RULES Rule B2: never clean/restore)
docker images | grep -c basalt-blackwell       # → 0 (preflight builds nothing)
# + project gate steps 1–3
```
Expected: preflight passes on a clean checkout, refuses a dirty one, and builds no image.

**Commit**: `[basalt][P1][T2][docker] add two-stage sm_120 Dockerfile and ./build with preflight`

### T3: `./run` + its dry-run gate step

**File**: `run` (new), `README.md` (knob table), `context/plans/RULES.md` (gate step 4 + counts `3 b`).

**What to do**: `./run [--single] [PACK]`, `set -euo pipefail`, sources `lib/common.sh`, `load_pins`.
- Mode: default `PARALLEL=4` with `--slot-full-context`; `--single` → `PARALLEL=1`, no
  `--slot-full-context`. `PARALLEL=N` env overrides (1..8; `--slot-full-context` only when N > 1).
- Knobs (env) and defaults: `PACK`/`$1` = `$HERE/models/vllm/jesdga/Qwen3.8-FN-NVFP4-DQ6_K.basalt`;
  `PORT=8700`; `ALIAS=cosmo-6000`; `ALLOWED_HOSTS="cosmo.lan $(hostname)"`; `MAX_CONTEXT=262144`;
  `KV=int8`; `VISION=dynamic`; `API_KEY` empty; `NAME=basalt`; `CACHE_DIR=$HERE/storage/cache`;
  `EXTRA_ARGS` (appended last, last wins); `DRY_RUN=1`.
- Pack checks **before any docker call that starts something**: exists and readable; a `*.ninfer`
  path dies naming `../ninfer` (pitfalls.md); else the first 8 bytes must be `BASALTMF` (any name).
  Print the pack path and size.
- `refuse_busy "$PORT" vllm ninfer ignis basalt`; image = `image_for_pin` (none → die
  "run ./build first"); `ensure_dir "$CACHE_DIR"`.
- `docker run --rm --name "$NAME" --user 1000:1000 -e HOME=/home/ubuntu --gpus device=0
  -p "0.0.0.0:$PORT:$PORT" --ulimit memlock=-1 -v "<pack dir>:/models:ro"
  -v "$CACHE_DIR:/home/ubuntu/.cache:rw" "$IMAGE" serve /models/<pack file>
  --host 0.0.0.0 --port "$PORT" --alias "$ALIAS" --allowed-host … --gpu 0 --max-context …
  --kv … --parallel … [--slot-full-context] --vision … [--api-key …] $EXTRA_ARGS`.
- Print the image tag, the pack and the full command before starting (`DRY_RUN=1`: print and exit 0,
  create nothing). The api key is printed as `***`.
- RULES gate: add **step 4 — `./run` dry run**: `DRY_RUN=1 ./run` and `DRY_RUN=1 ./run --single`
  pass mechanical greps (default: `--parallel 4` + `--slot-full-context` + `--max-context 262144`;
  single: `--parallel 1`, no `--slot-full-context`; both: `-p 0.0.0.0:8700:8700`, `--gpus device=0`,
  `--ulimit memlock=-1`, `--allowed-host cosmo.lan`, `--alias cosmo-6000`, `--kv int8`,
  `--vision dynamic`); `DRY_RUN=1 ./run /tmp/x.ninfer` exits 1 naming `../ninfer`; a missing pack
  exits 1. Until the image exists, the dry run stubs the image lookup with `IMAGE_OVERRIDE` and the
  step says so. Counts `3 b`.

**Verify**:
```bash
DRY_RUN=1 IMAGE_OVERRIDE=x:y ./run            | tail -3   # → the full docker command, 4 full-context slots
DRY_RUN=1 IMAGE_OVERRIDE=x:y ./run --single   | tail -3   # → --parallel 1, no --slot-full-context
DRY_RUN=1 IMAGE_OVERRIDE=x:y ./run "$PWD/models/vllm/jesdga/Qwen3.8-27B-QUASAR-DFlash2-nvfp4-NInfer/qwen3_8_27b_quasar.ninfer"; echo "rc=$?"  # → names ../ninfer, rc=1
git status --short --ignored storage/          # → nothing created by the dry runs
# + project gate steps 1–4
```
Expected: the printed commands carry every default above; the `.ninfer` file and a missing pack are
refused before docker runs anything; the dry runs create nothing.

**Commit**: `[basalt][P1][T3][run] add ./run (4 full-context slots, --single) for Flash-Next on :8700`

### Wave 2 — the owner builds and boots; the agent verifies and records. Exits when T7 is committed.

### T4: Owner build; verify the image is the pin

**File**: `context/plans/01_flashnext_container_tracker.md` (Evidence), `context/distilled.md`,
`README.md` (built image line).

**What to do**: the owner runs `./build` (Standing Constraints). The agent, without a GPU or a
model: reads the build log tail; checks the image's labels; runs the binary and the server without
starting anything.

**Verify**:
```bash
IMG=$(bash -c 'source lib/common.sh; load_pins; image_for_pin')
docker image inspect "$IMG" --format '{{index .Config.Labels "ai.basalt.ref"}} {{.Size}}'      # → dc047a77… <bytes>
docker run --rm --network none --entrypoint /opt/basalt/bin/basalt "$IMG" --help | head -5        # → Basalt's usage (no GPU needed? — record)
docker run --rm --network none --entrypoint /opt/basalt/.venv/bin/python -w /opt/basalt "$IMG" -m serve.server --help | head -3
docker run --rm --network none --entrypoint ls "$IMG" /opt/basalt /opt/basalt/data/profiles       # → bin data serve .venv …; qwen4exp-48x512.prof
# + project gate steps 1–4
```
**Expected observation**:
- **Confirms if**: the label is the pin, `bin/basalt --help` and `serve.server --help` print usage,
  and the profile file is in place.
- **Refutes if**: `--help` needs a GPU (record it; T5's flag check then runs with `--gpus device=0`),
  or the server import fails (missing Python dependency — fix the Dockerfile; the owner rebuilds).
- **Within noise if**: N/A — not a measurement.

**Commit**: `[basalt][P1][T4][docker] record the owner's build of <tag> (labels, layout, --help)`

### T5: Check `./run`'s flags against the built image

**File**: `context/plans/RULES.md` (gate step 5), `run` (only if a flag is wrong).

**What to do**: add **gate step 5**: take every `--flag` `DRY_RUN=1 ./run` emits after `serve
<pack>` and every one from `./run --single`; each must appear in the image's `bin/basalt --help`
or the server's `serve.server --help` (engine flags vs server options, `index.md:34-43`). Pass:
`flags ok` and zero missing. Fix any flag Basalt does not know.

**Verify**:
```bash
# gate step 5 as written in RULES.md → "flags ok (N engine, M server)"
EXTRA_ARGS=--no-such-flag DRY_RUN=1 ./run | …   # the same check → "FAIL(flags) --no-such-flag" (the negative control is live)
# + project gate steps 1–5
```
Expected: every emitted flag is known; an injected bogus flag is caught.

**Commit**: `[basalt][P1][T5][gate] check ./run's flags against the built image's --help`

### T6: Owner boots — default and `--single` — with a real reply each

**File**: `context/plans/01_flashnext_container_tracker.md` (Evidence), `context/distilled.md`.

**What to do**: with `:8700` free, the owner runs `./run` and pastes the start block (from
`[server] legend` through `[server] weights loaded`, plus any `warning:` and the `RAM floor`/`RAM …
free, guard` lines) and the load time. The agent (or owner) sends one request **through the client
name** and one through an IP:
```bash
curl -s http://cosmo.lan:8700/v1/chat/completions -H 'Content-Type: application/json' \
  -d '{"model":"cosmo-6000","messages":[{"role":"user","content":"Say hi in five words."}],"max_tokens":256}'
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8700/v1/models
curl -s -o /dev/null -w '%{http_code}\n' http://cosmo.lan:8700/v1/chat/completions \
  -H 'Content-Type: application/json' -d '{"model":"nope","messages":[{"role":"user","content":"x"}]}'
```
Then `docker stop basalt` (record that the engine exits with it) and repeat with `./run --single`.

**Verify**: the start block names `ctx=262144 kv=int8`, `slots par=4 ctx/slot=262144
(slot-full-context)` (default) / no slots line or `par=1` (single — record which), `vision
mode=dynamic`, the `loaded` split (`cuda0=…GiB ram=…GiB`), the `kv` line; the first curl returns a
`choices[0].message` from `cosmo-6000`; `/v1/models` = 200; the wrong model = 404; `docker stop`
returns within its 10 s grace.

**Expected observation**:
- **Confirms if**: both modes load with experts split between `cuda0` and page-locked RAM, no
  "could not be page-locked" / mmap fallback warning, and both replies arrive.
- **Refutes if**: a `warning: the N batch slots' sessions … take X GiB` names the 4 full-context
  slots as starving the expert cache (record X; the default stays the owner's call — a Follow-up),
  the Host check rejects `cosmo.lan` (fix `ALLOWED_HOSTS`), or the RAM tier falls back to a file
  mapping (memlock or RAM floor — record the line).
- **Within noise if**: N/A — the load time is recorded with its run count (1 per mode), not compared.

**Commit**: `[basalt][P1][T6][boot] record the owner boots of the DQ6_K pack (4×262144 and --single)`

### T7: Harvest

**File**: `context/distilled.md`, `context/pitfalls.md`, `context/high_level.md`, `README.md`.

**What to do**: move every confirmed T4–T6 fact into `distilled.md` with `[LIVE <date>]` (VRAM/RAM
split, load time over NFS, KV size per mode, memlock outcome, Host-check behavior, `--help` without
GPU), close the matching `## Open Questions` there; anything that took more than one attempt →
`pitfalls.md`; the image shape and the CLI-flags-not-config choice → `high_level.md`; README status
→ built + booted, with the commands.

**Verify**:
```bash
grep -n 'LIVE 20' context/distilled.md            # → the T4–T6 entries, each with its command
grep -n -A3 '## Open Questions' context/distilled.md   # → the two init questions resolved or re-scoped
# + project gate steps 1–5
```
Expected: every finding the tracker names is in a `context/` file (Rule D).

**Commit**: `[basalt][P1][T7][context] harvest the build and boot findings`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `run` (new) | the launcher: modes, knobs, pack checks, docker command, DRY_RUN | P0 |
| `Dockerfile` (new) | two-stage build keeping `/opt/basalt` layout + venv | P0 |
| `build` (new) | pinned `vendor/basalt` checkout, preflight, build + log | P0 |
| `versions.env` (new) | Basalt + CUDA pins, image name | P0 |
| `lib/common.sh` (new) | guards and helpers shared by `build` and `run` | P0 |
| `context/plans/RULES.md` | gate steps 3–5, expected counts | P0 |
| `Dockerfile.dockerignore` (new) | keep `.git` and build output out of the context | P1 |
| `README.md` (new) | user-facing commands, knobs, status | P1 |
| `.gitignore` (new), `models` (new symlink) | ignore vendor/storage/captures; model path | P1 |
| `context/distilled.md`, `pitfalls.md`, `high_level.md` | T4–T7 findings | P1 |

---

## Open Questions / Risks

1. **Risk: four full-context slots squeeze the expert cache.** Measured (T6 boot 3): all 24,576 experts
   fit in VRAM at start; at full growth the 4×262144 KV takes the cache's last 5,334 slots (13.73 GiB),
   so long concurrent conversations push up to 22% of the experts to the CPU. Original text: At `int8`, upstream's example shows
   `2.6/16.3GiB` for 4×262144 elastic — backed lazily, but every long conversation takes VRAM from
   experts, which then run from RAM over PCIe on a DDR4 host. *Mitigation*: elastic KV backs only
   what requests use; T6 records the `kv` line and any starvation warning; `--single` is one word
   away. The default stays the owner's decision (2026-11-04).
2. ~~**Risk: the resident RAM copy cannot page-lock inside the container.** *Mitigation*:
   `--ulimit memlock=-1` (as vllmcustom); T6 looks for the fallback/warning lines.~~
   RESOLVED (T6 boot 3): `ulimit -l` is `unlimited` inside the container; no fallback warning.
3. **Risk: host RAM.** 122.6 GiB pack; what VRAM does not hold goes to page-locked RAM on a 125 GiB
   host, plus checkpoints (`--prompt-cache` 6 × ~118 MB per slot + 1), vision's 0.85 GiB pinned,
   and the OS. *Mitigation*: Basalt sizes `resident` from free RAM above its floor and falls back to
   a file mapping (`index.md:76,149`); `refuse_busy` keeps vLLM (which pins host RAM too) down.
   T6 records the `auto … resident=` and `RAM floor` lines.
4. ~~**Question: does `bin/basalt --help` run without a GPU?** *How we'll settle it*: T4 runs it with
   `--network none` and no `--gpus`; if not, T5's gate step passes `--gpus device=0` and says so.~~
   RESOLVED (T4): yes, rc=0 with no `--gpus` (tracker Evidence).
5. **Risk: Host-name check.** Clients use `cosmo.lan:8700`; anything else (e.g. a FQDN) gets
   refused. *Mitigation*: `ALLOWED_HOSTS` knob; T6 tests the client name and an IP.
   Partly settled (T4, mock engine): `cosmo.lan`/`cosmo`/`localhost` → 200, an unlisted name → 403.
   T6 still checks it on the real server through `cosmo.lan`.
6. **Risk: one alias.** vLLM also served `cosmo-proxy`, `claude-heavy`, `claude-light`,
   `claude-haiku-4-5-20251001` (`../vllmcustom/common/_run-lib.sh:420`); Basalt answers one name, so
   requests naming those get 404. *Mitigation*: `ALIAS` knob; deferred — a client that needs another
   name is a Follow-up (llama-proxy rewrite or a later plan), not this plan.
7. **Risk: image sources over the bridge.** Requests from the host count as remote: `data:` URLs
   work, URL/path images are refused without `--image-url-host`/`--image-dir`. *Mitigation*:
   `EXTRA_ARGS`; deferred — opencode sends `data:` URLs; revisit if a client needs URLs.
8. **Risk: no api key on `0.0.0.0`.** Same LAN exposure as vLLM today; Basalt warns at start.
   *Mitigation*: `API_KEY` knob, printed as `***`. Owner's call.
9. **Risk: NFS load time.** 131.7 GB over NFS/RDMA at every boot (vLLM's FP4-Local: 57 s weight load
   on 40 GbE, `../vllmcustom/context/distilled.md`). *Mitigation*: none needed for correctness;
   T6 records it; a local-disk copy is a Follow-up if it hurts.
   **It hurts (T6, 2026-11-04):** boot 2 loads at ~5–10 MB/s — Basalt reads weights with `O_DIRECT`
   (no switch), and buffered NFS reads measured 28.9 MB/s during the boot. Owner let it run; see the
   tracker Notes.
10. **Question: `pip install` at image build needs network.** *How we'll settle it*: T2 keeps
    upstream's pinned `requirements.txt`; the owner's build has network (ninfer's did).

---

## Verification Checklist

- [x] T1: both CUDA digests resolve; `load_pins` prints `pins ok dc047a77…`; `DOCKER='sudo docker'` → rc=1; gate shows `1 b`, `env ok`
- [x] T2: `PREFLIGHT_ONLY=1 ./build` → `preflight ok`, `vendor/basalt` at `dc047a77…`, a dirty checkout → rc=1, no image built
- [x] T3: `DRY_RUN=1 ./run` / `--single` print commands carrying every default; a `.ninfer` path and a missing pack → rc=1 before docker starts anything; nothing created
- [x] T4: the built image's `ai.basalt.ref` = `dc047a77…`; `bin/basalt --help` and `serve.server --help` print usage; `data/profiles/qwen4exp-48x512.prof` present
- [x] T5: gate step 5 → `flags ok`; an injected bogus flag → `FAIL(flags)`
- [ ] T6: both modes boot; start blocks recorded; `cosmo.lan` chat → reply from `cosmo-6000`; wrong model → 404; `docker stop` clean
- [ ] All: the project gate (Rule A) passes on the final commit
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
