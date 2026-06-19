# vllmcustom — source-built, sm120-optimized vLLM

A self-owned vLLM build for the **RTX PRO 6000 Blackwell (sm_120)**. No prebuilt
images: PyTorch, FlashInfer, and vLLM are all compiled from source for
`TORCH_CUDA_ARCH_LIST=12.0` on CUDA 13.

| script   | what it does |
|----------|--------------|
| `./build`| clone/pull pinned source into `$BUILD_DIR/src`, compile, tag image |
| `./run`  | serve a model with the local image (GPU 0 only), OpenAI API on `:8700` |
| `./bench`| client-side TTFT + decode tok/s against `:8700`, logs `bench-results.md` |
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
| FlashInfer | v0.6.12 (**from source**) | `FLASHINFER_CUDA_ARCH_LIST=12.0a` |
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

Attention/MoE backends are runtime flags on `./run`:

```bash
ATTN_BACKEND=flashinfer ./run ./models/vllm/Qwen3.6-27B   # default
ATTN_BACKEND=triton     ./run ./models/vllm/Qwen3.6-27B
MOE_BACKEND=flashinfer_trtllm ./run ./models/vllm/Qwen3.6-27B
EXTRA_ARGS="--max-num-seqs 8" ./run ./models/vllm/Qwen3.6-27B
```

Benchmark each with a distinct label; rows accumulate in `bench-results.md`:

```bash
./bench cu1302-flashinfer
./bench cu1302-triton
```

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
