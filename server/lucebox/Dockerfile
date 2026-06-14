FROM nvidia/cuda:13.1.2-cudnn-devel-ubuntu24.04
RUN apt update
RUN apt install -y cmake
RUN apt install -y git
COPY lucebox-hub /lucebox
WORKDIR /lucebox/dflash

RUN git submodule update --init --recursive

RUN apt install -y ninja-build
RUN cmake -B build -S . -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_CUDA_ARCHITECTURES=120 \
  -DDFLASH27B_ENABLE_BSA=ON \
  -DDFLASH27B_TESTS=ON
RUN cmake --build build --target test_dflash -j 28

RUN apt install -y python3
RUN apt install -y python3.12-venv
RUN python3 -m venv .venv
RUN .venv/bin/pip install fastapi uvicorn transformers jinja2
RUN .venv/bin/pip install gguf

RUN sed -i 's/"enable_thinking": False/"enable_thinking": True/' /lucebox/dflash/scripts/server.py
RUN apt install -y curl jq
