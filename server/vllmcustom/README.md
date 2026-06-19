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
| `./push` | `docker push` to `docker.io/iphands/vllm-blackwell` |

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

vLLM `v0.23.0` pins these, so they are the defaults:

| component  | ref/version | arch flags |
|------------|-------------|------------|
| CUDA       | 13.0.2 (`cudnn-devel-ubuntu24.04`) | — |
| PyTorch    | v2.11.0 (**from source**) | `TORCH_CUDA_ARCH_LIST=12.0` |
| FlashInfer | v0.6.12 (**from source**) | `FLASHINFER_CUDA_ARCH_LIST=12.0f` |
| vLLM       | v0.23.0 (**from source**) | `TORCH_CUDA_ARCH_LIST=12.0`, `VLLM_USE_PRECOMPILED=0` |
| torchvision/torchaudio | 0.26.0 / 2.11.0 (cu130 wheels, `--no-deps`) | not perf-critical |

## Trying other CUDA / versions

Everything is a build-arg; the CUDA version becomes part of the tag so variants
coexist:

```bash
CUDA_VERSION=13.1.2 ./build               # -> iphands/vllm-blackwell:cu1312-sm120
VLLM_REF=v0.22.1 TORCH_REF=v2.11.0 FLASHINFER_REF=v0.6.11 ./build
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
MOE_BACKEND=flashinfer_trtllm ./run ./models/vllm/Qwen3.6-27B
NVFP4_BACKEND=cutlass   ./run ./models/vllm/Qwen3.5-122B-A10B-NVFP4   # NVFP4 GEMM kernel
SPEC_TOKENS=3           ./run ./models/vllm/Qwen3.5-122B-A10B-NVFP4   # MTP tokens (default 2)
EXTRA_ARGS="--max-num-seqs 8" ./run ./models/vllm/Qwen3.6-27B
```

Benchmark each with a distinct label; rows accumulate in `bench-results.md`:

```bash
./bench cu1302-flashinfer
./bench cu1302-cutlass-mtp2
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
- `NVFP4_BACKEND` — vLLM's NVFP4 GEMM kernel. `cutlass` (internal sm120f) is the
  fastest per the wiki; `flashinfer-cudnn` is the *safest* (sidesteps a FlashInfer
  CUTLASS FP4 race condition that silently NaNs); `marlin` is a W4A16 fallback if FP4
  GEMM ever produces garbage. Empty = vLLM auto.
- `SPEC_TOKENS` — MTP=2 is the safe sweet spot; MTP=3 *may* add a bit for single
  streams (its instability is a long-context/high-concurrency problem). Watch the logs
  for `probability tensor contains inf/nan` → back off to `flashinfer-cudnn` or MTP=2.

`VLLM_LOG_STATS_INTERVAL=1` is on by default so each run prints live tok/s + MTP
acceptance for comparison.

**Automated sweep:** `./bench-wrapper` runs the whole matrix for you — it restarts
vLLM per config, warms up (mandelbrot prompt), measures (primes prompt), and prints a
table of **CAP(W)** / TTFT / prefill / **TG (token-gen) tok/s**, appending to
`bench-wrapper-results.md`. It also writes a long-term `bench-results.jsonl` (one
record per run, written by the Python measurement) that includes the **GPU0 power cap**
(read — never set — via `nvidia-smi`), so you can correlate tok/s with the wattage you
had set. Each config is a full 122B reload (minutes), so the default 6-config sweep is
long; trim via the `CONFIGS` env. Example:

```bash
./bench-wrapper /mnt/noir/scratch/ai/llm/models/vllm/Qwen3.5-122B-A10B-NVFP4
RUNS=3 ./bench-wrapper            # 3 measured runs/config, median TG
```

### Host tuning (outside the container)
- `sudo nvidia-smi -pm 1` then set max power limit (`sudo nvidia-smi -pl <max>`).
  Note: single-stream **decode is memory-bandwidth-bound and memory clock is
  power-limit-invariant** on these SKUs, so this mostly helps prefill/TTFT — don't
  expect decode gains.
- CPU: `governor=performance`, `vm.swappiness=0`, `kernel.numa_balancing=0`.
- GRUB `pcie_aspm=off pcie_port_pm=off` — avoids PCIe "Surprise Link Down" lockups
  under sustained load.

### Version notes (Tier 3, investigated)
Staying on the pinned set is fine for decode: flashinfer 0.6.12 already includes the
sm120f FP4 module (PR #2650), and vLLM v0.23.0 already includes the Qwen3.5 MTP fix
(#35581). A vLLM bump would only add fixes merged after 2026-06-14 — not worth the
full rebuild + torch/flashinfer compat risk unless a benchmark exposes a problem.

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

## Notes / caveats

- **First build compiles PyTorch from source** — expect hours. ccache makes
  subsequent builds fast. Tune `MAX_JOBS` / `NVCC_THREADS` for your box.
- **Build state is on NFS** (`/mnt/noir/scratch`) by request; compile I/O is
  slower than local NVMe, mitigated by the ccache mount. Override with
  `BUILD_DIR=/some/local/path ./build`.
- The CUDA **devel** toolkit is kept in the final image because FlashInfer
  JIT-compiles kernels for sm_120 at runtime and needs `nvcc`.
- Only **GPU 0** (the RTX PRO 6000) is exposed to the container; the RTX 4060 is
  never used.
- `./push` needs `docker login` first (Docker Hub namespace `iphands`).
