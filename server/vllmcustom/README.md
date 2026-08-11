# vllmcustom — source-built, sm120-optimized vLLM

A self-owned vLLM build for the **RTX PRO 6000 Blackwell (sm_120)**. No prebuilt
images: PyTorch, FlashInfer, and vLLM are all compiled from source for
`TORCH_CUDA_ARCH_LIST=12.0` on CUDA 13.

| script   | what it does |
|----------|--------------|
| `./build`| clone/pull pinned source into `$BUILD_DIR/src`, compile, tag image |
| `./run`  | serve a model with the local image (GPU 0 only), OpenAI API on `:8700` |
| `./bench`| client-side TTFT + decode tok/s against `:8700`, logs `bench-results.md` |
| `./bench-wrapper`| sweep NVFP4-backend × MTP configs: start/stop vLLM per config, warmup, measure, print table |
| `./bench-context`| large-context decode test: per backend, 3-turn convo + padded probes (8k–128k), TG-vs-depth |
| `./make-test-convo`| build the static ~100k-token "summarize Quake II source" turn from `vendor/yquake2` into `data/test-convo.json` |
| `./push` | `docker push` to `docker.io/iphands/vllm-blackwell` |
| `lib/`   | the shared measurement client (`benchclient.py`) + the Python drivers the bench scripts invoke (`measure.py`, `ctxbench.py`) |

## Quick start

```bash
./build                                   # first build compiles torch from source (slow; see below)
./run ./models/vllm/Qwen3.6-27B           # serves on http://localhost:8700/v1
./bench baseline                          # measure tok/s of the running server
```

`models` is a symlink to `/mnt/noir/scratch/ai/llm/models`, so paths match the
old `server/vllm` scripts:

```bash
./run ./models/vllm/Qwen3.5-122B-A10B-NVFP4
```

## Default version set (mutually compatible)

vLLM `v0.27.0` pins these (see its `requirements/cuda.txt`), so they are the defaults:

| component  | ref/version | arch flags |
|------------|-------------|------------|
| CUDA       | 13.0.3 (`cudnn-devel-ubuntu24.04`) | — |
| PyTorch    | v2.13.0 (**from source**) | `TORCH_CUDA_ARCH_LIST=12.0` |
| FlashInfer | v0.6.16.post3 (**from source**) | `FLASHINFER_CUDA_ARCH_LIST=12.0f` |
| vLLM       | v0.27.0 (**from source**) | `TORCH_CUDA_ARCH_LIST=12.0`, `VLLM_USE_PRECOMPILED=0` |
| torchvision/torchaudio | 0.28.0 / 2.11.0 (cu130 wheels, `--no-deps`) | not perf-critical |

torchaudio 2.11.0 alongside torch 2.13.0 is not a typo — that is upstream's own pairing.
`flashinfer-cubin` stopped publishing to PyPI after 0.6.13, so the build pulls it from
`--extra-index-url https://flashinfer.ai/whl/` (matching vLLM's `requirements/cuda.txt`).

## Trying other CUDA / versions

Everything is a build-arg; the CUDA version becomes part of the tag so variants
coexist:

```bash
CUDA_VERSION=13.1.2 ./build               # -> iphands/vllm-blackwell:cu1312-sm120
VLLM_REF=v0.25.1 TORCH_REF=v2.11.0 FLASHINFER_REF=v0.6.13 ./build
```

Then run a specific variant and benchmark it:

```bash
IMAGE_TAG=cu1312-sm120 ./run ./models/vllm/Qwen3.6-27B
./bench cu1312-flashinfer
```

## A/B-testing backends (runtime, no rebuild)

Backends and the main perf knobs are runtime env on `./run`:

```bash
ATTN_BACKEND=flashinfer ./run ./models/vllm/Qwen3.6-27B   # default (correct for Qwen3.5)
ATTN_BACKEND=triton     ./run ./models/vllm/Qwen3.6-27B
MOE_BACKEND=flashinfer_b12x ./run ./models/vllm/Qwen3.5-122B-A10B-NVFP4  # SM12x fused MoE (opt-in)
NVFP4_BACKEND=cutlass   ./run ./models/vllm/Qwen3.5-122B-A10B-NVFP4   # NVFP4 GEMM kernel
SPEC_TOKENS=3           ./run ./models/vllm/Qwen3.5-122B-A10B-NVFP4   # MTP tokens (default 2)
CUDAGRAPH_MODE=FULL_AND_PIECEWISE ./run ./models/vllm/Qwen3.5-122B-A10B-NVFP4  # full CUDA graphs
FLASHINFER_AUTOTUNE=1   ./run ./models/vllm/Qwen3.5-122B-A10B-NVFP4   # autotune during warmup
MAX_BATCHED_TOKENS=32768 ./run ./models/vllm/Qwen3.6-27B  # faster long-context prefill (default 8192)
EXTRA_ARGS="--max-num-seqs 8" ./run ./models/vllm/Qwen3.6-27B
```

Benchmark each with a distinct label; rows accumulate in `bench-results.md`:

```bash
./bench cu1303-flashinfer
./bench cu1303-cutlass-mtp2
```

## Performance tuning (sm120, single GPU)

Optimizing **single-stream decode tok/s** on one RTX PRO 6000. Distilled from the
`vendor/rtx6kpro` community wiki (ignore its multi-GPU/NVLink/NCCL/DCP advice — we
have one card).

**Already optimal in `./run` / `./build` (leave alone):**
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

**Automated sweep:** `./bench-wrapper` runs the whole matrix for you — it restarts
vLLM per config, warms up (mandelbrot prompt), measures (primes prompt), and prints a
table of **CAP(W)** / TTFT / prefill / **TG (token-gen) tok/s**, appending to
`bench-wrapper-results.md`. It also writes a long-term `bench-results.jsonl` (one
record per run, written by `lib/measure.py`) that includes the **GPU0 power cap**
(read — never set — via `nvidia-smi`) plus the image tag and vLLM version, so old rows
stay comparable across rebuilds. Config lines are
`<name> <NVFP4_BACKEND|-> <SPEC_TOKENS> [MOE_BACKEND|-]` (the MoE column is optional).
Measured requests set `ignore_eos`, so TG always covers exactly `GEN_TOKENS` tokens.
Each config is a full 122B reload (minutes), so the default 8-config sweep is
long; trim via the `CONFIGS` env. Example:

```bash
./bench-wrapper /mnt/noir/scratch/ai/llm/models/vllm/Qwen3.5-122B-A10B-NVFP4
RUNS=3 ./bench-wrapper            # 3 measured runs/config, median TG
```

**Large-context decode (`./bench-context`):** `bench-wrapper` only exercises ~1k
context, so it shows best-case decode. `bench-context` characterizes **TG vs. context
depth** — the number that actually drops as the KV cache fills — two ways, per backend:
- **convo**: a realistic 3-turn chat — (1) Bash prime sieve, (2) port to Python, (3) a
  static "summarize this Quake II source" turn that injects ~100k tokens of real C code
  (history is resent each turn, exercising prefix caching). Turn 3 is the big-context
  measurement. **Generate that turn first** with `./make-test-convo` (reads
  `vendor/yquake2` → `data/test-convo.json`); if it's missing, bench-context warns and
  skips turn 3.
- **pad**: prompts padded to exact `PAD_SIZES` (default `8192 32768 65536 131072`),
  short generation — a clean, repeatable decode-vs-context curve to compare backends at
  the same depth.

```bash
./make-test-convo                                 # build the ~100k-token quake2 turn (once)
QUAKE_TOKEN_BUDGET=150000 ./make-test-convo       # bigger turn (default 120k budget ≈ 104k real)
./bench-context                                   # ALL permutations (default): {auto,cutlass,cudnn,marlin} × MTP{0,1,2} + b12x
PAD_SIZES="8192 32768" CONFIGS=$'cutlass cutlass 2' ./bench-context   # quick single-backend subset
```

By default `./bench-context` (no args) runs the **full 14-config matrix** — every NVFP4
backend × MTP {0,1,2} plus the SM12x b12x GEMM/MoE opt-ins, each a full reload +
3-turn convo + pad-to-128k. Budget **hours**; trim with `CONFIGS=…` / `PAD_SIZES=…`
for a faster pass. Config lines take the same optional 4th MoE-backend column as
bench-wrapper.

The generator is deterministic (sorted files) so turn 3 is byte-identical across
backends — a fair A/B at the same large context. Its `approx_tokens` is a chars/token
estimate (default 2.3 chars/token, measured on this C corpus — the old 3.6 default
made a 120k budget tokenize to ~190k, so regenerate `data/test-convo.json` if yours
predates that); the **real** depth is the `context_tokens` bench-context reports for
the `quake2` turn — tune `QUAKE_TOKEN_BUDGET` from that.

Rows land in `bench-context-results.md` (tables) and `bench-results.jsonl` (with
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
native `DiffusionGemmaForBlockDiffusion` (unblocking `./run-diffusiongemma`) plus
`LagunaForCausalLM` / `DFlashLagunaForCausalLM`. Decode-wise the earlier pinned set was
already fine — flashinfer has carried the sm120f FP4 module since 0.6.12 (PR #2650).

A further bump is not free: it drags torch and flashinfer with it (0.27.0 moved torch
2.11.0 → 2.13.0), which invalidates the `torch-build` layer and most of ccache, so
budget a full multi-hour rebuild rather than an incremental one. Read the target tag's
`requirements/cuda.txt` first and move `TORCH_REF`/`FLASHINFER_REF` in `./build` to match.

`MuseGlimmer` still has no native implementation as of v0.27.0, so the
`patches/muse-embed-norm/` sitecustomize shim and `run-muse`'s `--hf-overrides` are
both still required.

## Caching — why builds aren't from scratch every time

- **Source** lives in `$BUILD_DIR/src/{pytorch,flashinfer,vllm}` (default
  `/mnt/noir/scratch/ai/vllm/build`). `./build` only `git fetch` + `checkout`s
  the pinned ref — **no re-clone**. Patch or `git checkout` in place to iterate.
- **Compiler cache**: the Dockerfile uses BuildKit `--mount=type=cache` for
  ccache + uv. This is the supported alternative to host bind mounts (Docker
  forbids arbitrary host bind mounts inside `RUN`) and persists across builds on
  the same daemon exactly like a bind mount would — so a small source change
  re-links in minutes instead of recompiling torch from scratch.
- **Runtime caches** (HF downloads, `torch.compile`, FlashInfer JIT) bind-mount
  from `/mnt/noir/scratch/ai/vllm/cache` into the container, so model loads and
  JIT compiles persist between `./run`s.

To inspect/relocate the ccache on disk, export it:
`docker buildx build ... --cache-to type=local,dest=$BUILD_DIR/ccache`.

## Memory-capped builds

The host has 125 GB and **no swap**, so an OOM during compilation isn't a build
failure — the kernel OOM-killer fires against the whole machine. Parallel `nvcc`
is what gets it there: each `cicc` holds 2–6 GB on the heavy sm120 template
kernels, so an unbounded 28-way build can spike past 110 GB.

`./build` is capped at `BUILD_MEM_GB` (default 90G) by two independent layers:

- **Derived `MAX_JOBS`** — `BUILD_MEM_GB / MEM_PER_JOB_GB` (default 5), clamped
  to `nproc - 4`. On this box that's **18** jobs instead of 28. This is the
  prevention layer: no privileges, no setup, just keeps the build off the
  ceiling. Setting `MAX_JOBS` explicitly bypasses the calculation entirely.
- **A cgroup ceiling** — `./build` passes `--cgroup-parent=vllmbuild.slice`, a
  systemd slice with `MemoryHigh=84G` / `MemoryMax=90G`. `High` throttles under
  reclaim pressure; `Max` is the wall, where the OOM killer fires **scoped to
  that cgroup** — it kills an `nvcc`, not your desktop session or a running
  vLLM server. This is containment, not prevention: hitting it still loses the
  build, just not the machine.

Install the slice once, as root:

```bash
su -c "$PWD/scripts/slice-setup"
```

Until you do, `./build` prints a warning and runs with `MAX_JOBS` as the only
limit — it never hard-fails on a missing slice. `docker buildx build` has no
`--memory` flag (that was classic-builder only), so `--cgroup-parent` is the
supported mechanism here; the BuildKit embedded in dockerd does honor it, and
`RUN` steps land in `/sys/fs/cgroup/vllmbuild.slice/buildkit/`.

> **Gotcha:** BuildKit *creates* the cgroup itself, uncapped, if the directory
> doesn't already exist — so pointing `--cgroup-parent` at a unit that was never
> `systemctl start`ed silently gives you no ceiling at all, while `systemctl show`
> still reports the configured 90G from the unit file. That's why `slice-setup`
> starts the slice, and why `./build` verifies live kernel state (`memory.max`)
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
BUILD_MEM_GB=64 su -c "$PWD/scripts/slice-setup"   # move the hard ceiling
BUILD_MEM_GB=64 ./build                            # and the derived MAX_JOBS
```

`MEM_PER_JOB_GB=5` is a starting estimate, not a measurement. After a full build
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
  `BUILD_DIR=/some/local/path ./build`.
- The CUDA **devel** toolkit is kept in the final image because FlashInfer
  JIT-compiles kernels for sm_120 at runtime and needs `nvcc`.
- Only **GPU 0** (the RTX PRO 6000) is exposed to the container; the RTX 4060 is
  never used.
- `./push` needs `docker login` first (Docker Hub namespace `iphands`).
