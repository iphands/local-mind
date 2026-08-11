# syntax=docker/dockerfile:1.7-labs
#
# Source-built, sm120-optimized vLLM for the RTX PRO 6000 Blackwell.
#
# Everything on the LLM-critical path (PyTorch, FlashInfer, vLLM) is compiled
# from source for sm_120 (TORCH_CUDA_ARCH_LIST=12.0). torchvision/torchaudio are
# pulled as cu130 wheels with --no-deps (not perf critical; only needed so vLLM
# imports cleanly).
#
# Source trees are supplied as named build-contexts (pytorch=, flashinfer=,
# vllm=) by ./build, so the giant repos never enter the default build context.
# ccache + uv caches use BuildKit cache mounts so an incremental change re-links
# in minutes instead of recompiling torch from scratch.

ARG CUDA_VERSION=13.0.3
ARG BASE_IMAGE=nvidia/cuda:${CUDA_VERSION}-cudnn-devel-ubuntu24.04

###############################################################################
# Stage: base — toolchain + python, shared by every build stage
###############################################################################
FROM ${BASE_IMAGE} AS base

ENV DEBIAN_FRONTEND=noninteractive \
    PIP_BREAK_SYSTEM_PACKAGES=1 \
    PATH=/usr/local/cuda/bin:/usr/lib/ccache:${PATH} \
    CUDA_HOME=/usr/local/cuda \
    CUDA_TOOLKIT_ROOT_DIR=/usr/local/cuda \
    CUDACXX=/usr/local/cuda/bin/nvcc \
    LD_LIBRARY_PATH=/usr/local/cuda/lib64 \
    CMAKE_C_COMPILER_LAUNCHER=ccache \
    CMAKE_CXX_COMPILER_LAUNCHER=ccache \
    CMAKE_CUDA_COMPILER_LAUNCHER=ccache \
    CCACHE_DIR=/root/.cache/ccache \
    CCACHE_MAXSIZE=40G \
    UV_LINK_MODE=copy

# Ubuntu 24.04 (noble) ships Python 3.12 natively — no PPA needed.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates python3 python3-dev python3-venv \
        git curl wget build-essential cmake ninja-build ccache \
        libopenmpi-dev libnuma-dev libssl-dev pkg-config && \
    curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR=/usr/local/bin sh
# NOTE: no nvcc symlink in /usr/lib/ccache — it would make CMake's FindCUDA infer
# the toolkit root as /usr/lib. nvcc is cached via CMAKE_CUDA_COMPILER_LAUNCHER=ccache.

# A single venv reused across stages (copied forward via /opt/venv).
ENV VIRTUAL_ENV=/opt/venv PATH=/opt/venv/bin:/usr/local/cuda/bin:/usr/lib/ccache:${PATH}
RUN uv venv --python python3 ${VIRTUAL_ENV} && \
    uv pip install --python ${VIRTUAL_ENV}/bin/python \
        "setuptools>=77" wheel "cmake>=3.26" ninja pybind11 "numpy<2.3" pyyaml typing_extensions

###############################################################################
# Stage: torch-build — PyTorch from source for sm_120
###############################################################################
FROM base AS torch-build

ARG MAX_JOBS=18
ARG NVCC_THREADS=8
ARG TORCH_CUDA_ARCH_LIST=12.0
ARG TORCH_VERSION=2.13.0
# Pin a clean wheel version (e.g. 2.13.0, not 2.13.0a0+gitXXXX) so vLLM's
# `torch==2.13.0` dependency resolves to our wheel instead of pulling from PyPI.
ENV TORCH_CUDA_ARCH_LIST=${TORCH_CUDA_ARCH_LIST} \
    PYTORCH_BUILD_VERSION=${TORCH_VERSION} \
    PYTORCH_BUILD_NUMBER=1 \
    MAX_JOBS=${MAX_JOBS} \
    NVCC_THREADS=${NVCC_THREADS} \
    USE_CUDA=1 USE_CUDNN=1 USE_CUSPARSELT=1 \
    USE_FLASH_ATTENTION=1 USE_MEM_EFF_ATTENTION=1 \
    BUILD_TEST=0 USE_DISTRIBUTED=1 \
    CMAKE_BUILD_PARALLEL_LEVEL=${MAX_JOBS} \
    _GLIBCXX_USE_CXX11_ABI=1

# pytorch source supplied as a build-context (recursive submodules already
# checked out on the host by ./build).
COPY --from=pytorch . /src/pytorch
WORKDIR /src/pytorch
RUN --mount=type=cache,target=/root/.cache/ccache \
    --mount=type=cache,target=/root/.cache/uv \
    uv pip install -r requirements.txt && \
    python setup.py bdist_wheel && \
    mkdir -p /wheels && cp dist/*.whl /wheels/ && \
    uv pip install dist/*.whl && \
    ccache -s

###############################################################################
# Stage: flashinfer-build — FlashInfer from source against the source torch
###############################################################################
FROM torch-build AS flashinfer-build

ARG MAX_JOBS=18
ARG NVCC_THREADS=8
ARG TORCH_CUDA_ARCH_LIST=12.0
# Must match FLASHINFER_REF in ./build (e.g. v0.6.16.post3 -> 0.6.16.post3); used to
# pull the matching prebuilt flashinfer-cubin wheel instead of rebuilding it from source.
ARG FLASHINFER_VERSION=0.6.16.post3
# 12.0f = sm120 family-conditional kernels (RTX PRO 6000 Blackwell). FlashInfer
# 0.6.x has no setup.py; it builds via a PEP 517 backend (python -m build).
ENV FLASHINFER_CUDA_ARCH_LIST="12.0f" \
    TORCH_CUDA_ARCH_LIST=${TORCH_CUDA_ARCH_LIST} \
    MAX_JOBS=${MAX_JOBS} \
    NVCC_THREADS=${NVCC_THREADS} \
    CMAKE_BUILD_PARALLEL_LEVEL=${MAX_JOBS}

COPY --from=flashinfer . /src/flashinfer
WORKDIR /src/flashinfer
RUN --mount=type=cache,target=/root/.cache/ccache \
    --mount=type=cache,target=/root/.cache/uv \
    uv pip install build "setuptools>=77" "packaging>=24" wheel ninja numpy \
        tqdm requests filelock nvidia-ml-py \
        "apache-tvm-ffi>=0.1.6,!=0.1.8,!=0.1.8.post0,<0.2" && \
    mkdir -p /wheels && \
    # core flashinfer-python (kernels still JIT-able at runtime as a fallback)
    python -m build --no-isolation --wheel --outdir /wheels . && \
    uv pip install --no-deps /wheels/flashinfer_python-*.whl && \
    # prebuilt cubins (vLLM pins flashinfer-cubin); install before jit-cache so the
    # AOT step can reuse them instead of refetching. The cubin package is a
    # py3-none-any blob of kernels NVIDIA already compiled — nothing to build for
    # sm120 — so we pull the prebuilt wheel instead of rebuilding it. Building it
    # from source scrapes ~15.6k files off NVIDIA's flaky edge.urm.nvidia.com CDN
    # all-or-nothing, and a single failed file aborts the whole image build.
    # flashinfer-cubin stopped publishing to PyPI after 0.6.13, so it comes from
    # flashinfer.ai/whl — the same --extra-index-url vLLM uses in requirements/cuda.txt.
    uv pip install --no-deps --extra-index-url https://flashinfer.ai/whl/ \
        "flashinfer-cubin==${FLASHINFER_VERSION}" && \
    # AOT-precompiled sm120 kernels (the heavy compile; honors FLASHINFER_CUDA_ARCH_LIST)
    python -m build --no-isolation --wheel --outdir /wheels ./flashinfer-jit-cache && \
    uv pip install --no-deps /wheels/flashinfer_jit_cache-*.whl && \
    ccache -s

###############################################################################
# Stage: vllm-build — vLLM from source against source torch + flashinfer
###############################################################################
FROM flashinfer-build AS vllm-build

ARG MAX_JOBS=18
ARG NVCC_THREADS=8
ARG TORCH_CUDA_ARCH_LIST=12.0
ARG VLLM_VERSION=0.27.0
ENV TORCH_CUDA_ARCH_LIST=${TORCH_CUDA_ARCH_LIST} \
    MAX_JOBS=${MAX_JOBS} \
    NVCC_THREADS=${NVCC_THREADS} \
    VLLM_USE_PRECOMPILED=0 \
    CMAKE_BUILD_PARALLEL_LEVEL=${MAX_JOBS} \
    SETUPTOOLS_SCM_PRETEND_VERSION_FOR_VLLM=${VLLM_VERSION}

COPY --from=vllm . /src/vllm
WORKDIR /src/vllm
# Don't let vLLM's pinned torch wheel clobber our source build.
# v0.23.0 replaced requirements/build.txt with a requirements/build/<platform>.txt
# split; use the cuda list. use_existing_torch.py globs requirements/**/*.txt, so it
# still strips the torch==2.13.0 pin from it, leaving our source torch in place.
RUN --mount=type=cache,target=/root/.cache/ccache \
    --mount=type=cache,target=/root/.cache/uv \
    python use_existing_torch.py 2>/dev/null || true && \
    uv pip install -r requirements/build/cuda.txt && \
    python setup.py bdist_wheel && \
    mkdir -p /wheels && cp dist/*.whl /wheels/ && \
    ccache -s

###############################################################################
# Stage: vllm-openai — runtime image (keeps CUDA devel toolkit for flashinfer JIT)
###############################################################################
FROM base AS vllm-openai

ARG TORCHVISION_VERSION=0.28.0
# Not a stale pin: vLLM's requirements/cuda.txt really does pair torchaudio 2.11.0
# with torch 2.13.0 / torchvision 0.28.0. Keep it in step with upstream, not with torch.
ARG TORCHAUDIO_VERSION=2.11.0

# All locally-built wheels (torch, flashinfer, vllm).
COPY --from=vllm-build /wheels /wheels

RUN --mount=type=cache,target=/root/.cache/uv \
    uv pip install /wheels/*.whl && \
    { uv pip install --no-deps --index-url https://download.pytorch.org/whl/cu130 \
        torchvision==${TORCHVISION_VERSION} torchaudio==${TORCHAUDIO_VERSION} || \
      echo "WARN: torchvision/torchaudio cu130 wheels unavailable; multimodal disabled"; } && \
    rm -rf /wheels

ENV VLLM_HOST_IP=0.0.0.0 \
    HF_HUB_ENABLE_HF_TRANSFER=1 \
    DO_NOT_TRACK=1

EXPOSE 8799
# Use `vllm serve`, NOT `python -m vllm.entrypoints.openai.api_server`. Only the
# `vllm serve` CLI copies the positional model_tag onto the `model` field
# (entrypoints/cli/serve.py); the bare api_server module does not. With the module
# entrypoint, `model` stays at vLLM's default (Qwen/Qwen3-0.6B), so speculative
# decoding builds its draft from that default instead of the served checkpoint —
# breaking MTP with "Unsupported speculative method: 'mtp'".
ENTRYPOINT ["vllm", "serve"]
