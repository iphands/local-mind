# syntax=docker/dockerfile:1
# check=skip=InvalidDefaultArgInFrom
#
# Basalt, pinned, for sm_120 on CUDA 13.4.3. Built by ./build with the build context vendor/basalt
# (checked out detached at BASALT_SHA; Dockerfile.dockerignore beside this file keeps .git and
# build output out) and a second, named context `patches` (our patches/ directory). Every ARG below is passed by ./build from versions.env — the defaults are
# deliberately empty so a bare `docker build` fails instead of building something unpinned. That is
# exactly what InvalidDefaultArgInFrom warns about, hence the skip above.
#
# Layout: the binary takes the directory above its own bin/ as its install root (since 0.3.2; the
# compiled-in source path is gone). `basalt serve` runs `python -m serve.server` with
# <root>/.venv/bin/python; the expert profiles are <root>/data/profiles/; serve/ imports
# tools/model_file.py through sys.path (serve/engine/modelfile.py). So the build runs in /opt/basalt
# and the runtime stage puts bin/, serve/, tools/, data/ and .venv/ back at /opt/basalt — the same
# tree upstream's own Dockerfile ships. That file sits in the build context since 0.3.3 and is not
# used: -f names this one.
# (context/plans/completed/01_flashnext_container.md § Why a source-built two-stage image.)

ARG CUDA_DEVEL_IMAGE=
ARG CUDA_RUNTIME_IMAGE=

FROM ${CUDA_DEVEL_IMAGE} AS build

ARG DEBIAN_FRONTEND=noninteractive
# Ubuntu 24.04 candidates on 2026-11-04: cmake 3.28.3 (>= 3.24), g++ 13.2.0 (>= 13), ninja 1.11.1.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        cmake \
        g++ \
        ninja-build \
        patch \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/basalt
COPY . .

# Our local patches (../../patches/*.patch, Plan 12), applied in name order from the named build
# context `patches` that ./build passes. --fuzz=0: a patch that no longer fits the pin fails the build
# here instead of landing somewhere near. vendor/basalt itself is never edited.
RUN --mount=type=bind,from=patches,target=/patches \
    set -e; for p in /patches/*.patch; do \
        [ -e "$p" ] || continue; \
        echo "== applying ${p##*/}"; \
        patch -p1 --fuzz=0 --no-backup-if-mismatch < "$p"; \
    done

# CMAKE_CUDA_ARCHITECTURES is left to upstream's default (120): the only arch Basalt tests. The CPU
# ISA rungs (AVX2 / AVX-VNNI / AVX-512) are compiled per file and picked at run time, so no -march.
ARG JOBS=16
RUN cmake -S /opt/basalt -B /opt/basalt/build -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DBUILD_TESTING=OFF \
    && cmake --build /opt/basalt/build --parallel "${JOBS}" \
    && ls -l /opt/basalt/bin

FROM ${CUDA_RUNTIME_IMAGE}

ARG DEBIAN_FRONTEND=noninteractive
# Ubuntu 24.04 candidate on 2026-11-04: python3 3.12.3 (>= 3.10).
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        ca-certificates \
        python3 \
        python3-venv \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /opt/basalt/bin /opt/basalt/bin
COPY --from=build /opt/basalt/serve /opt/basalt/serve
COPY --from=build /opt/basalt/tools /opt/basalt/tools
COPY --from=build /opt/basalt/data /opt/basalt/data
COPY --from=build /opt/basalt/requirements.txt /opt/basalt/LICENSE /opt/basalt/THIRD_PARTY_LICENSES /opt/basalt/

# upstream's pinned server dependencies, in the venv `basalt serve` looks for first
RUN python3 -m venv /opt/basalt/.venv \
    && /opt/basalt/.venv/bin/pip install --no-cache-dir --disable-pip-version-check \
        -r /opt/basalt/requirements.txt

ARG BASALT_REPO=
ARG BASALT_SHA=
ARG CUDA_VERSION_TAG=
# "<file>:<sha256, 12 hex>" per applied patch, or "none" (./build computes it).
ARG BASALT_PATCHES=
# A global ARG reaches only FROM lines; redeclared (no default) to inherit its value here.
ARG CUDA_RUNTIME_IMAGE
LABEL ai.basalt.ref="${BASALT_SHA}" \
      ai.basalt.repo="${BASALT_REPO}" \
      ai.basalt.patches="${BASALT_PATCHES}" \
      ai.basalt.cuda="${CUDA_VERSION_TAG}" \
      ai.basalt.base="${CUDA_RUNTIME_IMAGE}" \
      ai.basalt.arch="sm_120"

ENV PYTHONDONTWRITEBYTECODE=1
WORKDIR /opt/basalt
EXPOSE 8700
STOPSIGNAL SIGTERM
# Last, so nothing above runs as it. 1000 is `ubuntu` in the Ubuntu 24.04 base; ./run passes
# HOME=/home/ubuntu (Basalt's cache folder is ~/.cache/basalt/).
USER 1000:1000
ENTRYPOINT ["/opt/basalt/bin/basalt"]
CMD ["--help"]
