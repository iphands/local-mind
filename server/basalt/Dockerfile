# syntax=docker/dockerfile:1
# check=skip=InvalidDefaultArgInFrom
#
# Basalt, pinned, for sm_120 on CUDA 13.4.3. Built by ./build with the build context vendor/basalt
# (checked out detached at BASALT_SHA; Dockerfile.dockerignore beside this file keeps .git and
# build output out). Every ARG below is passed by ./build from versions.env — the defaults are
# deliberately empty so a bare `docker build` fails instead of building something unpinned. That is
# exactly what InvalidDefaultArgInFrom warns about, hence the skip above.
#
# Layout: `basalt serve` chdirs into the source dir compiled into the binary (BASALT_SOURCE_DIR =
# CMAKE_SOURCE_DIR) and runs `python -m serve.server` there, with <source>/.venv/bin/python as the
# default interpreter; the default expert profile is <source>/data/profiles/; serve/ imports
# tools/model_file.py through sys.path (serve/engine/modelfile.py). So the build runs in /opt/basalt
# and the runtime stage puts bin/, serve/, tools/, data/ and .venv/ back at /opt/basalt — what
# upstream's docs/building.md says an install or container image ships.
# (context/plans/01_flashnext_container.md § Why a source-built two-stage image.)

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
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/basalt
COPY . .

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
# A global ARG reaches only FROM lines; redeclared (no default) to inherit its value here.
ARG CUDA_RUNTIME_IMAGE
LABEL ai.basalt.ref="${BASALT_SHA}" \
      ai.basalt.repo="${BASALT_REPO}" \
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
