# Distilled — confirmed facts for ninfer

Compact, confirmed learnings: formats, protocols, tool/library behavior, measured limits. Read
before new work; add an entry whenever something is confirmed — **above the `## Open Questions`
section**, which always stays last. Keep it dense.

**Provenance discipline:** every entry carries a tag — `[SOURCE]` (upstream code, cite
`path:line`), `[DOC]` (documentation), `[REPO]` (this repo's own code/config), `[LIVE YYYY-MM-DD]`
(observed on a running system). **Do not upgrade a tag without doing the work.**

<!-- Entry shape:

## <Topic — the rule in one line> [TAG][TAG]

- The fact, stated so it can be checked.
- The evidence: command + verbatim output, or `path:line`.
- What it means for this project.
-->

---

## NInfer needs io_uring; docker's default seccomp profile blocks it [SOURCE][LIVE 2026-10-27]

- NInfer opens model files `O_DIRECT` (`src/core/io/file.cpp:37` @ `64d10fbf`) and its direct read
  queue calls `io_uring_queue_init` with no fallback (`src/core/io/read_queue.cpp:83`).
- Docker 29.8.0's default profile: `io_uring_setup` (syscall 425) → `-1 Operation not permitted`;
  with `--security-opt seccomp=unconfined` → `3 ok`. Probe: `docker run --rm --network none
  --entrypoint python3 <vllm image> -I -c '<libc.syscall(425, 8, buf)>'`, 1 run each, 2026-10-27.
  Host `kernel.io_uring_disabled` = `0`.
- So every NInfer container runs under `seccomp/ninfer.json` (docker default + the three io_uring
  syscalls, Plan 01 T2), never `unconfined`. [LIVE 2026-10-27] gate step 3 under ours:
  `uring ok`, `keyctl` still `Operation not permitted` (unconfined: `keyctl ok`), 2 runs.
- [REPO] docker 29.8.0 was cut from moby tag `docker-v29.8.0` (`^{}` = dockerd `GitCommit`
  `3ce5872b`); at that tag the default profile is `vendor/github.com/moby/profiles/seccomp/default.json`
  (sha256 `536529b6…`), **not** `profiles/seccomp/default.json` (404). io_uring registered buffers count against
  `RLIMIT_MEMLOCK` (`io_uring_register(2)` [DOC]), hence `--ulimit memlock=-1`.

## The converter's base must be the BF16 checkpoint; our NVFP4 one is its `nvidia` source [SOURCE][DOC]

- Recipe: `python3 -m tools.convert --model <BF16 base> --recipe qwen3_8_flash_next --source
  nvidia=<NVIDIA NVFP4> --components text,vision,mtp --rows-per-chunk 16384 --name … --out …`,
  with `OMP_NUM_THREADS=4 MKL_NUM_THREADS=4` (`docs/weight-conversion.md:86-97` @ `64d10fbf`).
- The builder rejects any non-BF16 base tensor, expects fused base expert banks
  (`mlp.experts.gate_up_proj`) and requires every base key to be consumed
  (`tools/convert/qwen4_exp.py` `Builder.add`, `bank_source`, `build_model`).
- `models/vllm/nvidia/Qwen3.8-Flash-Next-NVFP4` (header probe, 2026-10-27): per-expert `U8`/`F8_E4M3`/
  `F32` experts, block-FP8 MTP experts, FP8 PLE — exactly the `nvidia` source, not a base.
  `…-FP4-Local` is neither (CSF expert scales, MXFP8 dense, NVFP4 PLE).
- BF16 base: `Qwen/Qwen3.8-Flash-Next` rev `de4b8e4d43b917e7706784d8bb445c9af86a3540`, 131 shards,
  360.0 GB, not gated (HF API, 2026-10-27).
- PLE is quantized BF16 → NF4 by a JIT CUDA extension (`tools/convert/quantization/stored_scale.py:22-33`,
  `-gencode=arch=compute_120,code=sm_120`): the converter needs nvcc, a C++ compiler and ninja.
- [LIVE 2026-10-27] Real BF16 base headers (HF range reads, rev `de4b8e4d`):
  `…layers.0.mlp.experts.gate_up_proj` BF16 `[512, 1280, 2560]` (shard 00002);
  `…layers.1.ple.ple_embedding.ngram_embedding.shard_0.weight` BF16 `[2500012, 160]` (shard 00005).
  The NVFP4 checkpoint has no `gate_up_proj` and its PLE shard is `F8_E4M3` of the same shape.
- The NVFP4 checkpoint's `hf_quant_config.json` is `quant_algo: MIXED_PRECISION` at the top; the
  per-module `quantized_layers` has the 48 main-expert layers at `NVFP4` (plus one `FP8`, one
  `FP8_BLOCK_SCALES`). Its index has 294,912 main-expert keys (48 × 512 × {gate,up,down} ×
  {weight, weight_scale, weight_scale_2, input_scale}). The converter reads tensors, not this file.
- [LIVE 2026-10-27] Every `tools.convert`/`tools.artifact` module imports in the `../vllmcustom`
  image (Python 3.12.3, torch 2.14.0/CUDA 13.2, NumPy 2.2.6), 1 run; the image also carries `hf`
  (`/opt/venv/bin/hf`) — the host has none. A real conversion under 3.12 is unconfirmed (T7).
- Output: ~100.67 GiB payload; an entry plus `.part-NNNN` files at 32,000,000,000 B each, and
  `<entry>.conversion.json`; existing outputs are never overwritten (`docs/weight-conversion.md:427-436`).

## Flash-Next residency: experts are host-backed, PLE is read from disk every step [DOC]

- Main experts: full registered host backing always (~63.3 GiB; 1,415,577,600 B per layer × 48)
  plus a GPU static cache; small-batch misses run **on the CPU** (`docs/cli.md:29-31`,
  `docs/maintainer/expert-weight-cache.md`). MTP experts (2,516,889,600 B) stay on the GPU.
- `--kv-capacity auto` with CPU expert offload reserves one full context and gives the rest of the
  device budget to expert storage; startup keeps 1 GiB of planning headroom (`docs/cli.md`
  § Context and memory).
- PLE: no RAM/GPU table; NF4 rows read `O_DIRECT` via io_uring, 64 MiB row cache, first consumed at
  zero-based layer 1 (`docs/maintainer/qwen4_exp-execution.md` §1, §4.1). Disk latency is on the
  decode path — on NFS here unless the artifact is copied to NVMe (Plan 01 T11).
- CPU MoE path on this AVX2-only 5950X uses the AVX2 plan, `max_columns = 2`
  (`src/ops/moe/routed_swiglu/cpu/nvfp4/shapes/h2560_i640_e512_k10/dispatch.cpp`).
- Whether all 48 × 512 experts fit in the GPU cache on 96 GB is **unconfirmed** — see Open Questions.

## Build deps at the pin are all in Ubuntu 24.04's archive, liburing exactly at the floor [SOURCE][LIVE 2026-10-27]

- NInfer @ `64d10fbf` requires cmake ≥ 3.28, hwloc ≥ 2.5, liburing ≥ 2.5, libcurl ≥ 7.85, FFmpeg
  (no floor), C++/CUDA 20, and targets `sm_120a` only (`cmake/Dependencies.cmake:4-23`,
  `CMakeLists.txt:3-12`). It links `CUDA::cudart`, `CUDA::cuda_driver`, `CUDA::nvtx` — no cuBLAS.
- Ubuntu 24.04.4 candidates (`apt-cache policy` in `nvidia/cuda:13.2.0-devel-ubuntu24.04`, as uid
  1000 with private apt dirs, 1 run): cmake 3.28.3, liburing 2.5 (`liburing-dev`, runtime
  `liburing2`), hwloc 2.10.0 (`libhwloc15`), libcurl 8.5.0 (`libcurl4t64`), FFmpeg 6.1.1
  (`libavcodec60`, `libavformat60`, `libavutil58`, `libswscale7`), g++ 13.2.0.
- liburing sits exactly at the 2.5 floor: an upstream bump to `liburing>=2.6` cannot build on
  24.04 without a PPA or a source build.

## `ninfer-serve` defaults and the request contract [DOC]

- Defaults (`docs/serving.md` § Server options @ `64d10fbf`): `--host 127.0.0.1 --port 8080
  --max-context 8192 --max-concurrency 1` (range 1..8), `--kv-capacity` omitted = `--max-context`,
  `--kv-dtype bf16`, `--vision` off, `--preserve-thinking` off, thinking on, prefix reuse on,
  `--log-stats-interval-ms 5000`, `--default-max-tokens 8192`, `--host-context-mib` 8 GiB + 8
  StateImages, prefill chunk 1024 (4096 under CPU expert offload).
- Flash-Next speculation: native MTP only, `--spec mtp --draft-tokens 1..5`, full shared head —
  no `--lm-head-draft`, no DFlash (`docs/cli.md` § Speculative decoding).
- `model` must equal the public ID (`--model-id`, else the artifact's `metadata.name`)
  (`docs/serving.md:228`); `llama-proxy` addresses `:8700` as `cosmo-6000`.
- "Unknown top-level fields are ignored" (`docs/serving.md:210`) — so a bench's `ignore_eos` is
  silently dropped, and NInfer may stop at EOS where vLLM does not (Plan 01 T9 checks
  `completion_tokens`).
- `--request-log-jsonl FILE`: the parent directory must already exist (`docs/serving.md:1009`).
- `NINFER_CUDA_SYNC` defaults to `spin` (lowest latency); `blocking`/`yield`/`auto` exist
  (`docs/cli.md` § CUDA synchronization). The Engine-ready log reports the mode.
- Endpoints: `/health` (200 `{"status":"ok"}`, 503 after an engine-wide failure), `/metrics`
  (`ninfer_engine_ready`, `ninfer_spec_decode_*`), `/v1/models`, `/v1/chat/completions`
  (`reasoning_content` separate from `content`), `/v1/responses`, `/v1/messages`
  (`docs/serving.md` § Endpoints, § Live metrics).

## Open Questions

<!-- Things we believe but have not confirmed. Check here before trusting a related assumption;
     move an item up (with its tag) once it is confirmed, or into pitfalls.md if it bit. -->

- **Do all 48 × 512 main experts fit in the GPU cache on 96 GB?** Estimate: ~10 GiB fixed weights +
  ~3.4 GiB FP8 KV for one 262,144-token context (13,152 B/token) + workspace + 1 GiB headroom
  leaves > 63.3 GiB. Arithmetic only — Plan 01 T8 reads the debug planning log.
- **Does the converter run under Python 3.12 / torch 2.14** (the `../vllmcustom` image)? NInfer
  maintains Python 3.11 but calls it "not a configuration version gate"
  (`docs/maintainer/build-system.md`). Plan 01 T4 (import), T7 (real run).
