# Plan: conservative sm_120-only build trimming (torch, torchaudio, FlashInfer, vLLM)

## Context
Everything is built from source for sm_120 (`container/Dockerfile`: torch 317-372, torchaudio 382-401, flashinfer 406-496, vllm 501-573).
Torch takes hours, FlashInfer ~35 min, vLLM ~404 CUDA targets. Only trimming today: `TORCH_CUDA_ARCH_LIST=12.0`, `BUILD_TEST=0`, FA2/FA3 arch patch.
The linked PyTorch tracing-based selective build targets CPU/lite-interpreter kernels (examples use `USE_CUDA=0`); it cannot shrink nvcc work, so it is
not used directly. Its idea (record what real workloads use, then prove each trim against that record) is reused as an inventory + smoke gate.
Goal from the user: skip what is not needed on sm120, **but do not over-skip** — the model set is broad: Muse (Glimmer, dense, ViT, DFlash drafter),
Nemotron-3 Super / 3.5-Super-VL (Mamba2 hybrid, latent MoE relu2, NVFP4, RADIO vision, Parakeet audio), Qwen3.5/3.8 (GDN), Laguna, and plausibly gpt-oss (MXFP4),
Llama, Gemma, DeepSeek/GLM/Kimi, MiniMax. Evidence below is from reading the trees; build-time savings are not yet measured.

## Policy: three tiers
- **TRIM NOW** — unused by any plausible model, or arch-unreachable on sm120.
- **OPT-IN PROFILE** — build default stays *full*; a `slim` choice is available per stage (env/arg in `container/build`, passed like the `buildjobs` secret only if
  it does not change semantics; semantic flags go in `ENV` so the layer cache key stays honest). Each profile lives only in its own stage so a FlashInfer/vLLM tweak never re-runs torch.
- **NEVER TRIM** — see list at the end.
Any dropped FlashInfer module still JIT-compiles at runtime (nvcc/ninja/CUDA devel are in the runtime image, `Dockerfile:278-312,590`), so a wrong FlashInfer trim costs
cold-start time, not correctness. A wrong torch/vLLM trim is a hard "no kernel/op" failure — those get the strictest gate.

## Workstream 0: coverage gate (build first; model-free to write)
1. `tools/inventory`: while a launcher is up, log aten ops+dtypes (`TorchDispatchMode`; eager, CUDA-graph capture, torch.compile), `torch.ops._C/_moe_C` ops hit, and FlashInfer
   modules built/loaded (`$FLASHINFER_WORKSPACE_BASE/cached_ops`). Emit blog-style `selected_ops.yaml` per model under `context/`. Run for each model family, union them.
2. `tools/smoke-model <model>`: boot with empty FlashInfer cache, one short request + one long prefill + one spec-decode request; count `Building JIT module` lines
   (`flashinfer/jit/core.py:594`); strict mode `FLASHINFER_DISABLE_JIT=1` (raises `MissingJITCacheError`, `jit/core.py:348`); grep for `No such operator`, `marlin kernel ... not built`,
   `FP8 KV cache requires`, backend-selection lines. Cheap pre-check for a *new* model: derive (head_dim, Q/KV heads, kv dtype, quant) from `config.json` and compare to the built set.
3. `container/archs` extended to diff `dir(torch.ops._C)` against `csrc/torch_bindings.cpp` after any vLLM trim.

## Workstream 1: torch (ENV block, `Dockerfile:325-331`)
TRIM NOW: `USE_FBGEMM=0 USE_CUFILE=0 USE_CUSPARSELT=0 USE_TENSORPIPE=0 USE_MPI=0 USE_NNPACK=0 USE_PYTORCH_QNNPACK=0 USE_XNNPACK=0 USE_ITT=0 USE_NUMA=0
USE_NVSHMEM=0 USE_CUDSS=0 USE_MAGMA=0 BUILD_LAZY_TS_BACKEND=0`, and `USE_MKLDNN=0` (CPU-only fallback speed loss; verify Inductor imports/`torch.compile` smoke still pass).
`USE_MPI=0` also lets `libopenmpi-dev` leave the base stage. 2.14 forwards any `USE_*/BUILD_*` env var (`cmake/EnvVarForwarding.cmake`).
KEEP: `USE_DISTRIBUTED`, `USE_GLOO`, `USE_NCCL` (vLLM inits `cpu:gloo,cuda:nccl`, `parallel_state.py:1736`), `USE_KINETO` (profiler), `USE_CUDNN` (Conv3d/ViT patch embeds, FlashInfer cudnn),
`USE_OPENMP`, `USE_NUMPY`, **`USE_FLASH_ATTENTION` and `USE_MEM_EFF_ATTENTION`** (vLLM disables cuDNN SDPA, `platforms/cuda.py:52`, so torch flash/mem-eff are the real SDPA kernels for
granite/phi4mm/midashenglm/HF-fallback/Parakeet encoders; multimodal Nemotron/Muse may land there). Keep them until the inventory proves otherwise.
Never set mobile/lite-interpreter/static-dispatch/native-arch flags.
OPT-IN experiments (measure first, decide with the user): `USE_CPU_VECTORIZATION=0`, `USE_SYSTEM_NCCL=1` (skips NCCL compile; needs libnccl-dev, version-coupled), `USE_PRECOMPILED_HEADERS=1`.
Bundle torch flag changes into the already-forced 2.14 full rebuild.

## Workstream 2: torchaudio (`Dockerfile:388`)
TRIM NOW: `BUILD_RNNT=0 BUILD_ALIGN=0 BUILD_CUDA_CTC_DECODER=0` (vLLM uses only Resample/mel/kaldi/spectrogram, all pure-torch; Parakeet uses `torch.stft` + HF).
KEEP `USE_CUDA=1` (CUDA-minor import guard and `cuda_version()` smoke at `Dockerfile:400` depend on it) and torchaudio itself at 2.11.0 (transformers imports it).
Verify torchaudio 2.11 sources still compile against 2.14 headers (already a flagged risk).

## Workstream 3: FlashInfer (patch `aot.py` in `flashinfer-build` only; PEP 517 provider calls `compile_and_package_modules(config=None)`)
Mechanism: assert-once patch script under `container/patches/` (same pattern as `vllm-flash-attn-arch.py`), injected before the provider build.
- Finding: the 384-module XQA AOT grid is **mis-targeted** — groups {1,2,4,8} but real GQA groups seen are 16 (Nemotron-3-Super, Muse, Qwen3.5-122B), 12 (Flash-Next), 6 (Qwen3.8-27B); the
  runtime cache holds only JIT-built `xqa_*_hd256_group6` modules incl. spec-dec `q_seq_len` 2-9, which the AOT grid never covers. JIT costs ~6-11 s/module and persists in the cache mount.
- TRIM NOW: XQA page size 128 (backend asserts ≤64 off sm100); FA2 MLA hd512 modules (MLA on sm12x uses TRITON_MLA / sparse SM120 instead).
- OPT-IN `FI_AOT_PROFILE=slim`: replace the XQA grid with explicit tuples (bf16, pages 16/32/64, groups 2/4/6/8/12/16, hd 64/128/256) or drop it; drop `use_logits_soft_cap=True` cross
  product (≈half of FA2 modules; Gemma2-style hd128+cap would JIT); fp16-qo variants. Default stays `full` until the smoke gate shows the cost/benefit.
- NEVER TRIM: FA2 hd 64/128/256, sliding-window on/off, fp8-KV variants, oai_oss sink hd64 (gpt-oss), NVFP4/MXFP8 sm120 gemm/quant, `cutlass_fused_moe_sm120`, sampling/topk/rope/norm, `pcie_ipc_comm`, `trtllm_comm`.
- Note: SSU, fused GDN decode, `trtllm_utils`, XQA spec-dec, hd512 are not AOT'd on 12.0f and already JIT at runtime; keep the runtime image's nvcc/ninja.

## Workstream 4: vLLM (patches in `vllm-build` only)
Already skipped on 12.0 by arch gates: Machete, scaled_mm sm90/sm100, grouped_mm sm90/100, W4A8, cutlass MLA, FlashMLA, DeepSelect, FP4 sm100, FA3 (patched).
- TRIM NOW: Marlin and MoE-Marlin **int8-activation and fp8-activation variants** (~44% of Marlin instantiations; reachable only through `VLLM_MARLIN_INPUT_DTYPE`, `marlin_utils.py:720-736`).
  Implementation: assert-once patch to `csrc/quantization/marlin/generate_kernels.py` (MoE reuses it, `CMakeLists.txt:1342-1343`), verified by diffing the generated file list.
- OPT-IN `EXTRAS`: skip `hadacore` (PTX-only on 12.0, fallback exists) and `qutlass` (optional import) when `slim`; GPTQ-int8 Marlin (`u8b128`).
- KEEP (deliberately): NVFP4/MXFP4/FP8/MXFP8 Marlin (gpt-oss MXFP4 defaults to Marlin on sm120, NVFP4 Marlin is a fallback), AWQ-u4 and GPTQ-u4b8 Marlin, awq/gptq/int8 `_C_stable_libtorch` (small TUs),
  fused GDN and KDA decode (qwen/run default), FlashKDA, DeepGEMM host ext (needs `libdw-dev`), hisparse/DSv4 helpers (tiny), NVFP4 sm120 cutlass, `scaled_mm_c3x_sm120`, `cutlass_moe_data`, topk/moe_align/grouped_topk/permute kernels.
- Set `VLLM_FLASH_ATTN_SRC_DIR` to a local clone later to inspect FA2's head-dim/dtype set; FA2 stays (default ViT backend and fallback on sm120).

## Sequencing / process
Follow `context/plans/RULES.md` (read in full first) and `SERIES.md`: new plan via `NN_example.md` + tracker, one task per commit `[P<n>][T<n>][<topic>]`, tracker row in the same commit. Never push.
Order: W0 gate -> W2 torchaudio -> W1 torch flags (same rebuild as 2.14 repin) -> W4 vLLM Marlin patch -> W3 FlashInfer patch/profile -> opt-in experiments.
Claude edits and verifies; the user runs `./container/build` (per memory). `PREFLIGHT_ONLY=1 ./container/build` first.

## Verification
- Preflight; user-run build; record per-stage wall time and `ccache -s` before/after (baseline needed — none recorded today).
- `./container/archs IMAGE:TAG` (sm120-only), `uv pip check`, runtime-stage import asserts; add `is_nccl_available()/is_gloo_available()`, torchaudio `Resample`, flashinfer import, `torch.compile` smoke.
- Project gate steps 1-5 (model-free). Step 6 only when named: smoke-boot each of Muse, Nemotron (text and VL), Qwen, Laguna (+ gpt-oss/Gemma-class if available), compare `./bench/bench` to baseline, compare post-build
  inventory + JIT-module count to pre-build; any new "no operator/kernel" error or unexpected new JIT module reverts that trim.
