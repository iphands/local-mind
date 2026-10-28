# syntax=docker/dockerfile:1
# check=skip=InvalidDefaultArgInFrom
#
# NInfer, pinned, for sm_120a on CUDA 13.4.3. Built by ./build with the build context
# vendor/ninfer (checked out detached at NINFER_SHA; its .dockerignore keeps .git out).
# Every ARG below is passed by ./build from versions.env — the defaults are deliberately empty so
# a bare `docker build` fails instead of building something unpinned. That is exactly what
# InvalidDefaultArgInFrom warns about, hence the skip above; a default here would be a second copy
# of a versions.env pin, free to drift.
#
# Differs from upstream's Dockerfile @ 64d10fbf on purpose (context/high_level.md § Image):
# liburing (Dependencies.cmake requires >= 2.5; upstream installs none), CUDA 13.4.3 by digest
# (upstream: 13.1.2), the arch named explicitly, labels to find the image by, and USER 1000:1000.

ARG CUDA_DEVEL_IMAGE=
ARG CUDA_RUNTIME_IMAGE=

FROM ${CUDA_DEVEL_IMAGE} AS build

ARG DEBIAN_FRONTEND=noninteractive
# Ubuntu 24.04 candidates on 2026-10-27: cmake 3.28.3, liburing-dev 2.5, libhwloc-dev 2.10.0,
# libcurl4-openssl-dev 8.5.0, FFmpeg 6.1.1, g++ 13.2.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        cmake \
        g++ \
        libavcodec-dev \
        libavformat-dev \
        libavutil-dev \
        libcurl4-openssl-dev \
        libhwloc-dev \
        liburing-dev \
        libswscale-dev \
        ninja-build \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .

ARG JOBS=16
RUN cmake -S . -B /build -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_CUDA_ARCHITECTURES=120a \
        -DNINFER_BUILD_APPS=ON \
        -DBUILD_TESTING=OFF \
        -DNINFER_BUILD_BENCHMARKS=OFF \
    && cmake --build /build --parallel "${JOBS}" --target ninfer ninfer-serve

FROM ${CUDA_RUNTIME_IMAGE}

ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        ca-certificates \
        libavcodec60 \
        libavformat60 \
        libavutil58 \
        libcurl4t64 \
        libhwloc15 \
        liburing2 \
        libswscale7 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /build/apps/ninfer /usr/local/bin/ninfer
COPY --from=build /build/apps/ninfer-serve /usr/local/bin/ninfer-serve

ARG NINFER_REPO=
ARG NINFER_SHA=
ARG CUDA_VERSION_TAG=
# A global ARG reaches only FROM lines; redeclared (no default) to inherit its value here.
ARG CUDA_RUNTIME_IMAGE
LABEL ai.ninfer.ref="${NINFER_SHA}" \
      ai.ninfer.repo="${NINFER_REPO}" \
      ai.ninfer.cuda="${CUDA_VERSION_TAG}" \
      ai.ninfer.base="${CUDA_RUNTIME_IMAGE}" \
      ai.ninfer.arch="sm_120a"

WORKDIR /workspace
EXPOSE 8080
STOPSIGNAL SIGTERM
# Last, so nothing above runs as it and nothing below can run as root. 1000 is `ubuntu` in the
# Ubuntu 24.04 base; ./run passes HOME=/home/ubuntu.
USER 1000:1000
ENTRYPOINT ["ninfer-serve"]
CMD ["--help"]
