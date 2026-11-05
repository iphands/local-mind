# basalt — Basalt in a container, on :8700

[Basalt](https://github.com/jesdga95/basalt) (a local inference engine for **Qwen3.8-Flash-Next**)
built from a pinned commit in our own image for the RTX PRO 6000 Blackwell (sm_120), and one
launcher, `./run`, that serves a `.basalt` pack on `:8700` with an OpenAI- and Anthropic-compatible
API.

Basalt loads only Flash-Next `.basalt` packs. `.ninfer` artifacts (for example the QUASAR 27B)
belong in `../ninfer`.

**Status (2026-11-04):** pins and shared guards in place; `./build` and `./run` are still being
written (Plan 01). No image has been built and nothing has booted yet.

## Prerequisites

- Docker 29+ with BuildKit and the NVIDIA runtime, used as yourself (no `sudo`).
- The pack, downloaded by the owner:
  `models/vllm/jesdga/Qwen3.8-FN-NVFP4-DQ6_K.basalt` (131.7 GB; `models` →
  `/mnt/noir/scratch/ai/llm/models`).
- Nothing else on `:8700`: stop `vllm` (`../vllmcustom`), `../ninfer` or `../ignis` first.

## Pins

`versions.env`: Basalt `dc047a77` (main on 2026-11-04, the v0.2.1 engine), CUDA 13.4.3 devel and
runtime images by digest, image repository `iphands/basalt-blackwell`.
