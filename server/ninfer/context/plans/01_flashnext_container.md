# Plan 01 — NInfer Flash-Next: pinned sm_120a image, containerized conversion, `./run`, measured

> **Status**: in-progress
> **Created**: 2026-10-27
> **Depends on**: N/A
> **Goal**: Serve Qwen3.8-Flash-Next through NInfer from our own pinned sm_120a/CUDA 13.4 image, with the `.ninfer` artifact converted in a container, launched by `./run` on `:8700` — and measure it against the vLLM FP4-Local daily driver to pick the fastest defaults.
> **Agent**: implementation agent (Wave 1, T8's script, T12) · owner runs (T6–T11: build, download, conversion, boots, benches)

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Build the whole NInfer path for Flash-Next on this card — pins, a Dockerfile that
actually builds at the pinned commit, a seccomp profile that lets NInfer's io_uring reader run,
a containerized converter, the `./run` launcher — then boot it, prove the experts sit on the GPU,
and A/B it against vLLM FP4-Local on the same bench to settle the defaults.

**Deliverables**:
1. `versions.env`, `lib/common.sh`, `README.md`, `models` symlink; `context/` seeded with the research findings (T1).
2. `seccomp/ninfer.json` + `scripts/seccomp-profile`: docker's default profile plus exactly the three io_uring syscalls (T2).
3. `Dockerfile` + `build`: NInfer `64d10fbf` for `sm_120a` on CUDA 13.4.3, `USER 1000:1000`, labelled (T3).
4. `scripts/convert`: `tools.convert --recipe qwen3_8_flash_next` in a container, BF16 base + NVIDIA NVFP4 → `.ninfer` (T4).
5. `run`: the launcher on `:8700`, `--model-id cosmo-6000`, MTP, FP8 KV, auto KV/expert budget (T5).
6. A built image, a converted artifact, a booted server that passes `scripts/smoke` (T6–T8, owner-run).
7. Evidence rows: NInfer vs FP4-Local same-session A/B, an MTP depth sweep, NFS vs NVMe for the PLE path (T9–T11), and the measured-best defaults locked into `run` + `README.md` (T12).

**Estimated effort**: Large (3 days) — Wave 1 is ~1 day of agent work. Waves 2–3 are mostly
owner wall time: a 360 GB download, a conversion of unknown duration (it rewrites ~100 GiB), an
image build, and ~30 bench passes.

---

## Scope

**In**: everything under `server/ninfer/`. **Reads, never writes**: `../vllmcustom` (its bench
harness is *run* with `--out` pointed here; its image is *used* as the converter runtime; its
`run-fp4local` is *launched* by the owner as the baseline arm). **Out**: any change to
`../vllmcustom`, to NInfer's source (`vendor/ninfer` is read-only), the parent `llama-proxy`, and
host state (`nvidia-smi -pl/-lgc`). If a fix needs to cross this line, **stop** and record the
blocker in the tracker instead of expanding scope.

---

## Context

The owner wants the best and fastest NInfer setup for this card, in a container, after a Reddit
benchmark post about Qwen3.8-27B on an RTX 5090 (`r/Qwen_AI` post `1xgwo7o`; Reddit refuses our
fetcher, and no mirror was found — the numbers below come from the NInfer repo, the author-adjacent
gists and upstream issues instead). NInfer is a from-scratch C++/CUDA engine that targets
**`sm_120a` only** — the same GB202 silicon as this RTX PRO 6000 — and claims native support for
Qwen4Exp, which is Flash-Next, our daily-driver family (`../vllmcustom`, `run-fp4local`).

What makes this card different from the 5090 NInfer was built on: Flash-Next's main experts are
**always** host-backed (~63.3 GiB registered RAM) with a GPU static cache sized from what VRAM is
left, and missing experts run **on the CPU** for small batches. A 32 GB 5090 caches a fraction; on
96 GB, the auto budget should cache all of them (Key Facts). That — plus PLE rows read from disk
with `O_DIRECT` on every decode step — is where "fastest" will be won or lost here, and neither is
visible without a boot.

### Pre-Identified Bug/Issue

Today there is nothing: no project, no image, no artifact. Three findings shape the work.

**1. Docker's default seccomp profile blocks io_uring, and NInfer has no fallback**
(`src/core/io/read_queue.cpp:83` calls `io_uring_queue_init` and fails on error):

```bash
$ docker run --rm --network none --entrypoint python3 \
    iphands/vllm-blackwell:cu1321-sm120-main-vllm0.34.0.dev20261027-g40fe396 -I -c '
import ctypes, os
libc = ctypes.CDLL(None, use_errno=True)
p = ctypes.create_string_buffer(120)
fd = libc.syscall(425, 8, p)
print("io_uring_setup default-seccomp:", fd, os.strerror(ctypes.get_errno()) if fd < 0 else "ok")'
io_uring_setup default-seccomp: -1 Operation not permitted
$ # same command with --security-opt seccomp=unconfined
io_uring_setup seccomp=unconfined: 3 ok
```

**2. Neither checkpoint we have can be the converter's base.** The `qwen3_8_flash_next` recipe
takes `--model <BF16 base>` + `--source nvidia=<NVIDIA NVFP4>`, and its model builder rejects any
base tensor that is not BF16 (`tools/convert/qwen4_exp.py`, `Builder.add`, `bank_source`, the PLE
shard check). A header probe of our NVFP4 checkpoint (`python3 -I hdr.py models/vllm/nvidia/Qwen3.8-Flash-Next-NVFP4`,
2026-10-27):

```text
('expert', 'F32') 147456
('expert', 'F8_E4M3') 73728
('expert', 'U8') 73728
('mtp', 'BF16') 1565
('mtp', 'F8_E4M3') 1536
('other', 'BF16') 1067
('other', 'I64') 3
('ple', 'BF16') 1
('ple', 'F8_E4M3') 128
('visual', 'BF16') 333
PLE model.language_model.layers.1.ple.ple_embedding.ngram_embedding.shard_0.weight ('F8_E4M3', [2500012, 160], 'model-fp8-mtp-ple.safetensors')
```

It is exactly what the recipe wants as the **`nvidia`** source (per-expert modelopt NVFP4, block-FP8
MTP experts), and not a base: per-expert `U8` experts, an FP8 PLE table. `FP4-Local` is neither:
its expert scales are CSF-compressed (`weight_scale.nvfp4_csf_fixed`/`_exceptions`), its dense
layers MXFP8, its PLE packed NVFP4. **The owner downloads the BF16 base** (decision 2026-10-27).

**3. Upstream's own `Dockerfile` cannot build the pinned commit.** `cmake/Dependencies.cmake:5`
is `pkg_check_modules(URING REQUIRED IMPORTED_TARGET liburing>=2.5)` (added in `8cea711b`,
2026-10-13), but the `Dockerfile` was last changed in `74671a4e` (2026-10-12) and installs neither
`liburing-dev` nor `liburing2`. It also uses CUDA 13.1.2 where the maintained toolkit is 13.4,
runs as root, and carries no labels. *Inferred from the source, not from a failed build* — T6's
build is the first real check.

### Why our own Dockerfile, a BF16 download, the vLLM image as converter, and a seccomp profile

- **Our Dockerfile, not upstream's**: upstream's misses `liburing` (above), uses CUDA 13.1.2, runs
  as root and has no labels to find the image by. Ours pins NInfer by SHA and CUDA by tag *and*
  digest, adds `liburing`, `USER 1000:1000` last, `ai.ninfer.ref`.
- **Download the BF16 base, not shim the NVFP4 checkpoint into one**: a shim would monkeypatch the
  converter's builder (dtype checks, fused-bank names, the coverage check) against one pinned
  commit, and NF4-quantize the PLE table from FP8 instead of BF16. Rejected by the owner
  2026-10-27 in favor of the documented path.
- **Convert inside `../vllmcustom`'s image, not a new torch image**: it already has Python 3.12,
  torch 2.14 (CUDA 13.2, sm_120), NumPy, `nvcc`, `ninja` and `g++` (probe below) — everything the
  converter's JIT encoder needs. A dedicated converter image means several GB of torch wheels for
  an untested CUDA 13.4 combination. Risk: NInfer maintains Python 3.11 — T4 checks import-level
  compatibility, T7 is the real run.
- **A seccomp profile, not `seccomp=unconfined`**: unconfined drops all syscall filtering on a
  server bound to `0.0.0.0`; the profile is docker's default plus three syscalls.
- **`lib/common.sh` written fresh, `../vllmcustom/common/_run-lib.sh` as reference only**: its
  `docker_core_args` is vLLM-shaped (`--entrypoint vllm`, container port 8799, vLLM env), and
  sourcing across projects would couple two plan systems. The guards (DOCKER-is-a-string, refuse
  `sudo` on both arms, refuse root, probe the socket) are re-implemented, not copied blind.
- **Measure with `../vllmcustom/bench/bench --out captures/…`**: the same prompts and client as the
  FP4-Local baseline, so the numbers compare; `--out` keeps the writes in this project.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| NInfer pin | `64d10fbf9f8e6bfc79ab0570b94674c5b9997929` (2026-10-16, HEAD of `master`) | `git log -1 origin/master` in a scratch clone, 2026-10-27 |
| Arch gate | CMake refuses anything but `CMAKE_CUDA_ARCHITECTURES=120a` | `CMakeLists.txt:3-12` @ `64d10fbf` [SOURCE] |
| Toolkit | CUDA 13.4 is the maintained toolkit; upstream Dockerfile uses `nvidia/cuda:13.1.2-*` | `docs/maintainer/build-system.md`, `Dockerfile:3` [SOURCE] |
| Base images | `nvidia/cuda:13.4.3-devel-ubuntu24.04` and `-runtime-` exist | Docker Hub tags API, 2026-10-27; digests: unconfirmed — resolve in T1 |
| Build deps | cmake ≥ 3.28, C++20, Ninja, pkg-config, FFmpeg dev, hwloc ≥ 2.5, liburing ≥ 2.5, libcurl ≥ 7.85 | `cmake/Dependencies.cmake:4-23`, `docs/maintainer/build-system.md` [SOURCE] |
| Ubuntu 24.04 versions | cmake 3.28.3, liburing 2.5, hwloc 2.10 | unconfirmed — confirm in T3 (`apt-cache policy` in the devel image) |
| io_uring | NInfer opens model files `O_DIRECT` (`src/core/io/file.cpp:37`) and requires io_uring (`src/core/io/read_queue.cpp:83`) | [SOURCE] |
| Docker seccomp | docker 29.8.0 default profile: `io_uring_setup` → `EPERM`; host `kernel.io_uring_disabled` = `0` | probe above + `cat /proc/sys/kernel/io_uring_disabled`, 2026-10-27 |
| memlock | io_uring registered buffers count against `RLIMIT_MEMLOCK`; the launcher passes `--ulimit memlock=-1` | `io_uring_register(2)` [DOC]; whether NInfer registers buffers: [SOURCE] `docs/maintainer/qwen4_exp-execution.md` §4.1 ("registered files and buffers") |
| Converter recipe | `--model <BF16 base> --recipe qwen3_8_flash_next --source nvidia=<NVFP4> --components text,vision,mtp --rows-per-chunk 16384`, `OMP_NUM_THREADS=4 MKL_NUM_THREADS=4` | `docs/weight-conversion.md` [DOC]; `tools/convert/official_recipes.py:188-382` [SOURCE] |
| Converter strictness | base tensors must be BF16; base expert banks fused (`mlp.experts.gate_up_proj`); every base key must be consumed | `tools/convert/qwen4_exp.py` `Builder.add`, `bank_source`, `build_model` coverage check [SOURCE] |
| BF16 base | `Qwen/Qwen3.8-Flash-Next` rev `de4b8e4d43b917e7706784d8bb445c9af86a3540`: 131 safetensors, 360.0 GB (335.3 GiB), not gated; 1,658 keys incl. `model.language_model.layers.0.mlp.experts.gate_up_proj` and the 128 `…ngram_embedding.shard_N.weight` | HF API + its `model.safetensors.index.json`, 2026-10-27 |
| NVFP4 provenance | experts quantized from the BF16 base; MTP + PLE byte-identical to `Qwen/Qwen3.8-Flash-Next-FP8` | `models/vllm/nvidia/Qwen3.8-Flash-Next-NVFP4/README.md:188` [DOC] |
| Artifact | ~100.67 GiB payload; entry + `.part-NNNN` files at 32,000,000,000 B; `<entry>.conversion.json` beside it; never overwrites | `docs/weight-conversion.md` [DOC] |
| Converter JIT | `torch.utils.cpp_extension.load(name="ninfer_stored_scale", …, "-gencode=arch=compute_120,code=sm_120")` — needs nvcc + C++ + ninja | `tools/convert/quantization/stored_scale.py:22-33` [SOURCE] |
| Converter runtime | `../vllmcustom` image `…g40fe396`: Python 3.12.3, torch 2.14.0 (CUDA 13.2), NumPy 2.2.6, nvcc 13.2, `/opt/venv/bin/ninja`, `g++` | `docker run … bash -c 'command -v nvcc ninja g++ …'`, 2026-10-27 |
| Python 3.12 vs 3.11 | "CUDA 13.4 and Python 3.11 describe the maintained environment, not configuration version gates" | `docs/maintainer/build-system.md` [DOC]; actual compatibility unconfirmed — confirm in T4 |
| Expert residency | Main experts: full registered Host backing always + GPU static cache; small-batch misses run on the CPU; `--kv-capacity auto` reserves one full context, the rest goes to the expert cache | `docs/cli.md` (Flash Next), `docs/maintainer/qwen4_exp-execution.md` §1, §8.2 [DOC] |
| Expert sizes | 1,415,577,600 B per main layer × 48 = 63.3 GiB; MTP experts 2,516,889,600 B, GPU-fixed | `docs/maintainer/expert-weight-cache.md` [DOC] |
| All experts fit on 96 GB? | estimate: ~10 GiB fixed weights + ~3.4 GiB FP8 KV (262,144 × 13,152 B) + workspace + 1 GiB headroom leaves > 63.3 GiB | arithmetic from the two rows above; **unconfirmed — confirm in T8** (debug planning log) |
| PLE path | disk only (no RAM/GPU table), `O_DIRECT` + io_uring, 64 MiB row cache, 128 reads in flight; first consumed at zero-based layer 1, so little compute hides it | `docs/maintainer/qwen4_exp-execution.md` §1, §4 [DOC] |
| Flash-Next spec | native MTP only, `--draft-tokens 1..5`, full shared head — no `--lm-head-draft`, no DFlash | `docs/cli.md`, `README.md` [DOC] |
| Serve defaults | `--host 127.0.0.1 --port 8080 --max-context 8192 --max-concurrency 1 --kv-dtype bf16`, `--vision` off, `--preserve-thinking` off, prefill chunk 4096 under CPU expert offload | `docs/serving.md` option table [DOC] |
| Model name | one public alias: `model` must equal `--model-id`; `llama-proxy` addresses `:8700` as `cosmo-6000` | `docs/serving.md:534` [DOC]; `../../llama-proxy/config.yaml:9-10` [REPO] |
| Sync | `NINFER_CUDA_SYNC` defaults to `spin` (lowest latency) | `docs/cli.md:347` [DOC] |
| CPU MoE ISA | runtime dispatch avx512_bf16 / avx512 / avx2; the Flash-Next shape has an AVX2 plan (`max_columns = 2`); this host is AVX2-only | `src/ops/moe/routed_swiglu/cpu/nvfp4/shapes/h2560_i640_e512_k10/dispatch.cpp` [SOURCE]; `lscpu` |
| `ignore_eos` | the chat parser has no unknown-member rejection, so the bench's `ignore_eos` is probably ignored, not rejected | `grep -n unknown src/serve/openai_chat_request.cpp` (none) [SOURCE]; unconfirmed — confirm in T9 (`completion_tokens == max_tokens`) |
| This card runs it | Flash-Next v3 on an RTX PRO 6000 with `--max-context 131072 --kv-capacity 262144 --max-concurrency 4 --prefill-chunk 8192 --kv-dtype fp8 --vision` | upstream issue #394 (Windows build `81c8ce09`) [DOC]; no tok/s published |
| Host | 5950X (AVX2), 126 GiB RAM, PRO 6000 97,887 MiB at 475 W + RTX 4060, driver 615.78.08 / CUDA 13.4 UMD, models on NFS 4.2 over RDMA, `/home` NVMe btrfs 319 GB free | `lscpu`, `/proc/meminfo`, `nvidia-smi`, `findmnt`, `df`, 2026-10-27 |

### Prior Measurements

| Source | Metric | Result | Runs | Spread |
|--------|--------|--------|------|--------|
| `../vllmcustom` Plan 06 tracker Evidence 2026-10-18, `bench/bench-logs/flashnext-matrix-20261018-184027/` (image `…gfe8f214`, humming, K=3, `SM120_SKINNY=1`) | FP4-Local single-stream decode tok/s, `code` / `long` | 195.5 / 190.7 | 4 | 188.5–210.6 / 184.4–197.4 |
| same | `long` (~16k) TTFT ms | 433 | 4 | 416–481 |
| NInfer `README.md` @ `64d10fbf` — **RTX 5090, Qwen3.8-27B nvfp4: different model, different card, context only** | C=1 MTP3 decode tok/s | 147.7 | n/a | n/a |
| Gist `PierpaoloPernici/f1d1382f…` — **RTX 5090, 27B, context only** | c1 tg tok/s at depth 0, NInfer nvfp4 vs vLLM 0.27.1 MTP3 | 127.1 vs 117.3 | n/a | n/a |

The FP4-Local row was taken on an older image; T9 re-measures it in the same session (P1.4–P1.5).

### Rejected Claims

- **"Our NVFP4 checkpoint can be the base"** — no: the builder checks BF16 per tensor and the PLE is
  FP8 (probe above). Do not "fix" the converter to accept it; that is the rejected shim.
- **"FP4-Local can be the `nvidia` source"** — no: CSF scales and NVFP4 MTP experts; the recipe's
  ModelOpt reader and block-FP8 MTP import both refuse it.
- **"NInfer only supports the RTX 5090"** — the README says so, but the build gate is the
  architecture (`sm_120a`), which this card has, and issue #394 runs Flash-Next on a PRO 6000.

---

## Step-by-Step Tasks

### Wave 1 — scripts and pins (agent; no GPU, no build, no model)

*Wave 1 exits when gate steps 1–6 pass and `README.md` documents build → convert → run.*

### T1: Pins, shared guards, README, `models`, knowledge seed

**File**: `versions.env`, `lib/common.sh`, `README.md`, `models` (symlink), `context/distilled.md`, `context/pitfalls.md`, `context/high_level.md`, `context/plans/RULES.md` (step 1 counts)

**What to do**:
- `versions.env` (plain `KEY=value`, gate 1c): `NINFER_REPO=https://github.com/Neroued/ninfer.git`,
  `NINFER_SHA=64d10fbf9f8e6bfc79ab0570b94674c5b9997929`, `CUDA_DEVEL_IMAGE` / `CUDA_RUNTIME_IMAGE` =
  `nvidia/cuda:13.4.3-{devel,runtime}-ubuntu24.04@sha256:<digest>` (resolve with
  `docker buildx imagetools inspect`), `IMAGE=iphands/ninfer-blackwell`, `MOBY_SECCOMP_REF=<tag>`
  (the moby tag for docker 29.8.0, confirmed in T2), `HF_BASE_REPO=Qwen/Qwen3.8-Flash-Next`,
  `HF_BASE_REV=de4b8e4d43b917e7706784d8bb445c9af86a3540`.
- `lib/common.sh` (sourced, `# shellcheck shell=bash`, every function ends `return 0`): parse
  `DOCKER` as a string into an array (refuse an array, refuse empty); refuse `sudo` on **both** the
  word arm and the path arm (`' '"${DOCKER[*]}"' ' == *' sudo '*` / `*'/sudo '*`); refuse `EUID 0`;
  probe `timeout 15 "${DOCKER[@]}" version` and print the last 3 lines on failure; `RUN_UID=1000`;
  `load_pins` (sources `versions.env`); `owned_dir <path>` (mkdir as you, then refuse unless owned
  by `RUN_UID`); `image_for_pin` (newest `$IMAGE` with label `ai.ninfer.ref=$NINFER_SHA`, empty if
  none). Reference only: `../vllmcustom/common/_run-lib.sh:42-160` — read it, do not source it.
- `models -> /mnt/noir/scratch/ai/llm/models` (the siblings' convention; tracked symlink).
- `README.md`: what this is, prerequisites, the three steps (`./build`, download + `./scripts/convert`,
  `./run`) as placeholders filled by T3–T5, and the "owner runs the long steps" note.
- Seed `context/` with this plan's confirmed Key Facts (one `distilled.md` entry per topic: io_uring
  + seccomp, the converter's BF16 contract, Flash-Next residency/PLE, serve defaults), the
  upstream-Dockerfile/liburing hazard and the "NVFP4 is not a base" trap in `pitfalls.md` (untagged
  — seeded from reading), and the four "Why" choices in `high_level.md` with **Revisit if** lines.
- Gate step 1: set the expected counts (`1 b` after this task: `lib/common.sh`).

**Before**: N/A — new files.

**Verify**:
```bash
bash -c 'set -a; . ./versions.env; set +a; echo "$NINFER_SHA $CUDA_DEVEL_IMAGE"'   # → the pins, digests included
docker buildx imagetools inspect "${CUDA_DEVEL_IMAGE}" >/dev/null && echo devel-ok     # → devel-ok (digest resolves)
bash -c '. lib/common.sh && echo guards-ok'                                            # → guards-ok
DOCKER='sudo -n docker' bash -c '. lib/common.sh' ; echo rc=$?                          # → refusal text, rc=1
DOCKER=/usr/bin/sudo bash -c '. lib/common.sh' ; echo rc=$?                             # → refusal text, rc=1
readlink models                                                                        # → /mnt/noir/scratch/ai/llm/models
# + project gate steps 1–2 (RULES.md § Project gate): counts "1 b", env ok, shellcheck ok
```
Expected: both sudo arms refused with rc 1; a plain source prints `guards-ok`; the digests resolve.

**Commit**: `[P1][T1][pins] add ninfer pins, shared guards, README, knowledge seed`

### T2: seccomp profile — docker's default plus io_uring

**File**: `scripts/seccomp-profile` (new), `seccomp/ninfer.json` (new, generated, committed), `seccomp/README.md` (new), `context/plans/RULES.md` (gate step 3)

**What to do**: Find the moby tag that docker 29.8.0 was cut from (`docker version --format
'{{.Server.GitCommit}}'` against `https://github.com/moby/moby` tags) and record it as
`MOBY_SECCOMP_REF`. `scripts/seccomp-profile` downloads `profiles/seccomp/default.json` at that ref
into a temp dir, records its sha256, and with `python3 -I` adds `io_uring_setup`,
`io_uring_enter` and `io_uring_register` to the unconditional `SCMP_ACT_ALLOW` entry (refuse if any
of the three already appears anywhere, or if the entry is not found — the profile's shape changed),
writing `seccomp/ninfer.json` deterministically (`sort_keys`, 2-space indent). `seccomp/README.md`
records the source URL, ref, source sha256, and the three added syscalls. Gate step 3 runs the
probe below in the converter image (`PROBE_IMAGE`, default the `../vllmcustom/container/.last-build`
image — it has python3; the NInfer runtime image has none).

**Before**: N/A — new files.

**Verify** (gate step 3, written into RULES in this commit):
```bash
P='import ctypes,os,sys; L=ctypes.CDLL(None,use_errno=True); b=ctypes.create_string_buffer(120)
r=L.syscall(425,8,b); print("uring", "ok" if r>=0 else os.strerror(ctypes.get_errno()))
r=L.syscall(250,0,-3,0); print("keyctl", "ok" if r>=0 else os.strerror(ctypes.get_errno()))'
for prof in default "$PWD/seccomp/ninfer.json" unconfined; do
  o=(); [ "$prof" != default ] && o=(--security-opt "seccomp=$prof")
  echo "== $prof"; docker run --rm --network none "${o[@]}" --entrypoint python3 "$PROBE_IMAGE" -I -c "$P"
done
```
Expected: `default` → `uring Operation not permitted` · `keyctl Operation not permitted`;
`ninfer.json` → `uring ok` · `keyctl Operation not permitted`; `unconfined` → `uring ok` and a
`keyctl` line that differs from `Operation not permitted` (else the negative control proves nothing
and a different blocked syscall must be chosen). `./scripts/seccomp-profile` run twice produces a
byte-identical file (`sha256sum`).

**Commit**: `[P1][T2][seccomp] add io_uring seccomp profile and gate probe`

### T3: `Dockerfile` + `build`

**File**: `Dockerfile` (new), `build` (new), `context/plans/RULES.md` (gate step 4), `README.md`

**What to do**:
- `Dockerfile`, two stages, build context `vendor/ninfer`:
  ```dockerfile
  # syntax=docker/dockerfile:1
  ARG CUDA_DEVEL_IMAGE
  ARG CUDA_RUNTIME_IMAGE
  FROM ${CUDA_DEVEL_IMAGE} AS build
  ARG DEBIAN_FRONTEND=noninteractive
  RUN apt-get update && apt-get install -y --no-install-recommends \
        cmake ninja-build pkg-config g++ libavcodec-dev libavformat-dev libavutil-dev \
        libswscale-dev libcurl4-openssl-dev libhwloc-dev liburing-dev \
      && rm -rf /var/lib/apt/lists/*
  WORKDIR /src
  COPY . .
  ARG JOBS=16
  RUN cmake -S . -B /build -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_CUDA_ARCHITECTURES=120a \
        -DNINFER_BUILD_APPS=ON -DBUILD_TESTING=OFF -DNINFER_BUILD_BENCHMARKS=OFF \
      && cmake --build /build --parallel "${JOBS}" --target ninfer ninfer-serve
  FROM ${CUDA_RUNTIME_IMAGE}
  # runtime libs: libavcodec60 libavformat60 libavutil58 libswscale7 libcurl4t64 libhwloc15 liburing2 ca-certificates
  # COPY both binaries to /usr/local/bin; LABEL ai.ninfer.ref / ai.ninfer.repo / ai.ninfer.cuda / ai.ninfer.arch=sm_120a
  # EXPOSE 8080; STOPSIGNAL SIGTERM; USER 1000:1000 LAST; ENTRYPOINT ["ninfer-serve"]; CMD ["--help"]
  ```
- `build` (sources `lib/common.sh`): manage `vendor/ninfer` — clone (`--filter=blob:none`) if
  absent; **refuse** if `git status --porcelain` prints anything; fetch only if `NINFER_SHA` is
  missing; `git checkout --detach "$NINFER_SHA"` (never `--force`); assert `HEAD == NINFER_SHA`.
  Tag `$IMAGE:cu<digits>-sm120a-<sha7>` (e.g. `cu1343-sm120a-64d10fb`). `JOBS` defaults to
  `$(( $(nproc) / 2 ))`. `PREFLIGHT_ONLY=1`: do the checkout checks, `imagetools inspect` both
  bases, `docker build --check` (lint only, no build), print the build command, exit 0. Otherwise
  `docker build -f "$HERE/Dockerfile" --build-arg … -t <tag> vendor/ninfer`, then print the image
  ID and its labels.
- Confirm the Ubuntu 24.04 package versions (`apt-cache policy cmake liburing-dev libhwloc-dev` in
  the devel image) and note them in `distilled.md`. `README.md`: the build step.

**Before**: N/A — new files.

**Verify**:
```bash
PREFLIGHT_ONLY=1 ./build                         # → checkout at 64d10fbf…, both bases resolve, `docker build --check` clean, the build command printed, exit 0
git -C vendor/ninfer status --porcelain | wc -l  # → 0
touch vendor/ninfer/DIRTY && PREFLIGHT_ONLY=1 ./build; echo rc=$?; rm vendor/ninfer/DIRTY   # → refusal naming the dirty tree, rc=1
# + project gate steps 1–4
```
Expected: preflight passes on a clean checkout and refuses a dirty one; no image is built.

**Commit**: `[P1][T3][docker] add pinned sm_120a Dockerfile and build script`

### T4: `scripts/convert` — the conversion, in a container

**File**: `scripts/convert` (new), `context/plans/RULES.md` (gate step 5), `README.md`

**What to do**: Inputs (env): `BASE` (default `models/vllm/Qwen/Qwen3.8-Flash-Next`), `NVIDIA`
(default `models/vllm/nvidia/Qwen3.8-Flash-Next-NVFP4`), `OUT` (default
`models/ninfer/qwen3.8-flash-next/qwen3_8_flash_next.ninfer`), `COMPONENTS=text,vision,mtp`,
`ROWS_PER_CHUNK=16384`, `NAME=qwen3.8-flash-next`, `CONVERT_IMAGE` (default: the image named by
`../vllmcustom/container/.last-build`), `CACHE_DIR` (default `/mnt/noir/scratch/ai/ninfer/cache`,
`owned_dir`), `DRY_RUN`, `INSPECT`. Host-side preflight, headers only (`python3 -I`, no torch):
- `BASE`: `config.json` architecture `Qwen4Exp*`; every shard in its index present; the
  `…layers.0.mlp.experts.gate_up_proj` and `…ngram_embedding.shard_0.weight` headers are **BF16**
  with the expected shapes. Else refuse and print the download command:
  `hf download Qwen/Qwen3.8-Flash-Next --revision <HF_BASE_REV> --local-dir models/vllm/Qwen/Qwen3.8-Flash-Next`.
- `NVIDIA`: `hf_quant_config.json` says NVFP4; 294,912 main-expert keys (48 × 512 × 3 × 4).
- `OUT`: neither the entry nor any `OUT.part-*` exists; its directory is `owned_dir`; ≥ 110 GB free.
- No container named `vllm`, nothing publishing `:8700` — the conversion needs the GPU.
- `vendor/ninfer` clean at `NINFER_SHA`.

Then: `docker run --rm -it --name ninfer-convert --gpus device=0 --user 1000:1000 --network none
--ulimit memlock=-1 -e HOME=/home/ubuntu -e PYTHONDONTWRITEBYTECODE=1 -e OMP_NUM_THREADS=4
-e MKL_NUM_THREADS=4 -e TORCH_EXTENSIONS_DIR=/home/ubuntu/.cache/torch_extensions
-v vendor/ninfer:/ninfer:ro -v $BASE:/in/base:ro -v $NVIDIA:/in/nvidia:ro -v $(dirname $OUT):/out
-v $CACHE_DIR:/home/ubuntu/.cache -w /ninfer --entrypoint python3 $CONVERT_IMAGE -m tools.convert
--model /in/base --recipe qwen3_8_flash_next --source nvidia=/in/nvidia --components $COMPONENTS
--rows-per-chunk $ROWS_PER_CHUNK --name $NAME --out /out/$(basename $OUT)` (real paths via
`realpath`, since `models` is a symlink), logging through `tee captures/convert_<date>.log` and
wrapping it in `/usr/bin/time -v` on the host side for wall time. `INSPECT=1`: run
`python3 -m tools.artifact.inspect /out/<entry> --json` in the same image without `--gpus`.

**Before**: N/A — new file.

**Verify**:
```bash
DRY_RUN=1 ./scripts/convert; echo rc=$?                    # → refuses: BASE missing + the hf download command, rc=1 (today's tree)
DRY_RUN=1 BASE=models/vllm/nvidia/Qwen3.8-Flash-Next-NVFP4 ./scripts/convert; echo rc=$?   # → refuses: base gate_up_proj missing / PLE shard F8_E4M3, not BF16; rc=1
docker run --rm --network none --user 1000:1000 -e PYTHONDONTWRITEBYTECODE=1 \
  -v "$PWD/vendor/ninfer:/ninfer:ro" -w /ninfer --entrypoint python3 "$CONVERT_IMAGE" -m tools.convert --help | head -3   # → usage text: the converter imports under Python 3.12 / torch 2.14
# gate step 5 (added here): fake BASE/NVIDIA trees with crafted safetensors headers → DRY_RUN prints a docker line with
#   --user 1000:1000, --gpus device=0, --network none, --recipe qwen3_8_flash_next, --source nvidia=/in/nvidia;
#   and each refusal (missing base, non-BF16 base, OUT exists, vllm running via a stub docker) fires
# + project gate steps 1–5
```
Expected: the two real refusals print exactly why; `--help` exits 0 inside the vLLM image.

**Commit**: `[P1][T4][convert] add containerized Flash-Next conversion script`

### T5: `run` — the launcher

**File**: `run` (new), `context/plans/RULES.md` (gate step 6), `README.md`

**What to do**: Source `lib/common.sh`; `load_pins`; refuse unless `image_for_pin` finds an image
(hint: `./build`). Knobs, each an env var with these defaults:

| Knob | Default | Flag |
|---|---|---|
| `ARTIFACT` (or `$1`) | `models/ninfer/qwen3.8-flash-next/qwen3_8_flash_next.ninfer` | positional (mounted dir `:ro` at `/models`) |
| `PORT` | `8700` | `-p 0.0.0.0:$PORT:8080` |
| `MODEL_ID` | `cosmo-6000` | `--model-id` |
| `MAX_CONTEXT` | `262144` | `--max-context` |
| `KV_CAPACITY` | `auto` | `--kv-capacity` |
| `MAX_CONCURRENCY` | `4` | `--max-concurrency` (NInfer max 8) |
| `KV_DTYPE` | `fp8` | `--kv-dtype` |
| `DRAFT_TOKENS` | `3` (`0` = no `--spec`) | `--spec mtp --draft-tokens` |
| `PREFILL_CHUNK` | empty (NInfer's default) | `--prefill-chunk` |
| `VISION` | `1` | `--vision` |
| `PRESERVE_THINKING` | `1` | `--preserve-thinking` (matches vLLM's chat-template kwargs) |
| `LOG_LEVEL` / `STATS_MS` | `info` / `1000` | `--log-level` / `--log-stats-interval-ms` |
| `REQUEST_LOG` | `1` | `--request-log-jsonl /storage/requests.jsonl` (`./storage`, `owned_dir`) |
| `CUDA_SYNC` | `spin` | `-e NINFER_CUDA_SYNC` (explicit, so the launch line records it) |
| `EXTRA_ARGS` / `ENV_EXTRA` | empty | appended **last** (last-wins) |

Docker args: `run -it --rm --name ninfer --gpus device=0 --user 1000:1000 -e HOME=/home/ubuntu
--security-opt seccomp=$HERE/seccomp/ninfer.json --ulimit memlock=-1 -v <artifact dir>:/models:ro
-p 0.0.0.0:$PORT:8080`, image, then `/models/<entry> --host 0.0.0.0 --port 8080 …`. Preflight:
the entry and every file its `.conversion.json` lists exist; `stat -f -c %T` of the artifact dir is
`nfs` → print that PLE reads hit NFS on every decode step (T11 measures it); `MemAvailable` < 80 GiB
→ warn (63.3 GiB of experts get registered); a running `vllm` container → **refuse** (one server
at a time). `DRY_RUN=1` prints the full command and exits 0. Echo the command before running it.

**Before**: N/A — new file.

**Verify** (gate step 6, added here): a stub `docker` on `PATH` (answers `version`, `images`,
`image inspect`, `ps`), a fake artifact dir with `x.ninfer` + `x.ninfer.conversion.json`:
```bash
OUT=$(DRY_RUN=1 PATH=/tmp/ninfer-gate6/stub:$PATH ./run /tmp/ninfer-gate6/model/x.ninfer 2>&1)
for want in '--user 1000:1000' 'seccomp=.*/seccomp/ninfer.json' '--ulimit memlock=-1' \
            '-p 0.0.0.0:8700:8080' '--spec mtp --draft-tokens 3' '--model-id cosmo-6000' \
            '--kv-capacity auto' '--gpus device=0'; do
  grep -qE -- "$want" <<<"$OUT" && echo "PASS($want)" || echo "FAIL($want)"
done
# + both sudo arms refused; a stub `ps` reporting a `vllm` container → refusal; DRAFT_TOKENS=0 → no --spec
# + project gate steps 1–6
```
Expected: every `PASS`; the refusals fire; the fake tree is removed afterwards.

**Commit**: `[P1][T5][run] add ./run launcher for NInfer on :8700`

### Wave 2 — build, convert, boot (owner runs; agent verifies and records)

*Wave 2 exits when `./run` boots the converted artifact and `scripts/smoke` passes against it.*

### T6: Owner builds the image; agent proves its identity (gate step 7)

**File**: `context/plans/RULES.md` (gate step 7), tracker Evidence, `context/distilled.md` (build facts), `context/pitfalls.md` (anything that broke)

**What to do**: Owner runs `./build` and pastes the tail. Agent then checks, and writes these
commands into RULES as gate step 7: exactly one image ID for `label=ai.ninfer.ref=$NINFER_SHA`;
`docker image inspect -f '{{.Config.User}}'` = `1000:1000`; `ldd /usr/local/bin/ninfer-serve` in
the image has no `not found`; `docker run --rm --network none <image> --help` prints usage; the
binary's cubins are `sm_120a` only (`docker create` + `docker cp` the binary to scratch, run
`cuobjdump --list-elf` from the converter image). Record build wall time and whether upstream's
missing-liburing hazard was real (it cannot be here — ours installs it — so record only what the
build log says about liburing's version).

**Verify**: the gate-7 block → `1`, `1000:1000`, `0` missing libraries, usage text, `sm_120a`-only
ELF list. + project gate steps 1–7.

**Expected observation**:
- **Confirms if**: all five assertions pass on the first build.
- **Refutes if**: the build fails (record the exact error in `pitfalls.md` as `[OBSERVED]`, fix in
  this task's commit), or an ELF for another arch appears.
- **Within noise if**: n/a (not a measurement).

**Commit**: `[P1][T6][docker] record built image identity, add gate step 7`

### T7: Owner downloads the BF16 base and converts; agent inspects the artifact

**File**: tracker Evidence, `context/distilled.md` (conversion time, artifact layout)

**What to do**: Owner: `hf download Qwen/Qwen3.8-Flash-Next --revision de4b8e4d43b917e7706784d8bb445c9af86a3540
--local-dir models/vllm/Qwen/Qwen3.8-Flash-Next`, stop vLLM, `./scripts/convert`. Agent:
`INSPECT=1 ./scripts/convert` and read `<entry>.conversion.json`.

**Verify**:
```bash
INSPECT=1 ./scripts/convert | head -40   # → arch Qwen4ExpForCausalLM; components text, vision, mtp; every part listed
du -cb models/ninfer/qwen3.8-flash-next/qwen3_8_flash_next.ninfer* | tail -1   # → ≈ 108 GB (100.67 GiB payload + framing)
# + project gate steps 1–7
```

**Expected observation**:
- **Confirms if**: the conversion completes, inspect lists all three components and every part,
  and the size lands within ±2% of 100.67 GiB.
- **Refutes if**: the converter fails under Python 3.12 / torch 2.14 (record the traceback,
  `[OBSERVED]` pitfall; the fallback is a Python 3.11 converter image — a Follow-up, not this
  task), or runs out of GPU or host memory (record the peak from `/usr/bin/time -v`).
- **Within noise if**: n/a. Record wall time and peak RSS as single-run facts, labelled as such.

**Commit**: `[P1][T7][convert] record Flash-Next artifact conversion and layout`

### T8: `scripts/smoke`; owner's first boot; is every expert on the GPU?

**File**: `scripts/smoke` (new), tracker Evidence, `context/distilled.md`, `README.md`

**What to do**: Agent writes `scripts/smoke` (bash + `curl` + `python3 -I`): `GET /health` → 200;
`GET /v1/models` lists `MODEL_ID`; a thinking chat returns non-empty `reasoning_content` **and**
`content`; a one-tool request returns `tool_calls` with parseable JSON arguments; with
`VISION=1`, a data-URI image request answers; `GET /metrics` has `ninfer_engine_ready 1`. Exit 0
only if all pass. Owner: stop vLLM, then `LOG_LEVEL=debug ./run` once and paste the startup log
(it carries the resource planning), then `./scripts/smoke`, `nvidia-smi`, and `free -g` while idle.

**Verify**: `./scripts/smoke` → every check `PASS`, exit 0; the boot log names the RTX PRO 6000. + project gate steps 1–7 (`scripts/smoke` joins the step 1 counts).

**Expected observation** (written before the boot):
- **Confirms if**: the planning log shows the main-expert cache holding all 48 × 512 experts (no
  CPU-computed subset at decode), GPU memory near full, ~63 GiB registered host memory, MTP K=3
  loaded, `NINFER_CUDA_SYNC=spin` reported.
- **Refutes if**: the cache holds only a fraction — then record the fraction, because it is the
  biggest single lever on this card, and open the Follow-ups (`--vision` off, smaller
  `MAX_CONCURRENCY`, smaller `MAX_CONTEXT`, each as a one-change arm). Or the boot dies on an
  `EPERM` from io_uring (the seccomp profile is wrong), on memlock, or on host-RAM registration.
- **Within noise if**: n/a.

**Commit**: `[P1][T8][smoke] add smoke checks, record first boot and expert residency`

### Wave 3 — measurement (owner runs; agent records per P1)

*Wave 3 exits when T9–T11 each have an Evidence row with ≥ 3 recorded runs per arm, mean and
spread, and a verdict that uses "within noise" where it applies.*

Common to T9–T11: before each arm, `nvidia-smi --query-gpu=power.limit,clocks.sm,temperature.gpu
--format=csv` and nothing else on the GPU (P1.5). Each arm = 1 unrecorded warm-up + recorded passes
of `../vllmcustom/bench/bench <label> --only code,long --model cosmo-6000 --out captures/bench_<date>_<arm>.md`
(single stream, `max_tokens` 1024). Read decode tok/s and TTFT per prompt from the `--out` file.

### T9: Same-session A/B — NInfer defaults vs vLLM FP4-Local

**File**: tracker Evidence, `context/distilled.md`

**What to do**: Arm A: `../vllmcustom/qwen3.8-flash-next/run-fp4local` (its own defaults:
humming, K=3), 1 + 4 passes. Stop it. Arm B: `./run` (T5 defaults), 1 + 4 passes. Check every
NInfer row's completion tokens: if any is < 1024, `ignore_eos` was ignored and the rows are not
comparable — record it, and re-measure arm B with a prompt that does not stop early before
drawing a conclusion.

**Verify**: both arms' `--out` files exist with 4 recorded passes; Evidence row names both images
(`…g40fe396` for vLLM, `ai.ninfer.ref` for NInfer), both launch lines, and the artifact. + project gate steps 1–7.

**Expected observation**:
- **Confirms if** (NInfer faster): NInfer's `code` and `long` decode ranges sit entirely above
  FP4-Local's (FP4-Local spread was ~11% on `code` in Plan 06).
- **Refutes if**: NInfer's ranges sit entirely below.
- **Within noise if**: the ranges overlap — write "within noise", and the default question moves
  to TTFT, prefill and stability instead.

**Commit**: `[P1][T9][bench] record NInfer vs FP4-Local same-session A/B`

### T10: MTP depth sweep on NInfer

**File**: tracker Evidence, `run` (default only if a K wins beyond noise), `README.md`

**What to do**: `DRAFT_TOKENS` ∈ {3 (T9's arm B reused only if same session; otherwise re-run),
2, 4, 5, 0 (no spec, the floor)}, 1 + 3 passes each, one change at a time. Record the mean MTP
acceptance per arm from the server's stats lines (`STATS_MS=1000`).

**Verify**: Evidence row per K with mean (min–max); `run`'s `DRAFT_TOKENS` default equals the
winner, or stays 3 with "within noise" written down. + project gate steps 1–7 (step 6's expected `--draft-tokens` follows the default).

**Expected observation**:
- **Confirms if**: one K's range sits above every other K's on both prompts.
- **Refutes if**: n/a — any ordering is a result.
- **Within noise if**: neighbouring Ks overlap — keep 3 (NInfer's own default, and vLLM's measured
  best on this model).

**Commit**: `[P1][T10][bench] record NInfer MTP depth sweep; set DRAFT_TOKENS`

### T11: PLE storage A/B — NFS vs local NVMe

**File**: tracker Evidence, `run` / `README.md` (only if the default changes)

**What to do**: Owner copies the entry, its parts and `.conversion.json` to a local NVMe directory
(e.g. `/home/iphands/ninfer-models/qwen3.8-flash-next/`, ~108 GB; check `df` first). Same session:
arm NFS (`./run`) and arm NVMe (`ARTIFACT=<copy> ./run`), 1 + 3 passes each. The PLE reads are
`O_DIRECT`, so the page cache cannot blur the two arms.

**Verify**: Evidence row per arm; the launcher's NFS warning appears only in the NFS arm. + project gate steps 1–7.

**Expected observation**:
- **Confirms if** (NVMe faster): the NVMe range sits above the NFS range on `code` decode → change
  the default artifact root to the NVMe path, document the copy step, and keep the copy.
- **Refutes if**: NFS is faster or equal beyond doubt → keep NFS and delete the copy (P2.4).
- **Within noise if**: overlap → keep NFS, delete the copy, record "within noise".

**Commit**: `[P1][T11][bench] record PLE storage A/B (NFS vs NVMe)`

### T12: Lock in the measured defaults; document the fastest setup

**File**: `run`, `README.md`, `context/distilled.md`, `context/high_level.md`, tracker

**What to do**: Apply only what T9–T11 proved beyond noise. Add a README section "Fastest settings
on this card" — a table of each knob, its value, and the Evidence row behind it, or "untested" —
and the commands to reproduce each number. Harvest every tracker finding into `context/` (Rule D).

**Verify**: `DRY_RUN=1 ./run` shows the documented defaults; every README number cites a tracker
Evidence row; + project gate steps 1–7.

**Commit**: `[P1][T12][run] lock measured-best defaults, document fastest setup`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `versions.env` | new — NInfer SHA, CUDA base images by digest, image name, moby seccomp ref, HF base rev | P0 |
| `lib/common.sh` | new — DOCKER/sudo/root guards, socket probe, pins, owned dirs, image lookup by label | P0 |
| `seccomp/ninfer.json` | new, generated — docker default + 3 io_uring syscalls | P0 |
| `scripts/seccomp-profile` | new — regenerates the profile from a pinned moby ref | P1 |
| `Dockerfile` | new — two-stage, CUDA 13.4.3, `sm_120a`, liburing, `USER 1000:1000` | P0 |
| `build` | new — pinned `vendor/ninfer` management, preflight, labelled build | P0 |
| `scripts/convert` | new — containerized `tools.convert`, header preflight, inspect mode | P0 |
| `run` | new — the launcher | P0 |
| `scripts/smoke` | new — health/models/thinking/tools/vision/metrics checks | P1 |
| `README.md` | new — build → convert → run, fastest-settings table | P1 |
| `models` | new symlink → `/mnt/noir/scratch/ai/llm/models` | P1 |
| `context/plans/RULES.md` | gate step 1 counts; steps 3–7 added by T2–T6 | P0 |
| `context/distilled.md`, `pitfalls.md`, `high_level.md` | seeded in T1, extended by every task | P1 |
| `../vllmcustom/bench/bench`, `../vllmcustom/qwen3.8-flash-next/run-fp4local`, `../vllmcustom` image | **used, not changed** | — |

Priority values: `P0` = blocking, `P1` = important, `P2` = nice-to-have.

---

## Open Questions / Risks

1. **Is the effect above noise?** FP4-Local's `code` spread was 188.5–210.6 tok/s (~11%) over 4
   runs. A NInfer win smaller than that is "within noise" by P1.2, and T9 must say so.
2. **Does it survive a restart?** A one-session A/B is provisional; T12 only locks a default that
   held in its own session, and the README says which session.
3. **Risk: the experts do not all fit on the GPU.** The CPU path on this AVX2-only host runs at
   `max_columns = 2`, so any CPU-computed experts would hurt badly. *Mitigation*: T8 reads the
   planning log first; the levers (`VISION=0`, smaller `MAX_CONCURRENCY` / `MAX_CONTEXT`) are
   one-change arms in the Follow-ups.
4. **Risk: PLE disk latency on NFS dominates decode.** *Mitigation*: T11.
5. **Risk: the converter breaks under Python 3.12 / torch 2.14.** *Mitigation*: T4 import check;
   T7 real run; fallback is a Python 3.11 converter image (Follow-up).
6. **Risk: conversion peaks in GPU or host memory are unknown.** *Mitigation*: `scripts/convert`
   refuses while vLLM runs and records `/usr/bin/time -v`.
7. **Risk: NInfer moves fast** (artifact v2 → v3 in October; it is a personal project). *Mitigation*:
   SHA pin; a bump is a plan (plan-gate row) and may mean re-conversion.
8. **Risk: one model alias.** Clients that send `claude-heavy` etc. straight to `:8700` will be
   rejected; `llama-proxy` sends `cosmo-6000`. *Mitigation*: `MODEL_ID=cosmo-6000`. deferred — an
   alias shim is outside this plan; revisit if a direct client breaks.
9. **Risk: known upstream bugs.** #399: one grammar violation in ~20k requests takes the engine to
   503 until restart. #394: prefix reuse degrades once the host context budget (8 GiB default) is
   full. *Mitigation*: noted in `pitfalls.md` (T1); `--host-context-mib` larger is a Follow-up.
10. **Risk: the seccomp profile drifts from docker's.** A docker upgrade changes the default
    profile; ours stays frozen at `MOBY_SECCOMP_REF`. *Mitigation*: `seccomp/README.md` says to
    regenerate on a docker upgrade; gate step 3 still proves io_uring works and the control stays
    blocked.
11. **Risk: `ignore_eos` is silently ignored**, so NInfer stops early and its tok/s is not
    comparable. *Mitigation*: T9's completion-token check.
12. **Question: does `cuobjdump` from CUDA 13.2 read CUDA 13.4.3 cubins?** *How we'll settle it*:
    T6; if not, run it from the NInfer devel stage instead.

---

## Verification Checklist

- [x] T1: `DOCKER='sudo -n docker'` and `DOCKER=/usr/bin/sudo` each → refusal, rc 1; both CUDA digests resolve; gate step 1 prints `1 b` and `env ok`
- [x] T2: gate step 3 → default `uring Operation not permitted`, `ninfer.json` `uring ok` with `keyctl` still blocked, unconfined differs on `keyctl`; regeneration is byte-identical
- [x] T3: `PREFLIGHT_ONLY=1 ./build` exit 0 on a clean checkout at `64d10fbf`, refusal on a dirty one, no image built
- [x] T4: today's `DRY_RUN=1 ./scripts/convert` refuses with the `hf download` command; NVFP4-as-base refuses naming the non-BF16 tensor; `tools.convert --help` exits 0 in the converter image; gate step 5 passes
- [x] T5: gate step 6 → every `PASS`, both sudo arms and the running-`vllm` case refused
- [ ] T6: gate step 7 → one image ID, `1000:1000`, no missing libraries, usage text, `sm_120a`-only cubins
- [ ] T7: inspect lists text/vision/mtp and every part; size within ±2% of 100.67 GiB payload
- [ ] T8: `scripts/smoke` exit 0; the planning log's expert-cache line recorded verbatim, with the GPU-resident fraction
- [ ] T9: 4 + 4 recorded passes, both images named, completion tokens checked, verdict uses "within noise" if ranges overlap
- [ ] T10: ≥ 3 passes per K ∈ {0,2,3,4,5}; `run`'s default matches the Evidence
- [ ] T11: ≥ 3 passes per storage arm; default changed only beyond noise; unneeded copy deleted
- [ ] T12: every README number cites an Evidence row; `DRY_RUN=1 ./run` shows those defaults
- [ ] All: the project gate (Rule A) passes on the final commit, every applicable step `pass`
- [ ] All: the runtime tier ran (owner), **or** the tracker says the behavior went unverified at runtime
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
