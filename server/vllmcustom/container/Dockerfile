# syntax=docker/dockerfile:1.7-labs
#
# Source-built, sm120-optimized vLLM for the RTX PRO 6000 Blackwell.
#
# Everything on the LLM-critical path (PyTorch, FlashInfer, vLLM) is compiled
# from source for sm_120 (TORCH_CUDA_ARCH_LIST=12.0). torchvision is pulled as a
# prebuilt wheel from the CUDA-minor-matched index (cu132); torchaudio is built
# from source because no cu132 wheel exists -- and a cu130 one is NOT usable:
# both torchvision and torchaudio compare their compiled CUDA major.minor with
# torch.version.cuda at import and raise on mismatch (learned the hard way:
# "PyTorch has CUDA version 13.2 whereas TorchAudio has CUDA version 13.0").
# transformers imports torchaudio at module load, so that error kills `vllm`.
#
# Version notes (checked 2026-09-11; see README "Default version set"):
#   - vLLM v0.29.0 (tagged 2026-09-08) pins torch 2.13.0 / torchaudio 2.11.0 /
#     torchvision 0.28.0 / flashinfer 0.6.18 -- only flashinfer moved from
#     v0.28.0. main still pins torch 2.13.0 (flashinfer 0.6.18.post1), so torch
#     2.14.0 (released 2026-08-26) is not yet usable with any vLLM.
#   - CUDA 13.2.1 is the newest toolkit torch 2.13.0 officially builds against
#     (its binary matrix is 12.6/12.9/13.0/13.2). 13.3.x exists and the host
#     driver supports it, but no torch/flashinfer/vLLM release has CI on it.
#   - FlashInfer 0.6.18 (both the v0.29.0 pin and main's 0.6.18.post1) added a
#     build-time dependency policy (nvidia-cutlass-dsl) that the jit-cache
#     backend declares; flashinfer-build now installs whatever the tree
#     declares. See the comment in that stage.
#   - main snapshot (VLLM_REF=main in ./build, pinned at 468663a, 2026-09-18;
#     was 1ee4be4, 2026-09-12): same torch 2.13.0 / torchaudio 2.11.0 /
#     torchvision 0.28.0, flashinfer 0.6.18.post1 (cubin wheel published at
#     flashinfer.ai/whl), same requirements/build/cuda.txt, same
#     vllm-flash-attention CMakeLists (the sm120 patch below still matches),
#     same apache-tvm-ffi build range. Versus the release the runtime resolve
#     gains instanttensor, cutlass-dsl 4.7.1, quack 0.6.5, transformers>=5.10.4;
#     1ee4be4 -> 468663a only moves xgrammar to ==0.2.7 -- both checked to
#     resolve against torch 2.13.0 with uv. requirements/cuda.txt is byte-
#     identical between the two, so torch/torchaudio/flashinfer layers are
#     reused. What did change is a SYSTEM dep: vllm#56876 repinned DeepGEMM
#     (built for 12.0) to the vllm-project fork, whose deep_jit submodule needs
#     elfutils' libdw -- see vllm-build and vllm-openai. The new DeepSelect
#     extension is 10.0f-only and configures to an empty target here.
#     Otherwise nothing in this file is snapshot-specific; ./build passes the pins.
#
# Source trees are supplied as named build-contexts (pytorch=, flashinfer=,
# vllm=) by ./build, so the giant repos never enter the default build context.
# ccache + uv caches use BuildKit cache mounts so an incremental change re-links
# in minutes instead of recompiling torch from scratch.

ARG CUDA_VERSION=13.2.1
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

ARG TORCH_CUDA_ARCH_LIST=12.0
ARG TORCH_VERSION=2.13.0
# Pin a clean wheel version (e.g. 2.13.0, not 2.13.0a0+gitXXXX) so vLLM's
# `torch==2.13.0` dependency resolves to our wheel instead of pulling from PyPI.
# Everything here is semantic — it changes the artifact, so it *should* bust the
# cache. Build-speed knobs deliberately live elsewhere; see the RUN below.
ENV TORCH_CUDA_ARCH_LIST=${TORCH_CUDA_ARCH_LIST} \
    PYTORCH_BUILD_VERSION=${TORCH_VERSION} \
    PYTORCH_BUILD_NUMBER=1 \
    USE_CUDA=1 USE_CUDNN=1 USE_CUSPARSELT=1 \
    USE_FLASH_ATTENTION=1 USE_MEM_EFF_ATTENTION=1 \
    BUILD_TEST=0 USE_DISTRIBUTED=1 \
    _GLIBCXX_USE_CXX11_ABI=1

# pytorch source supplied as a build-context (recursive submodules already
# checked out on the host by ./build).
#
# .git is excluded here rather than by a .dockerignore inside the source tree:
# ./build runs `git checkout --force` against those trees, and pytorch's
# .dockerignore is a symlink to .gitignore, which does NOT exclude .git. That put
# 5.8 GB of git objects (72% of the context) into this COPY's cache key, churned
# by the `git fetch` every ./build does — invalidating torch, and with it
# flashinfer and vllm downstream. Nothing here needs git metadata:
# PYTORCH_BUILD_VERSION above is what sets the wheel version.
COPY --from=pytorch --exclude=.git --exclude=**/.git . /src/pytorch
WORKDIR /src/pytorch
# MAX_JOBS/NVCC_THREADS arrive by secret mount, NOT as ARG/ENV. BuildKit excludes
# secret *contents* from the cache key, which is exactly the semantics wanted:
# these knobs change how fast the build runs, never what it produces. As ARG/ENV
# they were part of every layer key, so retuning MAX_JOBS forced a full
# torch+flashinfer+vllm recompile. Please don't "simplify" this back into ENV.
# The -f guard and := defaults keep a bare `docker build` (no --secret) working.
RUN --mount=type=cache,target=/root/.cache/ccache \
    --mount=type=cache,target=/root/.cache/uv \
    --mount=type=secret,id=buildjobs \
    { [ -f /run/secrets/buildjobs ] && . /run/secrets/buildjobs; } ; \
    : "${MAX_JOBS:=8}" "${NVCC_THREADS:=1}" ; \
    export MAX_JOBS NVCC_THREADS CMAKE_BUILD_PARALLEL_LEVEL="$MAX_JOBS" ; \
    uv pip install -r requirements.txt && \
    python setup.py bdist_wheel && \
    mkdir -p /wheels && cp dist/*.whl /wheels/ && \
    uv pip install dist/*.whl && \
    ccache -s

###############################################################################
# Stage: torchaudio-build — torchaudio from source against the source torch
###############################################################################
# Separate from the flashinfer -> vllm chain so it never invalidates those
# layers. Small: one CppExtension/CUDAExtension (rnnt, align, ctc decoder), no
# submodules, no cmake. BUILD_VERSION pins the clean wheel version (setup.py
# would otherwise append +<sha>, which we have no .git for anyway) so vLLM's
# torchaudio==2.11.0 pin resolves to this wheel in vllm-openai.
FROM torch-build AS torchaudio-build

ARG TORCH_CUDA_ARCH_LIST=12.0
ARG TORCHAUDIO_VERSION=2.11.0
ENV TORCH_CUDA_ARCH_LIST=${TORCH_CUDA_ARCH_LIST} \
    BUILD_VERSION=${TORCHAUDIO_VERSION} \
    USE_CUDA=1 BUILD_RNNT=1 BUILD_ALIGN=1 BUILD_CUDA_CTC_DECODER=1 BUILD_CPP_TEST=0

COPY --from=torchaudio --exclude=.git --exclude=**/.git . /src/torchaudio
WORKDIR /src/torchaudio
RUN --mount=type=cache,target=/root/.cache/ccache \
    --mount=type=cache,target=/root/.cache/uv \
    --mount=type=secret,id=buildjobs \
    { [ -f /run/secrets/buildjobs ] && . /run/secrets/buildjobs; } ; \
    : "${MAX_JOBS:=8}" ; export MAX_JOBS ; \
    python setup.py bdist_wheel && \
    mkdir -p /wheels-torchaudio && cp dist/torchaudio-*.whl /wheels-torchaudio/ && \
    ls -la /wheels-torchaudio && \
    uv pip install --no-deps /wheels-torchaudio/*.whl && \
    python -c "import torch, torchaudio; print('torchaudio', torchaudio.__version__, 'cuda', torch.ops._torchaudio.cuda_version())"

###############################################################################
# Stage: flashinfer-build — FlashInfer from source against the source torch
###############################################################################
FROM torch-build AS flashinfer-build

ARG TORCH_CUDA_ARCH_LIST=12.0
ARG CUDA_VERSION
# Must match FLASHINFER_REF in ./build (e.g. v0.6.18 -> 0.6.18); used to pull the
# matching prebuilt flashinfer-cubin wheel instead of rebuilding it from source.
ARG FLASHINFER_VERSION=0.6.18
# 12.0f = sm120 family-conditional kernels (RTX PRO 6000 Blackwell). FlashInfer
# 0.6.x has no setup.py; it builds via a PEP 517 backend (python -m build).
ENV FLASHINFER_CUDA_ARCH_LIST="12.0f" \
    TORCH_CUDA_ARCH_LIST=${TORCH_CUDA_ARCH_LIST}

# flashinfer's own .dockerignore already excludes .git, but it is an untracked
# file in a tree ./build runs `git checkout --force` against — state the
# exclusion here too so it cannot be clobbered. See torch-build for the rationale.
COPY --from=flashinfer --exclude=.git --exclude=**/.git . /src/flashinfer
WORKDIR /src/flashinfer
# Build-speed knobs by secret mount — see torch-build above.
RUN --mount=type=cache,target=/root/.cache/ccache \
    --mount=type=cache,target=/root/.cache/uv \
    --mount=type=secret,id=buildjobs \
    { [ -f /run/secrets/buildjobs ] && . /run/secrets/buildjobs; } ; \
    : "${MAX_JOBS:=8}" "${NVCC_THREADS:=1}" ; \
    export MAX_JOBS NVCC_THREADS CMAKE_BUILD_PARALLEL_LEVEL="$MAX_JOBS" ; \
    uv pip install build "setuptools>=77" "packaging>=24" wheel ninja numpy \
        tqdm requests filelock nvidia-ml-py \
        "apache-tvm-ffi>=0.1.6,!=0.1.8,!=0.1.8.post0,<0.2" && \
    # flashinfer >= 0.6.18 declares extra "provider wheel" build deps in
    # ci/cuda-versions.json (dependency_policy; today nvidia-cutlass-dsl[cu13]
    # >=4.6.2a0), returned by build_utils.get_build_dependency_requirements()
    # from the jit-cache backend's get_requires_for_build_wheel hook. Under
    # --no-isolation `python -m build` only *checks* them, and aborts with
    # "ERROR Unmet dependencies" (hit on the v0.29.0 bump, 2026-09-12). Ask the
    # tree instead of hardcoding, so the next policy entry does not repeat this.
    # CUDA_MAJOR selects the cu13 extra. Tags before 0.6.18 have no build_utils
    # and declare nothing extra: the import fails and the list is empty, and if
    # the helper ever breaks for another reason `python -m build` still stops
    # the build with the same Unmet error. Whatever is installed here stays in
    # this stage; vllm-openai starts from base and installs vLLM's own pin.
    _fi_build_deps=$(CUDA_MAJOR="${CUDA_VERSION%%.*}" python -c 'import build_utils; print(" ".join(build_utils.get_build_dependency_requirements()))' 2>/dev/null || true) && \
    echo "flashinfer provider build deps: ${_fi_build_deps:-<none declared>}" && \
    { [ -z "$_fi_build_deps" ] || uv pip install $_fi_build_deps; } && \
    mkdir -p /wheels && \
    # core flashinfer-python (kernels still JIT-able at runtime as a fallback)
    python -m build --no-isolation --wheel --outdir /wheels . && \
    uv pip install --no-deps /wheels/flashinfer_python-*.whl && \
    # prebuilt cubins; install before jit-cache so the AOT step can reuse them
    # instead of refetching. (vLLM's requirements/cuda.txt pins flashinfer-cubin
    # but its setup.py drops the pin from the wheel, so vllm-openai installs the
    # same wheel again for the runtime venv.) The cubin package is a
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

ARG TORCH_CUDA_ARCH_LIST=12.0
ARG VLLM_VERSION=0.29.0
# VLLM_VERSION_OVERRIDE, *not* SETUPTOOLS_SCM_PRETEND_VERSION_FOR_VLLM. The
# scoped form does not work here: setup.py calls setuptools-scm's get_version()
# with no dist_name (setup.py:1026 in v0.29.0), so there is nothing for the
# _FOR_VLLM suffix to match and the lookup fails — verified directly against
# setuptools-scm 10.2.1 / vcs-versioning 2.2.4. It silently appeared to work only
# while .git was in the build context, because setuptools-scm was reading the
# git tags instead. VLLM_VERSION_OVERRIDE is vLLM's own hook (setup.py:1020 in
# v0.29.0, :1261 at main 468663a -- same early return) and sets the *unscoped*
# SETUPTOOLS_SCM_PRETEND_VERSION internally.
ENV TORCH_CUDA_ARCH_LIST=${TORCH_CUDA_ARCH_LIST} \
    VLLM_USE_PRECOMPILED=0 \
    VLLM_VERSION_OVERRIDE=${VLLM_VERSION}

# elfutils headers for DeepGEMM. Since vllm#56876 (2026-09-15) vLLM pins the
# vllm-project/DeepGEMM fork, whose third-party/deep_jit submodule does
# `#include <elfutils/libdwfl.h>` (deep_jit/utils/exception.hpp, pulled in by
# csrc/python_api.cpp). DeepGEMM's arch list includes 12.0, and its _C target is
# `ALL`, not optional -- without the header the vLLM build fails, late. Upstream
# added `elfutils-devel` to its manylinux image in the same commit; libdw-dev is
# the noble package that owns that header (and depends on libelf-dev).
# Installed HERE rather than in base on purpose: base is the parent of every
# compile stage, so touching its apt list would recompile torch and flashinfer
# for a header only this stage reads. Harmless for v0.29.0, which never looks.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends libdw-dev && \
    test -f /usr/include/elfutils/libdwfl.h

# .git excluded (see torch-build). vLLM is the case where that matters most —
# setuptools-scm derives the version from git tags — so VLLM_VERSION_OVERRIDE
# above is load-bearing, not belt-and-braces: without it this build fails with
# "setuptools-scm was unable to detect version for /src/vllm".
# It also yields a clean 0.29.0 rather than the 0.29.0.dev0+g<sha>.d<date> that
# git derivation produced (use_existing_torch.py edits pyproject.toml before
# setup.py runs, so the tree is always dirty by then). Snapshot builds pass
# <next release>.dev<date>+g<sha7> here (./build derives it), which is the only place
# the vLLM commit survives into the image besides the ai.vllmcustom.vllm.ref label.
# Verify with `vllm.__version__` after changing either.
COPY --from=vllm --exclude=.git --exclude=**/.git . /src/vllm
WORKDIR /src/vllm
# vLLM's CMake fetches vllm-flash-attention (FetchContent, pinned sha) and builds
# it with its OWN arch rules, ignoring TORCH_CUDA_ARCH_LIST: FA2 as sm_80+PTX
# (PTX-JIT'd on sm120, never native) and FA3 as 192 Hopper-only kernels that,
# with no sm90 target, get nvcc's default sm_75 -- 818 MB that cannot run here.
# Hook a PATCH_COMMAND into the FetchContent declaration so the clone is edited
# in place: FA2 -> the requested archs, FA3 -> off. Rationale and the exact
# edits live in the patch script. The grep asserts the hook landed on the git
# branch of the declaration (the VLLM_FLASH_ATTN_SRC_DIR branch has no clone to
# patch), so a vLLM bump that rewrites this file fails here, not 40 min later.
COPY patches/vllm-flash-attn-arch.py /opt/patches/vllm-flash-attn-arch.py
RUN f=cmake/external_projects/vllm_flash_attn.cmake && \
    sed -i '/GIT_PROGRESS TRUE/a\          PATCH_COMMAND /opt/venv/bin/python /opt/patches/vllm-flash-attn-arch.py' "$f" && \
    [ "$(grep -c 'PATCH_COMMAND /opt/venv/bin/python /opt/patches/vllm-flash-attn-arch.py' "$f")" = 1 ] && \
    grep -A4 'GIT_REPOSITORY https://github.com/vllm-project/flash-attention.git' "$f"
# Don't let vLLM's pinned torch wheel clobber our source build.
# v0.23.0 replaced requirements/build.txt with a requirements/build/<platform>.txt
# split; use the cuda list. use_existing_torch.py globs requirements/**/*.txt, so it
# still strips the torch==2.13.0 pin from it, leaving our source torch in place.
RUN --mount=type=cache,target=/root/.cache/ccache \
    --mount=type=cache,target=/root/.cache/uv \
    --mount=type=secret,id=buildjobs \
    { [ -f /run/secrets/buildjobs ] && . /run/secrets/buildjobs; } ; \
    : "${MAX_JOBS:=8}" "${NVCC_THREADS:=1}" ; \
    export MAX_JOBS NVCC_THREADS CMAKE_BUILD_PARALLEL_LEVEL="$MAX_JOBS" ; \
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
# torchvision publishes a wheel per CUDA minor (cu130, cu132, ...); ./build
# derives the index from CUDA_VERSION and preflight checks the wheel exists. The
# wheel's CUDA minor MUST equal torch's: torchvision/extension.py compares them
# at import and raises. (torchaudio has the same check and no cu132 wheel, which
# is why it is built from source in torchaudio-build instead.)
ARG TORCHVISION_INDEX=cu132

# Declared before the RUN because it is used in it (unlike the label-only ARGs
# below). No cache cost: a flashinfer bump already changes the wheels this layer
# installs, so the layer is rebuilt either way.
ARG FLASHINFER_VERSION

# Runtime half of the DeepGEMM/deep_jit elfutils dependency (see vllm-build):
# deep_jit dlopen()s "libdw.so.1" to symbolize the stack trace it attaches to a
# kernel/JIT exception. A failed dlopen is tolerated (frames print as ??:0), so
# this is not needed to serve -- it is what makes a DeepGEMM failure readable.
# This stage starts from base, not vllm-build, so the -dev install there does
# not reach it. Before the wheel COPY so a vLLM bump does not redo it.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends libdw1 && \
    python -c "import ctypes; ctypes.CDLL('libdw.so.1')"

# All locally-built wheels (torch, flashinfer, vllm, torchaudio). Installing
# them in one resolve lets vLLM's torch==/torchaudio== pins land on our wheels
# instead of PyPI's cu130 builds.
COPY --from=vllm-build /wheels /wheels
COPY --from=torchaudio-build /wheels-torchaudio /wheels

RUN --mount=type=cache,target=/root/.cache/uv \
    uv pip install /wheels/*.whl && \
    # flashinfer-cubin: pinned in vLLM's requirements/cuda.txt, but setup.py
    # strips it from the wheel's Requires-Dist (it left PyPI after 0.6.13), so
    # the resolve above never installs it and flashinfer would instead download
    # cubins into ~/.cache/flashinfer at first use, at serve time. Pull the same
    # prebuilt wheel flashinfer-build uses, from the same index.
    uv pip install --no-deps --extra-index-url https://flashinfer.ai/whl/ \
        "flashinfer-cubin==${FLASHINFER_VERSION}" && \
    { uv pip install --no-deps --index-url https://download.pytorch.org/whl/${TORCHVISION_INDEX} \
        torchvision==${TORCHVISION_VERSION} || \
      echo "WARN: torchvision ${TORCHVISION_VERSION} ${TORCHVISION_INDEX} wheel unavailable; multimodal disabled"; } && \
    rm -rf /wheels && \
    # Fail the build here, not at `vllm serve`, if any CUDA-minor check trips,
    # and confirm the cubin package is importable (flashinfer.jit.env checks
    # for the flashinfer_cubin module, not the dist name).
    python -c "import torch, torchvision, torchaudio, flashinfer_cubin; \
               print('torch', torch.__version__, torch.version.cuda, \
                     'torchvision', torchvision.__version__, \
                     'torchaudio', torchaudio.__version__, \
                     'flashinfer-cubin', flashinfer_cubin.__file__)"

ENV VLLM_HOST_IP=0.0.0.0 \
    HF_HUB_ENABLE_HF_TRANSFER=1 \
    DO_NOT_TRACK=1

# Stamp the version set into the image so ./push can derive its tags by reading
# the image rather than re-declaring the versions (which is how ./push's
# CUDA_VERSION drifted from ./build's). Declared here, *after* the expensive RUN,
# so a version change only rebuilds this metadata layer.
#
# ARG re-declaration is required per stage; these inherit the values ./build
# already passes with --build-arg.
ARG CUDA_VERSION
ARG TORCH_VERSION
ARG VLLM_VERSION
# The git ref vLLM was built from: a tag (v0.29.0) or, for a snapshot, the full
# commit sha. The version label alone cannot say that for a release build.
ARG VLLM_REF
ARG TORCH_CUDA_ARCH_LIST
LABEL org.opencontainers.image.title="vllm-blackwell" \
      org.opencontainers.image.description="Source-built sm120-optimized vLLM (RTX PRO 6000 Blackwell)" \
      org.opencontainers.image.version="${VLLM_VERSION}" \
      ai.vllmcustom.vllm.version="${VLLM_VERSION}" \
      ai.vllmcustom.vllm.ref="${VLLM_REF}" \
      ai.vllmcustom.torch.version="${TORCH_VERSION}" \
      ai.vllmcustom.flashinfer.version="${FLASHINFER_VERSION}" \
      ai.vllmcustom.cuda.version="${CUDA_VERSION}" \
      ai.vllmcustom.cuda.arch="${TORCH_CUDA_ARCH_LIST}"

EXPOSE 8799
# Use `vllm serve`, NOT `python -m vllm.entrypoints.openai.api_server`. Only the
# `vllm serve` CLI copies the positional model_tag onto the `model` field
# (entrypoints/cli/serve.py); the bare api_server module does not. With the module
# entrypoint, `model` stays at vLLM's default (Qwen/Qwen3-0.6B), so speculative
# decoding builds its draft from that default instead of the served checkpoint —
# breaking MTP with "Unsupported speculative method: 'mtp'".
ENTRYPOINT ["vllm", "serve"]
