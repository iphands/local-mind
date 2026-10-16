# Plan 05 — Trim the source build to what sm_120 serving uses

> **Status**: pending
> **Created**: 2026-10-16
> **Depends on**: N/A (rebuild timing is coordinated with Plan 01 — see Open Question 6)
> **Goal**: torch, torchaudio, FlashInfer and vLLM stop compiling features and kernels that no plausible model on this sm_120 box can reach, with a model-free gate plus per-launcher smoke boot proving nothing reachable was removed, and a measured before/after build time.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Remove arch-unreachable or provably unused build work in three tiers — **TRIM NOW** (unused by any plausible model), **OPT-IN PROFILE** (default stays full; a `slim` choice per stage), **NEVER TRIM** — and gate every trim against what the launchers actually hit.

**Deliverables**:
1. A per-stage build-time baseline (none exists today) and the same numbers after the trim.
2. `tools/smoke-model`: a coverage check per model — runtime-JIT modules diffed against a known list, missing-op/kernel greps, and a `config.json` pre-check for new models.
3. Runtime-stage image assertions for every feature the trims touch (NCCL, Gloo, torchaudio, flashinfer providers, `torch.compile`), plus a `torch.ops._C` symbol diff in `container/archs`.
4. torchaudio and torch flag trims in the Dockerfile.
5. Build-time patches: vLLM Marlin (int8-activation only), FlashInfer AOT (XQA page 128, FA2 MLA hd512), each assert-once with a host test, each version-gated.
6. Opt-in `slim` profiles (`FI_AOT_PROFILE`, `VLLM_EXTRAS`) wired `ARG`→`ENV` and stamped as an image label.
7. One owner rebuild, gate step 5, and a step-6 smoke boot of every launcher: `qwen`, `muse`, `laguna`, `qwen3.8-flash-next`, `nemotron`.

**Estimated effort**: Large (3 days of edits) plus one or two owner full rebuilds (hours each).

---

## Scope

**In**: `container/Dockerfile`, `container/build` (profile plumbing only), new build patches under `container/patches/`, `container/archs`, `tools/smoke-model`, README § *sm120-only*, `context/`.
**Out**: any pin in `container/versions.env`, runtime patches, launcher behavior, torch `USE_FLASH_ATTENTION`/`USE_MEM_EFF_ATTENTION`, anything that removes a kernel listed under NEVER TRIM. If a trim needs to cross this line, **stop** and record a blocker in the tracker.

---

## Context

Everything on the LLM path is compiled from source for `TORCH_CUDA_ARCH_LIST=12.0` (`container/Dockerfile` stages `torch-build` ~317-372, `torchaudio-build` ~382-401, `flashinfer-build` ~406-496, `vllm-build` ~501-573). Torch takes "hours" (`README.md:512,778`), FlashInfer AOT ~35 min (`README.md:441`), vLLM ~404 CUDA targets (`README.md:744`). The only trimming today is the arch list, `BUILD_TEST=0`, and the FA2/FA3 fetch patch (`container/patches/vllm-flash-attn-arch.py`).

Prompted by the owner reading PyTorch's tracing-based selective build (https://pytorch.org/blog/pytorchs-tracing-based-selective-build/), with the directive (2026-10-15): skip what is not needed on sm_120, **but do not over-skip** — the served set is broad: Muse (Glimmer, ViT, DFlash), Nemotron-3/3.5-Super(-VL) (Mamba2 hybrid, latent MoE relu2, NVFP4, RADIO vision, Parakeet audio, MTP), Qwen3.5/3.8 (GDN, MTP), Laguna, and plausibly gpt-oss (MXFP4), Llama, Gemma, DeepSeek/GLM/Kimi, MiniMax.

Research for this plan was a source read of the four trees under `/mnt/noir/scratch/ai/vllm/build/src/` at the **main snapshot** (vLLM `4d9a8d6`, FlashInfer `v0.7.1`, torch `v2.14.0`, torchaudio `v2.11.0`). Line numbers below are for that snapshot. The release set (vLLM `v0.29.0`, FlashInfer `0.6.18`, torch `2.13.0`) differs — every patch must handle both (T6, T7).

### Pre-Identified Bug/Issue

Today's torch and torchaudio feature sets — everything not named here builds at torch's default (ON for MKLDNN, FBGEMM, NNPACK, QNNPACK, TensorPipe, MPI, NUMA, ITT, cuFile, NVSHMEM…):

```bash
$ sed -n 325,331p container/Dockerfile
ENV TORCH_CUDA_ARCH_LIST=${TORCH_CUDA_ARCH_LIST} \
    PYTORCH_BUILD_VERSION=${TORCH_VERSION} \
    PYTORCH_BUILD_NUMBER=1 \
    USE_CUDA=1 USE_CUDNN=1 USE_CUSPARSELT=1 \
    USE_FLASH_ATTENTION=1 USE_MEM_EFF_ATTENTION=1 \
    BUILD_TEST=0 USE_DISTRIBUTED=1 \
    _GLIBCXX_USE_CXX11_ABI=1
$ grep -n 'BUILD_RNNT' container/Dockerfile
388:    USE_CUDA=1 BUILD_RNNT=1 BUILD_ALIGN=1 BUILD_CUDA_CTC_DECODER=1 BUILD_CPP_TEST=0
```

FlashInfer's AOT XQA grid does not cover the models served here (see Key Facts): the flashinfer cache under `/mnt/noir/scratch/ai/vllm/cache/flashinfer/0.7.0.post1/120f/cached_ops` holds runtime-JIT `xqa_*_hd256_group6_*` modules (incl. spec-dec `q_seq_len` 2–9) — the precompiled grid uses groups {1,2,4,8} only. Re-list that directory in T2 to capture it verbatim.

### Why tiers + a coverage gate (not the blog's selective build)

The blog's flow (`TRACING_BASED=1` instrumented build → `model_tracer` on a `.ptl` lite-interpreter model → `SELECTED_OP_LIST` YAML → `BUILD_LITE_INTERPRETER=1` rebuild) only selects **CPU/QuantizedCPU** kernel registrations, prim ops, CPU dtype switches and custom classes; its examples build with `USE_CUDA=0 USE_DISTRIBUTED=0`. Our cost is nvcc, vLLM is eager/`torch.compile` Python, and `INTERN_BUILD_MOBILE` forces `BUILD_PYTHON=OFF` (torch `CMakeLists.txt:836-846`). Rejected. Its *idea* — record what real workloads use and check each trim against it — survives as T2.

An aten-op inventory (`TorchDispatchMode`) was also rejected as the torch gate: the torch trims are feature flags, not op registrations, so an op list cannot validate them; and vLLM runs the model in an EngineCore subprocess, CUDA-graph replay bypasses the dispatcher, and Inductor's Triton never touches aten. Torch trims are gated by in-image asserts (T3) plus smoke boots (T9).

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| torch 2.14 forwards every `USE_*`/`BUILD_*`/`CMAKE_*` env var to CMake | `ENV USE_X=0` works | torch `cmake/EnvVarForwarding.cmake:~185` [SOURCE] |
| vLLM always inits `cpu:gloo,cuda:nccl`, even single-GPU | keep `USE_DISTRIBUTED`, `USE_GLOO`, `USE_NCCL` | vllm `distributed/parallel_state.py:1731-1740` [SOURCE] |
| vLLM disables cuDNN SDPA | torch flash/mem-eff are the real SDPA kernels for ViT/audio encoders | vllm `platforms/cuda.py:52` [SOURCE] |
| vLLM profiler needs Kineto CUDA activity | keep `USE_KINETO` | vllm `profiler/wrapper.py:216-220` [SOURCE] |
| No vLLM use of FBGEMM (torch's), MKLDNN on CUDA, TensorPipe/RPC, MPI, cuFile, cuSPARSELt, NVSHMEM | safe to disable | grep of `vllm/` + `_custom_ops.py:3757` (oneDNN CPU-only, `hasattr`-guarded), `nvfp4/fbgemm.py:43` (pip `fbgemm_gpu`, separate) [SOURCE] |
| FlashInfer's `mpi4py` use is lazy/optional | dropping `libopenmpi-dev` cannot break import | flashinfer `comm/comm_backend.py:28-34` (lazy, raises ImportError), `comm/trtllm_mnnvl_ar.py:28` (inside a function) [SOURCE] |
| torchaudio: vLLM uses only `Resample`, mel/fbanks, spectrogram, `compliance.kaldi` | RNNT/align/CTC unused | vllm `multimodal/audio.py:289-335`, `cohere_asr.py:11`, `funasr.py:8`, `midashenglm.py:311-341` [SOURCE] |
| torchaudio `USE_CUDA=1` is load-bearing | the stage smoke calls `torch.ops._torchaudio.cuda_version()` | `container/Dockerfile:400` [REPO] |
| Marlin **int8**-activation kernels are reachable only via `VLLM_MARLIN_INPUT_DTYPE=int8` | trim | vllm `quantization/utils/marlin_utils.py:720-736`; `auto_gptq.py:256,264`, `auto_awq.py:443` go through it; `compressed_tensors_w4a8_int.py:98` uses `params_dtype` [SOURCE] |
| Marlin **fp8**-activation kernels are reached **without** the env var | **keep** | `compressed_tensors_w4a8_fp8.py:89` hardcodes `act_type=float8_e4m3fn` → `MarlinLinearKernel` (`mixed_precision/marlin.py:84`); CUTLASS W4A8 is `9.0a`-only (vllm `CMakeLists.txt:1056-1070`) [SOURCE] |
| Marlin generator encodes activation type per `QUANT_CONFIGS` entry | `kS8` entries at ~:83,:91; `kFE4M3fn` at ~:99-115 | vllm `csrc/quantization/marlin/generate_kernels.py` [SOURCE]; MoE Marlin reuses a generator — confirm the exact path in T6 (`CMakeLists.txt:649,1343`) |
| FlashInfer provider build ignores `aot.py` CLI flags | only `get_default_config()` applies; trim = patch | flashinfer `flashinfer-jit-cache-provider/build_backend.py:~88-106` (`config=None`) → `aot.py:1170-1173` [SOURCE] |
| XQA page size > 64 unreachable off sm100 | trim page 128 | vllm `v1/attention/backends/flashinfer.py:876` (assert) [SOURCE] |
| MLA on sm12x offers only `TRITON_MLA`, `FLASHINFER_MLA_SPARSE_SM120` | FA2 MLA hd512 modules unreachable | vllm `platforms/cuda.py:134-138` [SOURCE] |
| XQA is not silently disabled offline | `has_nvidia_artifactory()` returns True when `flashinfer_cubin` is installed | vllm `utils/flashinfer.py:626-627`; image installs `flashinfer-cubin` [SOURCE] |
| Not precompiled on `12.0f`, always runtime-JIT | SSU, fused GDN decode, `trtllm_utils`, XQA spec-dec, hd512 | flashinfer `aot.py:788-796,1055,1071` + cache listing [SOURCE] |
| Runtime image can JIT | `FROM base` = `cudnn-devel` with nvcc, ninja | `container/Dockerfile:278-312,590` [REPO] |
| The torch 2.14 snapshot image is **already built** | torch flags cost a fresh full rebuild, not a free ride | `container/.last-build`: `VTAG=cu1321-sm120-main-vllm0.33.0.dev20261015-g4d9a8d6`; Plan 04 tracker evidence [REPO] |
| Build-time savings | **unmeasured** | no per-stage timing recorded anywhere — T1 |

### Prior Measurements

| Source | Metric | Result | Runs | Spread |
|--------|--------|--------|------|--------|
| `README.md:441,610,653` | FlashInfer AOT | ~35 min, 3408 kernels | anecdotal | — |
| `README.md:640-645` | torch build peak memory | 89.8 GiB at 14 jobs | 1 | — |
| research scratch run of `generate_kernels.py 12.0` (2026-10-15) | Marlin instantiations per family | 876 (s8- + fp8-activation ≈ 390) | 1 | not a build time |
| `.ninja_log` in the flashinfer cache | XQA JIT cost | ~6–11 s/module | — | unverified spread |

### Rejected Claims

- **"Trim Marlin fp8-activation kernels, they need `VLLM_MARLIN_INPUT_DTYPE`."** False — W4A8-FP8 compressed-tensors checkpoints reach them unconditionally (Key Facts). Do NOT trim.
- **"`USE_DISTRIBUTED=0` is fine on one GPU."** False — vLLM inits the default process group regardless. Do NOT disable.
- **"torchaudio can drop `USE_CUDA`."** The stage smoke depends on `cuda_version()`. Do NOT change without rewriting that check (out of scope).
- **"XQA silently falls back to FA2 offline."** Not on this image — `flashinfer_cubin` short-circuits the network check.
- **"Use `FLASHINFER_DISABLE_JIT=1` as the strict gate."** It fails every model regardless of trimming, because SSU/GDN/spec-dec XQA are never precompiled. T2 uses an allowlist diff instead.
- **"Torch flags ride along with the 2.14 rebuild."** That rebuild already happened (2026-10-15).

---

## Step-by-Step Tasks

### Wave 1 — measure and gate (no image change). Exits when T1–T3 are committed and gate steps 1–5 pass.

### T1: Per-stage build-time baseline

**File**: `tools/build-times` (new), tracker Evidence.

**What to do**: Extract per-stage wall time (`torch-build`, `torchaudio-build`, `flashinfer-build`, `vllm-build`, `vllm-openai`) and `ccache -s` from a build. First establish what exists: BuildKit history (`docker buildx history ls` / `inspect`) for the 2026-10-15 image, or the build's plain-progress log if `container/build` keeps one — unconfirmed, confirm here. If no record of a cold build exists, the baseline is taken from the next owner build **before** any trim lands (T4/T5 wait for it).

**Verify**:
```bash
tools/build-times <build log or history ref>   # → one line per stage: name, seconds, cached?
# + gate steps 1-5 — RULES.md § Project gate
```

**Expected observation**:
- **Confirms if**: torch-build dominates (hours) and flashinfer-build is ~35 min, matching the README.
- **Refutes if**: a stage the README calls fast dominates — re-prioritize the waves.
- **Within noise if**: n/a — this is one cold build; label it N=1 and say so (Rule P1). A cold full build cannot be run 3× cheaply; record that limitation.

**Commit**: `[P5][T1][trim] add per-stage build-time extractor + baseline`

### T2: `tools/smoke-model` coverage check

**File**: `tools/smoke-model` (new), `context/plans/05_jit_allowlist.txt` (new, per-model known runtime-JIT module patterns).

**What to do**:
1. **Pre-check (no GPU)**: read a model's `config.json` → head_dim, Q/KV heads (GQA group), kv dtype default, quant method, Mamba/GDN layers, vision/audio towers; print which precompiled FlashInfer modules it would hit and which it will JIT.
2. **Boot check (step 6 only)**: against a running server, send one short request, one long prefill, one spec-decode request; then diff `$FLASHINFER_WORKSPACE_BASE/**/cached_ops/*` before/after against the allowlist (SSU, fused GDN decode, `trtllm_utils`, XQA spec-dec and hd512 are expected JIT); grep the container log for `No such operator`, `has no attribute` on `torch.ops._C`, `marlin kernel`, `not built`, `FP8 KV cache requires`, `Building JIT module` (flashinfer `jit/core.py:594`), and the `Using … backend` lines.
3. List `cached_ops` today, verbatim, into the tracker (the Pre-Identified XQA evidence).

**Verify**:
```bash
tools/smoke-model --precheck /mnt/noir/scratch/ai/vllm/models/<each local model>   # → prints group size per model; Nemotron-3-Super / Muse 16, Qwen3.8-27B 6, Flash-Next 12
# + gate steps 1-5
```

**Commit**: `[P5][T2][trim] add smoke-model coverage check and JIT allowlist`

### T3: Image assertions for every touched feature

**File**: `container/Dockerfile` (runtime-stage `RUN` with the existing import checks, ~636-657), `container/archs`.

**What to do**: Extend the existing python check: `torch.distributed.is_nccl_available()`, `is_gloo_available()`, `torch.backends.cuda.flash_sdp_enabled()` and mem-efficient availability, `torchaudio.transforms.Resample` round-trip on CPU, `torch.profiler` CUDA activity present, a trivial `torch.compile` on CPU. In `container/archs`, diff `dir(torch.ops._C)` / `_moe_C` in the image against the `ops.def(` names in the vLLM tree's `csrc/torch_bindings.cpp` + `csrc/moe/torch_bindings.cpp`. This lands **before** any trim, so it is proven green on the current image first.

**Verify**:
```bash
./container/archs "$IMAGE:$VTAG"   # → op diff section: 0 missing on the current image
# Dockerfile assertions run at the next owner build; until then, run the same python -c via `docker run --rm --entrypoint python "$IMAGE:$VTAG" -c …` on the current image → all True
# + gate steps 1-5
```

**Commit**: `[P5][T3][trim] assert distributed/sdpa/torchaudio/compile in image`

### Wave 2 — TRIM NOW. Exits when T4–T7 are committed, host tests pass, and the owner has rebuilt (T9).

### T4: torchaudio extension flags

**File**: `container/Dockerfile:388`

**Before**:
```bash
    USE_CUDA=1 BUILD_RNNT=1 BUILD_ALIGN=1 BUILD_CUDA_CTC_DECODER=1 BUILD_CPP_TEST=0
```
**After**:
```bash
    USE_CUDA=1 BUILD_RNNT=0 BUILD_ALIGN=0 BUILD_CUDA_CTC_DECODER=0 BUILD_CPP_TEST=0
```
Add a comment naming the vLLM call sites (Key Facts) and that `USE_CUDA=1` stays for line 400.

**Verify**: gate steps 1-5; real verification is the stage's own smoke at the T9 build (`cuda_version()` prints) plus T3's `Resample` check.

**Commit**: `[P5][T4][trim] torchaudio: drop RNNT, forced-align, CUDA CTC`

### T5: torch feature flags

**File**: `container/Dockerfile` torch `ENV` block (325-331) and base apt list (~303).

**What to do**: Add `USE_FBGEMM=0 USE_MKLDNN=0 USE_NNPACK=0 USE_PYTORCH_QNNPACK=0 USE_XNNPACK=0 USE_ITT=0 USE_MPI=0 USE_NUMA=0 USE_TENSORPIPE=0 USE_NVSHMEM=0 USE_CUFILE=0 USE_CUDSS=0 USE_MAGMA=0 BUILD_LAZY_TS_BACKEND=0`, change `USE_CUSPARSELT=1` → `0`, and drop `libopenmpi-dev` from base. Keep a comment per KEEP flag with its evidence (`USE_DISTRIBUTED/GLOO/NCCL`, `USE_KINETO`, `USE_CUDNN`, `USE_FLASH_ATTENTION`, `USE_MEM_EFF_ATTENTION`). Never set `INTERN_BUILD_MOBILE`, lite interpreter, static dispatch, `USE_NATIVE_ARCH`. Removing an apt package from `base` invalidates every stage — this is the full-rebuild task; it must land in the same owner build as T4/T6/T7.

**Verify**: gate steps 1-5; at T9, the torch build log's CMake summary (`cmake/Summary.cmake` output) shows each flag OFF, and T3's asserts pass.

**Commit**: `[P5][T5][trim] torch: disable CPU-quant, MPI, RPC, cuFile and friends`

### T6: vLLM Marlin — drop int8-activation (`kS8`) kernels only

**File**: `container/patches/vllm-marlin-trim/` (new: patch script, `UPSTREAM`, stdlib-only `test_*.py`), Dockerfile `vllm-build` (apply before `setup.py bdist_wheel`, like the FA2 hook ~554-558).

**What to do**: Assert-once edit to `generate_kernels.py` (and the MoE generator if separate — confirm via `CMakeLists.txt:649,1343`) removing only `QUANT_CONFIGS` entries whose `a_type` is `["kS8"]`. **Keep every `kFE4M3fn` entry.** If the anchor text is absent (release `v0.29.0` or a future repin), print `vllm-marlin-trim: anchor not found, skipping (no trim)` and exit 0 — never fail the build, never half-apply. Check the C++ side compiles with the reduced set: the generator also writes the dispatch file, and `VLLM_MARLIN_INPUT_DTYPE=int8` must then fail with a clear error, not a segfault (confirm the existing "kernel not found" path).

**Verify**:
```bash
python3 container/patches/vllm-marlin-trim/test_vllm_marlin_trim.py    # fake generator: applies once, idempotent, skips on missing anchor
# host diff on a scratch copy of the generator output, before vs after → only *s8* files removed, fe4m3 files present
# + gate steps 1-5 (step 3 count goes 5 → 6: bump RULES.md in this commit)
```

**Commit**: `[P5][T6][trim] vllm: build Marlin without int8-activation kernels`

### T7: FlashInfer AOT — XQA page 128, FA2 MLA hd512

**File**: `container/patches/flashinfer-aot-trim/` (new: script, `UPSTREAM`, test), Dockerfile `flashinfer-build` before the provider build (~485-491).

**What to do**: Assert-once patch to `flashinfer/aot.py`: remove 128 from the XQA page-size list (~`:1071-1074`) and skip the FA2 MLA hd512 modules (~`:451-462`). Apply **only** on the `flashinfer-jit-cache-provider` path (≥ 0.7); on 0.6.x (where `flashinfer-jit-cache` itself is the AOT package, `Dockerfile:469`) print a skip line and exit 0.

**Verify**:
```bash
python3 container/patches/flashinfer-aot-trim/test_flashinfer_aot_trim.py
# + gate steps 1-5 (step 3 count 6 → 7, bump RULES.md)
```
At T9: the runtime-stage provider print shows a smaller module count than today; record both.

**Commit**: `[P5][T7][trim] flashinfer: skip XQA page-128 and FA2 MLA hd512 AOT`

### Wave 3 — OPT-IN profiles. Exits when T8 is committed; default build output is byte-for-byte the Wave 2 set.

### T8: `FI_AOT_PROFILE` and `VLLM_EXTRAS`

**File**: `container/build`, `container/Dockerfile`, the two patch scripts.

**What to do**: Both are `ARG`→`ENV` in their own stage only (never the `buildjobs` secret — they change the artifact, so they must be in the cache key). Default `full`.
- `FI_AOT_PROFILE=slim`: replace the XQA grid with explicit tuples (bf16, pages 16/32/64, groups 2/4/6/8/12/16, hd 64/128/256) or drop it; drop the `use_logits_soft_cap=True` cross product (the `add_gemma` (256, swa, cap) set stays); drop fp16-qo.
- `VLLM_EXTRAS=slim`: skip `hadacore` and `qutlass` targets; drop GPTQ-int8 Marlin (`u8b128`).
Add `ai.vllmcustom.build.profile="fi=${FI_AOT_PROFILE},vllm=${VLLM_EXTRAS}"` to the LABEL block (~677-685) so `./container/push` and gate step 5 can tell images apart.

**Verify**: gate steps 1-5; `PREFLIGHT_ONLY` dry run prints the chosen profile; host tests cover both profiles.

**Commit**: `[P5][T8][trim] add opt-in slim profiles for flashinfer AOT and vllm extras`

### Wave 4 — owner build + runtime proof.

### T9: Owner rebuild, gate step 5, step-6 smoke of every launcher

**What to do**: Owner runs `VLLM_REF=main ./container/build` (Wave 2 set, profiles `full`). Then: gate step 5 incl. `container/archs` (op diff 0 missing); T1 extractor on the new build; `tools/smoke-model` boot check on `qwen`, `muse`, `laguna`, `qwen3.8-flash-next`, `nemotron` (text, and VL with `VISION=1`), plus gpt-oss / a Gemma-class model if one is on disk; `./bench/bench` per launcher vs the pre-trim image (≥3 runs, Rule P1).

**Expected observation**:
- **Confirms if**: per-stage times drop, every launcher boots with no new runtime-JIT module outside the allowlist and no missing-op line, bench deltas within noise.
- **Refutes if**: any launcher hits a missing op/kernel → revert that trim with a new commit, record in Notes / Deviations.
- **Within noise if**: tok/s delta inside the spread — expected; a build trim should not change decode speed.

**Commit**: `[P5][T9][trim] record trimmed-build timings and launcher smoke results`

### T10: torch opt-in experiments (decide with the owner)

**What to do**: Only if T9's numbers show torch CPU-side compile still worth chasing. One at a time (P1.4), each its own owner build: `USE_CPU_VECTORIZATION=0`, `USE_SYSTEM_NCCL=1` (needs `libnccl-dev`, version-coupled with the runtime `libnccl.so.2`), `USE_PRECOMPILED_HEADERS=1`. Record each as kept / rejected with the measured delta. `USE_FLASH_ATTENTION=0` is **not** in this list: out of scope until a model-wide SDPA backend probe exists.

**Commit**: `[P5][T10][trim] <experiment>: <kept|rejected>, <delta>`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `container/Dockerfile` | torch ENV flags, torchaudio flags, base apt list, patch hooks, image asserts, profile label | P0 |
| `container/patches/vllm-marlin-trim/` | new build patch + host test | P0 |
| `container/patches/flashinfer-aot-trim/` | new build patch + host test | P0 |
| `tools/smoke-model`, `context/plans/05_jit_allowlist.txt` | coverage gate | P0 |
| `container/archs` | `torch.ops._C` diff | P1 |
| `tools/build-times` | baseline/after timing | P1 |
| `container/build` | profile plumbing | P2 |
| `context/plans/RULES.md` | step-3 test count 5 → 7 | P1 |
| `README.md` § *sm120-only* | what is no longer built and why | P1 |

---

## Open Questions / Risks

1. **Is the win worth a full rebuild?** The heavy torch cost — ATen CUDA kernels, FlashAttention (96 `.cu`), mem-efficient attention (50) — is deliberately untouched; the torch flags mostly remove CPU and third-party compile. *How we'll settle it*: T1 baseline, T9 after. If torch-build drops by less than the run-to-run noise of a cold build, say so and keep the trims only for image size/clarity.
2. **NVFP4 MoE per-token-activation path.** `fused_moe/oracle/nvfp4.py:625` passes `input_dtype=moe_config.in_dtype`; could it reach 8-bit-activation MoE Marlin? *Mitigation*: confirm in T6 before removing `kS8`; if reachable, T6 becomes `invalid` for MoE Marlin.
3. **FA2 head-dim/dtype set unknown.** The vllm-flash-attention source (`b273a02621…`) is fetched only inside the build. *deferred — FA2 is NEVER TRIM in this plan; inspect via `VLLM_FLASH_ATTN_SRC_DIR` in a later plan.*
4. **Release set coverage.** Every patch skips cleanly when its anchor is missing; T9 builds only the snapshot. *Mitigation*: host tests feed both an anchored and an anchor-less fake; a release build is a follow-up, not a blocker.
5. **`USE_MKLDNN=0` and Inductor.** All mkldnn passes are guarded by `torch._C._has_mkldnn` (`_inductor/fx_passes/*`). *Mitigation*: T3's CPU `torch.compile` assert; any failure moves MKLDNN to KEEP.
6. **Plan 01 also waits on an image rebuild** (SERIES *Currently Active*). *How we'll settle it*: the owner decides whether T9's build also carries Plan 01 T3/T10; record the decision in both trackers.
7. **Gate step 2 is now a real FAIL baseline** (shellcheck 0.11.0 installed, 19 pre-existing findings — Plan 04 tracker). New bash in this plan must add zero findings; the pre-existing ones are not this plan's.
8. **A wrong FlashInfer trim is silent** (it JITs instead of failing). *Mitigation*: T2's allowlist diff is the only thing that sees it — never skip it at T9.

---

## Verification Checklist

- [ ] T1: `tools/build-times` prints one row per stage for a real build; baseline row in tracker Evidence with build ref
- [ ] T2: `tools/smoke-model --precheck` prints the GQA group for every local model; today's `cached_ops` listing pasted in the tracker
- [ ] T3: on the current image, the new asserts all print True and `container/archs` reports 0 missing `_C`/`_moe_C` ops
- [ ] T4: at T9, torchaudio stage prints `cuda_version()`; `Resample` assert passes
- [ ] T5: at T9, torch CMake summary shows every listed flag OFF and NCCL/Gloo/Kineto/cuDNN/flash/mem-eff ON
- [ ] T6: host test passes; generated Marlin file list loses only `s8` files; at T9 `VLLM_MARLIN_INPUT_DTYPE=int8` fails with a clear message
- [ ] T7: host test passes; at T9 provider module count < pre-trim count (both recorded)
- [ ] T8: default build label reads `fi=full,vllm=full`; host tests cover `slim`
- [ ] T9: five launchers boot; zero JIT modules outside the allowlist; zero missing-op lines; bench within noise (≥3 runs); per-stage times recorded
- [ ] T10: each experiment recorded kept/rejected with a measured delta, or row `skipped` with reason
- [ ] All: the project gate (Rule A) passes on the final commit, every step reported — `skip` steps named in the tracker
- [ ] All: step 6 ran, **or** the tracker says the behavior went unverified at runtime
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
