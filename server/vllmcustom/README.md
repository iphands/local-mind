# vllmcustom — source-built, sm120-optimized vLLM

A self-owned vLLM build for the **RTX PRO 6000 Blackwell (sm_120)**. No prebuilt
images: PyTorch, FlashInfer, and vLLM are all compiled from source for
`TORCH_CUDA_ARCH_LIST=12.0` on CUDA 13.

## Layout

```
container/   build, push, Dockerfile, and the two build helpers (preflight, slice-setup)
common/      the shared launcher mechanism every */run sources
qwen/        run — Qwen3.5 / Qwen3.6
laguna/      run — Laguna-S DFlash
muse/        run — Muse Glimmer, plus patches/ (the shims it mounts at /vllm-patches)
bench/       the measurement harness: bench, the sweeps, lib/, and all run artifacts
scripts/     one-off probes and per-model sweeps (mostly Muse)
notes/ plans/ vendor/ models/
```

Each `*/run` serves one model family and owns its own settings — parsers, sampling
defaults, context length, patches. What they share is the plumbing, which lives in
`common/`: resolving the image, turning a model argument into the `/models` bind
mount, the docker flags, the `vllm serve` flags that are the same everywhere, and
the launch. `common/_run-lib.sh` documents the call order a launcher must follow;
`common/_model-lib.sh` is the model-path resolver on its own because a wrong answer
there changes a bind mount silently rather than failing.

`bench/` holds everything a benchmark writes, so wiping results is
`rm -rf bench/bench-logs`.

Every launcher uses the **same container name (`vllm`) and port (`8700`)** — only
one model is ever up at a time, so `./bench/bench` and every sweep script find
whichever one is running without being told which. Starting a second launcher
while one is up fails on the name conflict rather than quietly serving the wrong
model on another port.

All of them take the model as `$1`, in any of these forms, and print what they
resolved before doing anything slow:

```bash
./muse/run                                                # the default for that launcher
./muse/run ./models/vllm/RedHatAI/Muse-Glimmer-30B-NVFP4  # a path under models/vllm
./muse/run RedHatAI/Muse-Glimmer-30B-NVFP4                # or just the name
```

| script   | what it does |
|----------|--------------|
| `./container/build`| clone/pull pinned source into `$BUILD_DIR/src`, compile, tag image |
| `./qwen/run`  | serve a model with the local image (GPU 0 only), OpenAI API on `:8700`. Default `Qwen3.5-122B-A10B-NVFP4` |
| `./muse/run`  | serve Muse Glimmer — same image, plus the shims in `muse/patches/` (see below). Default `RedHatAI/Muse-Glimmer-30B-NVFP4` |
| `./laguna/run`| serve Laguna-S with its DFlash drafter. Default `Laguna-S-2.1-NVFP4` |
| `./bench/bench`| client-side TTFT + decode tok/s against `:8700`, logs `bench/bench-results.md` |
| `./bench/bench-wrapper`| sweep NVFP4-backend × MTP configs: start/stop vLLM per config, warmup, measure, print table |
| `./bench/bench-context`| large-context decode test: per backend, 3-turn convo + padded probes (8k–128k), TG-vs-depth |
| `./bench/make-test-convo`| build the static ~100k-token "summarize Quake II source" turn from `vendor/yquake2` into `bench/data/test-convo.json` |
| `./container/push` | `docker push` to `docker.io/iphands/vllm-blackwell` |
| `bench/lib/`   | the shared measurement client (`benchclient.py`) + the Python drivers the bench scripts invoke (`measure.py`, `ctxbench.py`) |

## Quick start

```bash
./container/build                              # first build compiles torch from source (slow; see below)
./qwen/run ./models/vllm/Qwen3.6-27B           # serves on http://localhost:8700/v1
./bench/bench baseline                         # measure tok/s of the running server
```

`models` is a symlink to `/mnt/noir/scratch/ai/llm/models`, so paths match the
old `server/vllm` scripts:

```bash
./qwen/run ./models/vllm/Qwen3.5-122B-A10B-NVFP4
./qwen/run                                     # same thing — that model is the default
```

## Default version set (mutually compatible)

vLLM `v0.28.0` pins these (see its `requirements/cuda.txt`), so they are the defaults:

| component  | ref/version | arch flags |
|------------|-------------|------------|
| CUDA       | 13.2.1 (`cudnn-devel-ubuntu24.04`) | — |
| PyTorch    | v2.13.0 (**from source**) | `TORCH_CUDA_ARCH_LIST=12.0` |
| FlashInfer | v0.6.16.post3 (**from source**) | `FLASHINFER_CUDA_ARCH_LIST=12.0f` |
| vLLM       | v0.28.0 (**from source**) | `TORCH_CUDA_ARCH_LIST=12.0`, `VLLM_USE_PRECOMPILED=0` |
| torchvision/torchaudio | 0.28.0 (cu132 wheel) / 2.11.0 (cu130 wheel), `--no-deps` | not perf-critical |

torchaudio 2.11.0 alongside torch 2.13.0 is not a typo — that is upstream's own pairing.
torchaudio comes from the cu130 index because it never published cu132 wheels; a cu130
wheel runs on any 13.x toolkit (shared `libcudart.so.13`). `./container/preflight` checks
both wheels exist before anything compiles.
`flashinfer-cubin` stopped publishing to PyPI after 0.6.13, so the build pulls it from
`--extra-index-url https://flashinfer.ai/whl/` (matching vLLM's `requirements/cuda.txt`).

### Why not newer (as of 2026-09-05)

| candidate | status | blocker |
|---|---|---|
| torch 2.14.0 (2026-08-26) | released | no vLLM release **or rc** pins it — v0.29.0rc4 and `main` still pin 2.13.0 |
| FlashInfer 0.6.18.post1 (2026-09-04) | released | vLLM's wheel hard-pins `flashinfer-python==X`; v0.28.0 wants 0.6.16.post3, v0.29.0rc4 wants 0.6.18 |
| CUDA 13.3.1 | image published; driver 610.43.03 ≥ its 610.43.02 floor | not in torch's binary matrix (12.6/12.9/13.0/13.2), no FlashInfer/vLLM CI; 13.3+ nvcc changed its dry-run output (broke sccache upstream) |
| vLLM v0.29.0 | at rc4 (2026-09-04) | not tagged yet; when it is: `VLLM_REF=v0.29.0 FLASHINFER_REF=v0.6.18 ./container/build` |

vLLM upstream still builds its own images on CUDA 13.0.3, so `CUDA_VERSION=13.0.3` is the
conservative fallback if 13.2.1 misbehaves (it produces the separate `cu1303-sm120` tag).

## Trying other CUDA / versions

Everything is a build-arg; the CUDA version becomes part of the tag so variants
coexist:

```bash
CUDA_VERSION=13.3.1 ./container/build               # -> iphands/vllm-blackwell:cu1331-sm120 (untested upstream)
CUDA_VERSION=13.0.3 ./container/build               # -> iphands/vllm-blackwell:cu1303-sm120 (vLLM's own CI toolkit)
VLLM_REF=v0.25.1 TORCH_REF=v2.11.0 FLASHINFER_REF=v0.6.13 ./container/build
```

Then run a specific variant and benchmark it:

```bash
IMAGE_TAG=cu1303-sm120 ./qwen/run ./models/vllm/Qwen3.6-27B
./bench/bench cu1303-flashinfer
```

## A/B-testing backends (runtime, no rebuild)

Backends and the main perf knobs are runtime env on `./qwen/run`:

```bash
ATTN_BACKEND=flashinfer ./qwen/run ./models/vllm/Qwen3.6-27B   # default (correct for Qwen3.5)
ATTN_BACKEND=triton     ./qwen/run ./models/vllm/Qwen3.6-27B
MOE_BACKEND=flashinfer_b12x ./qwen/run ./models/vllm/Qwen3.5-122B-A10B-NVFP4  # SM12x fused MoE (opt-in)
NVFP4_BACKEND=cutlass   ./qwen/run ./models/vllm/Qwen3.5-122B-A10B-NVFP4   # NVFP4 GEMM kernel
SPEC_TOKENS=3           ./qwen/run ./models/vllm/Qwen3.5-122B-A10B-NVFP4   # MTP tokens (default 2)
CUDAGRAPH_MODE=FULL_AND_PIECEWISE ./qwen/run ./models/vllm/Qwen3.5-122B-A10B-NVFP4  # full CUDA graphs
FLASHINFER_AUTOTUNE=1   ./qwen/run ./models/vllm/Qwen3.5-122B-A10B-NVFP4   # autotune during warmup
MAX_BATCHED_TOKENS=32768 ./qwen/run ./models/vllm/Qwen3.6-27B  # faster long-context prefill (default 8192)
EXTRA_ARGS="--max-num-seqs 8" ./qwen/run ./models/vllm/Qwen3.6-27B
```

Benchmark each with a distinct label; rows accumulate in `bench/bench-results.md`:

```bash
./bench/bench cu1303-flashinfer
./bench/bench cu1303-cutlass-mtp2
```

## Performance tuning (sm120, single GPU)

Optimizing **single-stream decode tok/s** on one RTX PRO 6000. Distilled from the
`vendor/rtx6kpro` community wiki (ignore its multi-GPU/NVLink/NCCL/DCP advice — we
have one card).

**Already optimal in `./qwen/run` / `./container/build` (leave alone):**
- **sm120f compile** (`FLASHINFER_CUDA_ARCH_LIST=12.0f`) — *the* critical NVFP4 fix;
  enables the `cvt.rn.satfinite.e2m1x2.f32` FP4 PTX path. Without `f`, NVFP4 is slower
  than int4.
- **MTP=2** speculative decoding (+50–55% decode, ~89% accept) — the single biggest
  decode lever.
- **FP8 KV cache**, prefix caching, chunked prefill, async scheduling, CUDA graphs
  (`--no-enforce-eager`), `--language-model-only` (kills a ~12s vision TTFT spike).
- **flashinfer attention** — correct for Qwen3.5 (hybrid GDN/full attn). `TRITON_MLA`
  is only for MLA models (Kimi/GLM), not this one.

**Worth A/B testing (decode levers):**
- `MOE_BACKEND=flashinfer_b12x` — FlashInfer's CuTe DSL fused MoE built specifically
  for SM12x (RTX PRO 6000 / DGX Spark). vLLM *deliberately excludes it from
  auto-selection* (pending an upstream CUTLASS SM121 guard fix), so it never runs
  unless opted into — potentially the biggest untested decode lever for a 122B MoE.
- `NVFP4_BACKEND` — vLLM's NVFP4 GEMM kernel. `cutlass` (internal sm120f) is the
  fastest per the wiki; `flashinfer-cudnn` is the *safest* (sidesteps a FlashInfer
  CUTLASS FP4 race condition that silently NaNs); `marlin` is a W4A16 fallback if FP4
  GEMM ever produces garbage; `flashinfer-b12x` is the same SM12x CuTe DSL kernel for
  dense GEMM. Empty = vLLM auto. (All passed as `--linear-backend`. The old
  `VLLM_NVFP4_GEMM_BACKEND` env was deprecated in v0.23 and is gone from `vllm/envs.py`
  as of v0.27.0, which also promoted `flashinfer_b12x` into the flag's own choices.)
- `SPEC_TOKENS` — MTP=2 is the safe sweet spot; MTP=3 *may* add a bit for single
  streams (its instability is a long-context/high-concurrency problem). Watch the logs
  for `probability tensor contains inf/nan` → back off to `flashinfer-cudnn` or MTP=2.
- `CUDAGRAPH_MODE=FULL_AND_PIECEWISE` — full CUDA graphs for small-batch decode
  (exactly this workload) instead of piecewise-only.
- `FLASHINFER_AUTOTUNE=1` — FlashInfer kernel autotuning during warmup.
- `MAX_BATCHED_TOKENS` — 16384/32768 speeds long-context prefill/TTFT (decode
  unaffected; default 8192).

`VLLM_LOG_STATS_INTERVAL=1` is on by default so each run prints live tok/s + MTP
acceptance for comparison.

**Automated sweep:** `./bench/bench-wrapper` runs the whole matrix for you — it restarts
vLLM per config, warms up (mandelbrot prompt), measures (primes prompt), and prints a
table of **CAP(W)** / TTFT / prefill / **TG (token-gen) tok/s**, appending to
`bench/bench-wrapper-results.md`. It also writes a long-term `bench/bench-results.jsonl` (one
record per run, written by `bench/lib/measure.py`) that includes the **GPU0 power cap**
(read — never set — via `nvidia-smi`) plus the image tag and vLLM version, so old rows
stay comparable across rebuilds. Config lines are
`<name> <NVFP4_BACKEND|-> <SPEC_TOKENS> [MOE_BACKEND|-]` (the MoE column is optional).
Measured requests set `ignore_eos`, so TG always covers exactly `GEN_TOKENS` tokens.
Each config is a full 122B reload (minutes), so the default 8-config sweep is
long; trim via the `CONFIGS` env. Example:

```bash
./bench/bench-wrapper /mnt/noir/scratch/ai/llm/models/vllm/Qwen3.5-122B-A10B-NVFP4
RUNS=3 ./bench/bench-wrapper            # 3 measured runs/config, median TG
```

**Large-context decode (`./bench/bench-context`):** `bench/bench-wrapper` only exercises ~1k
context, so it shows best-case decode. `bench-context` characterizes **TG vs. context
depth** — the number that actually drops as the KV cache fills — two ways, per backend:
- **convo**: a realistic 3-turn chat — (1) Bash prime sieve, (2) port to Python, (3) a
  static "summarize this Quake II source" turn that injects ~100k tokens of real C code
  (history is resent each turn, exercising prefix caching). Turn 3 is the big-context
  measurement. **Generate that turn first** with `./bench/make-test-convo` (reads
  `vendor/yquake2` → `bench/data/test-convo.json`); if it's missing, bench/bench-context warns and
  skips turn 3.
- **pad**: prompts padded to exact `PAD_SIZES` (default `8192 32768 65536 131072`),
  short generation — a clean, repeatable decode-vs-context curve to compare backends at
  the same depth.

```bash
./bench/make-test-convo                                 # build the ~100k-token quake2 turn (once)
QUAKE_TOKEN_BUDGET=150000 ./bench/make-test-convo       # bigger turn (default 120k budget ≈ 104k real)
./bench/bench-context                                   # ALL permutations (default): {auto,cutlass,cudnn,marlin} × MTP{0,1,2} + b12x
PAD_SIZES="8192 32768" CONFIGS=$'cutlass cutlass 2' ./bench/bench-context   # quick single-backend subset
```

By default `./bench/bench-context` (no args) runs the **full 14-config matrix** — every NVFP4
backend × MTP {0,1,2} plus the SM12x b12x GEMM/MoE opt-ins, each a full reload +
3-turn convo + pad-to-128k. Budget **hours**; trim with `CONFIGS=…` / `PAD_SIZES=…`
for a faster pass. Config lines take the same optional 4th MoE-backend column as
bench-wrapper.

The generator is deterministic (sorted files) so turn 3 is byte-identical across
backends — a fair A/B at the same large context. Its `approx_tokens` is a chars/token
estimate (default 2.3 chars/token, measured on this C corpus — the old 3.6 default
made a 120k budget tokenize to ~190k, so regenerate `bench/data/test-convo.json` if yours
predates that); the **real** depth is the `context_tokens` bench-context reports for
the `quake2` turn — tune `QUAKE_TOKEN_BUDGET` from that.

Rows land in `bench/bench-context-results.md` (tables) and `bench/bench-results.jsonl` (with
`mode=convo|pad`, `turn_label`, `context_tokens`, `cached_tokens`, `answer_ttft_ms`,
the watt cap, and image/vLLM-version provenance). Caveats: **MTP can crash at long
context** (a failed turn is recorded and the run continues — use a `mtp0` row for a
clean number); the 128k `pad` probe needs the KV cache to fit in VRAM (an OOM probe is
recorded, not fatal); and in `convo` mode read **TG**, not TTFT (prefix caching).

> **Historic-data caveat:** jsonl rows written before 2026-07 with `ttft_ms: 0.0` are
> invalid — the old clients only detected `content` deltas, but the qwen3 reasoning
> parser streams `reasoning_content` first, so TTFT never fired and the whole prefill
> was counted as generation time. All pad rows and any all-thinking convo turns
> understate TG badly (e.g. at 131k the recorded 7.47 tok/s is mostly prefill).
> Re-run the sweeps for trustworthy TG-vs-depth curves.

### Host tuning (outside the container)
- `sudo nvidia-smi -pm 1` then set max power limit (`sudo nvidia-smi -pl <max>`).
  Note: single-stream **decode is memory-bandwidth-bound and memory clock is
  power-limit-invariant** on these SKUs, so this mostly helps prefill/TTFT — don't
  expect decode gains.
- CPU: `governor=performance`, `vm.swappiness=0`, `kernel.numa_balancing=0`.
- GRUB `pcie_aspm=off pcie_port_pm=off` — avoids PCIe "Surprise Link Down" lockups
  under sustained load.

### Version notes (Tier 3, investigated)
The v0.27.0 bump was taken for architecture support, not decode throughput: it lands
native `DiffusionGemmaForBlockDiffusion` (unblocking `./qwen/run-diffusiongemma`) plus
`LagunaForCausalLM` / `DFlashLagunaForCausalLM`. Decode-wise the earlier pinned set was
already fine — flashinfer has carried the sm120f FP4 module since 0.6.12 (PR #2650).

A further bump is not free: it drags torch and flashinfer with it (0.27.0 moved torch
2.11.0 → 2.13.0), which invalidates the `torch-build` layer and most of ccache, so
budget a full multi-hour rebuild rather than an incremental one. Read the target tag's
`requirements/cuda.txt` first and move `TORCH_REF`/`FLASHINFER_REF` in `./container/build` to match.

`MuseGlimmer` still has no native implementation as of v0.27.0, so three shims are
required, all runtime — no rebuild:

- `muse/patches/muse-embed-norm/sitecustomize.py` hosts five shims (one `sitecustomize` per
  `PYTHONPATH`, so they share a file). Two matter most: it restores the embedding RMSNorm the
  Transformers fallback drops, and it repairs EAGLE3 aux hidden-state capture — vLLM's
  Transformers backend advertises the interface but silently captures nothing on this model,
  which kills the server on `assert isinstance(model_output, tuple)` the moment
  speculation is on. It also reshapes the drafter's config, since `--hf-overrides`
  cannot reach a draft config (dict overrides are target-only by design).
- `muse/run`'s `--hf-overrides` supplies `text_config.logit_scale` and hoists
  `layer_types`/`sliding_window`.
- `muse/patches/muse-dflash/muse_dflash.py` registers a drafter class for
  `Muse-Glimmer-30B-assistant`; vLLM 0.27.1 supports the `dflash` *method* but has no
  architecture for this checkpoint. It is a thin subclass of vLLM's own
  `DFlashQwen3ForCausalLM` — after renaming the two `encoder.*` tensors, the checkpoint's
  key set is structurally identical to `Qwen3.6-27B-DFlash`.

Check the drafter with `./scripts/muse-spec-check` (greedy parity + acceptance rate +
tok/s), and the parsers with `./scripts/muse-e2e`.

## Preflight — fail fast on an incompatible version set

vLLM pins torch/flashinfer as a matched set and bakes
`Requires-Dist: flashinfer-python==X` into its wheel, so bumping
`FLASHINFER_REF` on its own produces a build that **cannot finish** — and the
failure only surfaces in the very last stage, at
`uv pip install /wheels/*.whl`, i.e. after flashinfer's ~35 min AOT recompile
and vLLM's 404 CUDA targets:

```
× No solution found when resolving dependencies:
╰─▶ Because vllm>=0.27.1 depends on flashinfer-python==0.6.16.post3
    and you require flashinfer-python==0.6.17, ... unsatisfiable.
```

`./container/build` now runs `container/preflight` first, which compares every pin against
the target vLLM's `requirements/cuda.txt` and confirms the `flashinfer-cubin`
wheel is actually published. It runs **entirely on the host** — it never invokes
`docker build`, so it cannot invalidate a layer.

```bash
PREFLIGHT_ONLY=1 ./container/build                      # sync + check, then stop
VLLM_REF=v0.28.0 PREFLIGHT_ONLY=1 ./container/build     # "would this vLLM work with my pins?"
ALLOW_VERSION_MISMATCH=1 ./container/build              # build the untested combination anyway
PREFLIGHT_OFFLINE=1 ./container/build                   # skip the wheel-index lookup
```

The second form is the useful one when a new release lands: it answers whether a
vLLM bump needs `TORCH_REF`/`FLASHINFER_REF` moved with it, in about a second and
without touching the cache.

Note what preflight can and can't tell you. A `flashinfer-python` mismatch is a
hard resolver failure. A **`torch` mismatch is not** — `use_existing_torch.py`
strips that pin so the source build is used, so nothing in the build will ever
complain; it just means vLLM was compiled against a torch upstream never tested.
Preflight flags it precisely because the build won't.

## Caching — why builds aren't from scratch every time

- **Source** lives in `$BUILD_DIR/src/{pytorch,flashinfer,vllm}` (default
  `/mnt/noir/scratch/ai/vllm/build`). `./container/build` only `git fetch` + `checkout`s
  the pinned ref — **no re-clone**. Patch or `git checkout` in place to iterate.
- **Compiler cache**: the container/Dockerfile uses BuildKit `--mount=type=cache` for
  ccache + uv. This is the supported alternative to host bind mounts (Docker
  forbids arbitrary host bind mounts inside `RUN`) and persists across builds on
  the same daemon exactly like a bind mount would — so a small source change
  re-links in minutes instead of recompiling torch from scratch.
- **Runtime caches** (HF downloads, `torch.compile`, FlashInfer JIT) bind-mount
  from `/mnt/noir/scratch/ai/vllm/cache` into the container, so model loads and
  JIT compiles persist between `./qwen/run`s.

To inspect/relocate the ccache on disk, export it:
`docker buildx build ... --cache-to type=local,dest=$BUILD_DIR/ccache`.

### Two things deliberately kept *out* of the cache key

Both exist because they were causing full `torch → flashinfer → vllm` rebuilds
(~35 min of flashinfer AOT each) when nothing about the output had changed. The
stages are a chain, so anything that invalidates torch invalidates everything —
which is why flashinfer seemed to rebuild constantly when its own inputs were
untouched.

- **Build-speed knobs travel by secret mount.** `MAX_JOBS` / `NVCC_THREADS` are
  passed as `--secret id=buildjobs` and sourced inside each compile `RUN`, not as
  `ARG`/`ENV`. BuildKit excludes secret *contents* from the cache key, so
  retuning reuses the cached layers. This is correct, not just convenient: these
  knobs change how fast a build runs, never what it produces. Verified both ways
  — changing `MAX_JOBS` keeps layers `CACHED`, while a semantic change still
  invalidates. **Don't move them back into `ENV`.**
- **`.git` is excluded in the container/Dockerfile**, via `COPY --exclude=.git
  --exclude=**/.git` (needs the `dockerfile:1.7-labs` syntax on line 1), rather
  than by `.dockerignore` files inside the source trees. Two reasons: pytorch's
  `.dockerignore` is a symlink to `.gitignore`, which does *not* exclude `.git`,
  so 5.8 GB of git objects (72% of that context) sat in the `COPY` cache key and
  churned on every `git fetch`; and those trees get `git checkout --force`d, so
  any `.dockerignore` living in them is unversioned and clobberable. No stage
  needs git metadata, but **only because the version is forced explicitly** in
  each: `PYTORCH_BUILD_VERSION` for torch and `VLLM_VERSION_OVERRIDE` for vLLM.
  That second one is load-bearing — vLLM's version comes from setuptools-scm, so
  without it, excluding `.git` fails the build outright with *"setuptools-scm was
  unable to detect version for /src/vllm"*. Note the scoped
  `SETUPTOOLS_SCM_PRETEND_VERSION_FOR_VLLM` does **not** work: `setup.py` calls
  `get_version()` with no `dist_name`, so the `_FOR_VLLM` suffix has nothing to
  match. It sat in the container/Dockerfile doing nothing for as long as `.git` was present
  to cover for it.

Note `docker build --check` reports `unknown flag: exclude` here. That is a false
alarm: the checker lints with a non-labs frontend (`dockerfile:1.8.1`) instead of
the `1.7-labs` the file declares. Real builds are unaffected.

## Memory-capped builds

The host has 125 GB and **no swap**, so an OOM during compilation isn't a build
failure — the kernel OOM-killer fires against the whole machine. Parallel `nvcc`
is what gets it there. Measured on the flashinfer AOT stage (3408 kernels, the
worst offender): 17 concurrent `cicc` held **82.3 GB** at one sample. A complete
build then measured a **89.8 GiB peak at 14 jobs = 6.4 GB/job**, so an unbounded
28-way build wants ~180 GB.

`./container/build` is capped at `BUILD_MEM_GB` (default 110G) by two independent layers:

- **Derived `MAX_JOBS`** — `BUILD_MEM_GB × MEM_HEADROOM_PCT / MEM_PER_JOB_GB`
  (defaults 80% and 6 GB), clamped to `nproc - 4`. On this box that's **14**
  jobs instead of 28: ~89 GiB at the measured rate against a 110 GiB cap. This
  is the prevention layer — no privileges, no setup. Setting `MAX_JOBS`
  explicitly bypasses the calculation entirely.
- **A cgroup ceiling** — `./container/build` passes `--cgroup-parent=vllmbuild.slice`, a
  systemd slice with `MemoryMax=110G`. That's the wall, where the OOM killer
  fires **scoped to that cgroup** — it kills an `nvcc`, not your desktop session
  or a running vLLM server. Containment, not prevention: hitting it still loses
  the build, just not the machine.

> **Why the headroom fraction, and why no `MemoryHigh`** — both learned the hard
> way, and both produce a build that runs overnight without finishing:
>
> Sizing jobs against the *whole* budget (`BUILD_MEM_GB / MEM_PER_JOB_GB`) makes
> predicted peak equal the cap by construction — 18 × 5 GB = 90 GB = the limit —
> so the build parks on the ceiling permanently. `MEM_HEADROOM_PCT=100`
> reproduces exactly that.
>
> `MemoryHigh` looks like the gentler knob (throttle before you kill) but here it
> is a **livelock**. `memory.high` throttles by forcing direct reclaim, which only
> degrades gracefully when something is reclaimable — page cache or swap. This
> build is 99.97% anonymous (90 GB anon vs 23 MB file cache) with no swap, so
> nothing can be evicted: the kernel burns itself scanning an anon LRU it can
> never free, and the only reclaimable memory left is the mapped text of
> `cicc`/`nvcc` themselves, which major-faults straight back in. Observed with
> `MemoryHigh=84G`: 397M `memory.high` events, 7.7B file refaults, 37M major
> faults, **3.3 GB/s of disk reads at ~0% user CPU**, and no forward progress —
> while `memory.max` and `oom_kill` both stayed at 0. `MemoryMax` alone fails
> fast instead of hanging forever.
>
> If a build ever crawls with high iowait, check this first:
>
> ```bash
> cat /sys/fs/cgroup/vllmbuild.slice/memory.events   # 'high' climbing = throttled
> grep -E '^(anon|file) ' /sys/fs/cgroup/vllmbuild.slice/memory.stat
> ```
>
> Adding swap would make both failure modes far less sharp — with anything to
> page out, overshoot degrades instead of livelocking or OOM-killing.

Install the slice once, as root:

```bash
su -c "$PWD/container/slice-setup"
```

Until you do, `./container/build` prints a warning and runs with `MAX_JOBS` as the only
limit — it never hard-fails on a missing slice. `docker buildx build` has no
`--memory` flag (that was classic-builder only), so `--cgroup-parent` is the
supported mechanism here; the BuildKit embedded in dockerd does honor it, and
`RUN` steps land in `/sys/fs/cgroup/vllmbuild.slice/buildkit/`.

> **Gotcha:** BuildKit *creates* the cgroup itself, uncapped, if the directory
> doesn't already exist — so pointing `--cgroup-parent` at a unit that was never
> `systemctl start`ed silently gives you no ceiling at all, while `systemctl show`
> still reports the configured 90G from the unit file. That's why `slice-setup`
> starts the slice, and why `./container/build` verifies live kernel state (`memory.max`)
> rather than asking systemd for the unit's configured value.
>
> **Why the name has no dash:** systemd reads `-` in a slice name as a hierarchy
> separator, so a `vllm-build.slice` silently nests under an implicit
> `vllm.slice` and lives at `/sys/fs/cgroup/vllm.slice/vllm-build.slice`. Since
> BuildKit treats `--cgroup-parent` as a literal path, the dashed name would send
> it to a *different*, uncapped cgroup. `vllmbuild.slice` is top-level, so the
> unit name and the path component match. Both scripts resolve the path from
> `systemctl show -p ControlGroup` anyway, so a rename stays correct.
>
> To confirm the cap is genuinely live:
>
> ```bash
> cat /sys/fs/cgroup/vllmbuild.slice/memory.max   # a number, not "max"
> ```

To retune, re-run both with a new budget — they read the same env var:

```bash
BUILD_MEM_GB=64 su -c "$PWD/container/slice-setup"   # move the hard ceiling
BUILD_MEM_GB=64 ./container/build                            # and the derived MAX_JOBS
```

> **Leave `NVCC_THREADS=1` alone.** vLLM's `setup.py` (`compute_num_jobs()`)
> computes its ninja parallelism as `max(1, MAX_JOBS // NVCC_THREADS)`. At the
> old `NVCC_THREADS=8`, a `MAX_JOBS` of 14 floored to **`ninja -j 1`** — the
> vllm-build stage compiled its 404 CUDA targets one at a time at ~3% CPU, while
> torch and flashinfer (which use `MAX_JOBS` directly) looked perfectly healthy.
> Since `nvcc -t` only parallelises across arch targets and we build the single
> `12.0` target, raising it buys nothing and silently divides this stage's
> throughput. If vllm-build ever looks idle, check first:
>
> ```bash
> ps -eo args | grep -oE 'ninja -j *[0-9]+'   # want -j MAX_JOBS, not -j 1
> ```

`MEM_PER_JOB_GB=6` comes from a completed build: 89.8 GiB peak ÷ 14 jobs =
6.4 GB/job. Don't tune it from an average sampled mid-build — an earlier 4.9 GB
average was taken during a light stretch of flashinfer's AOT stage and
underestimated by 30%, because per-kernel demand varies enormously.

**`BUILD_MEM_GB` moves two things at once.** It sets the cgroup ceiling *and*
feeds `MAX_JOBS`, so raising it alone buys no headroom — the extra budget is
immediately spent on more jobs (110G with the old 5 GB/job gives 17 jobs ≈
108 GiB, right back at the wall). Move `MEM_PER_JOB_GB` with it.
After a full build
read the real high-water mark and adjust:

```bash
cat /sys/fs/cgroup/vllmbuild.slice/memory.peak
```

If peak lands well under the budget, lower `MEM_PER_JOB_GB` to buy back
parallelism; if it brushes the ceiling, raise it.

## Notes / caveats

- **First build compiles PyTorch from source** — expect hours. ccache makes
  subsequent builds fast. `MAX_JOBS` is derived from `BUILD_MEM_GB` rather than
  hardcoded — see [Memory-capped builds](#memory-capped-builds) to trade compile
  parallelism against peak RAM.
- **Build state is on NFS** (`/mnt/noir/scratch`) by request; compile I/O is
  slower than local NVMe, mitigated by the ccache mount. Override with
  `BUILD_DIR=/some/local/path ./container/build`.
- The CUDA **devel** toolkit is kept in the final image because FlashInfer
  JIT-compiles kernels for sm_120 at runtime and needs `nvcc`.
- Only **GPU 0** (the RTX PRO 6000) is exposed to the container; the RTX 4060 is
  never used.
- `./container/push` needs `docker login` first (Docker Hub namespace `iphands`). It pushes
  four tags — two moving, two pinned:

  | Tag | Meaning |
  |---|---|
  | `cu1321-sm120` | newest build of this CUDA variant |
  | `cu1321-sm120-vllm0.28.0` | pinned to the vLLM version |
  | `cu1321-sm120-vllm0.28.0-d6d029f` | pinned to vLLM version *and* build commit |
  | `latest` | newest build of anything |

  The versions come from the image's own OCI labels (`ai.vllmcustom.*`, stamped
  by the container/Dockerfile), never re-declared in `./container/push` — so a tag cannot claim a
  version the image doesn't contain. `docker image inspect -f
  '{{json .Config.Labels}}' iphands/vllm-blackwell:latest` shows the full set.
  `PUSH_DRY_RUN=1 ./container/push` prints the tags without publishing. Images built before
  labels existed are rejected with a message rather than pushed under a guessed
  tag.
