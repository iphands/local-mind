# Plan 04 — Decode NVFP4-CSF expert scales at load time

> **Status**: in-progress
> **Created**: 2026-10-15
> **Depends on**: N/A
> **Goal**: `nvidia/Qwen3.8-Flash-Next-FP4-Local` boots on our source-built image, via a runtime patch that turns the checkpoint's compressed expert scales back into ordinary E4M3 `weight_scale` tensors before vLLM's own loader sees them.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: A new PYTHONPATH/`.pth` patch, `container/patches/csf-scale-decode/`, wraps `DefaultModelLoader.get_all_weights`, pairs each `<X>.weight_scale.nvfp4_csf_fixed` with its `…_exceptions`, and yields the lossless-decoded `<X>.weight_scale` (float8_e4m3fn) instead.

**Deliverables**:
1. `csf_scale_decode.py` + `.pth` + `UPSTREAM` + stdlib-only `test_csf_scale_decode.py` (fake seam, every degradation path).
2. A CPU-only check on the real shards through the real `DefaultModelLoader` seam, in the image (no GPU).
3. `run-fp4local` mounts the patch (`CSF_SCALE_DECODE`, default on); README + `context/` updated; gate counts bumped.
4. Owner-run boot (gate step 6) — recorded, or recorded as not run.

**Estimated effort**: Small–Medium (half day) plus one owner boot.

---

## Scope

**In**: `container/patches/csf-scale-decode/`, the one launcher that serves this checkpoint, README/context/gate text.
**Out**: any b12x kernel, any change to vLLM source or the pins, the other four launchers. The compressed-scale *kernels* (b12x reading CSF in place) are not ported — we decode once to plain E4M3, so the resident weights are the same size as any NVFP4 checkpoint. If the decoded scales do not boot on `HUMMING`/Marlin, stop and record a blocker.

---

## Context

The checkpoint's README (line 20) says its NVFP4 scale factors use a new lossless compression and "upstream vLLM support has not yet landed". Our image has no `csf` string anywhere, so the first MoE layer fails to load. Two community projects implement the format: `local-inference-lab/vllm` (reader: `vllm/model_executor/layers/quantization/nvfp4_csf.py`, `utils/nvfp4_csf_utils.py`, `model_loader/csf_utils.py`, PR #1002) and `local-inference-lab/b12x` (GPU decode kernels; the CPU reference encoder is `tests/quantization/test_nvfp4_csf.py::_fixture`, codec 0 = `byte-window4-fixed-stream-u24-exceptions/1`). Both bind the format to b12x kernels; neither exposes a plain "decode to E4M3" path, which is all our `HUMMING`/Marlin NVFP4 MoE needs.

### Pre-Identified Bug/Issue

```
$ ./qwen3.8-flash-next/run-fp4local            # 2026-10-16 03:18 boot, shards now found
(EngineCore pid=240) ERROR ... AttributeError: Layer language_model.model.layers.0.mlp.experts has no parameter 'w2_weight_scale.nvfp4_csf_exceptions' for checkpoint weight 'language_model.model.layers.0.mlp.experts.0.down_proj.weight_scale.nvfp4_csf_exceptions'
```

### Why decode in the weight stream

Rejected: porting the b12x/vLLM CSF reader (needs the b12x CUTLASS-DSL kernels, a vLLM fork's MoE method, and PP1/DP1 constraints — a repin-sized change). Rejected: an offline converter writing a standard checkpoint (≈100 GB more NFS, and a second copy to keep in step). A stream wrapper rides the same seam as `draft-load-filter`, adds nothing to the image, and degrades to "vLLM fails exactly as today" if the seam moves.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| Codec | per 16-row slab: 16 base bytes, then `rows×cols/2` nibble bytes (low nibble = even column); scale = `base[row] + nibble`; then each exception word `pos \| value<<24` (pos = row·cols+col, row-major, sorted unique) overwrites | b12x `tests/quantization/test_nvfp4_csf.py` `_fixture` codec 0; `nvfp4_csf_utils.py:_slice_scale_plane` (read 2026-10-15) |
| Tensor shapes | fixed `U8 [rows/16, 16·(1+cols/2)]`, exceptions `U32 [n]`; gate/up rows=640 cols=160, down rows=2560 cols=40 | safetensors header, shard 1, 2026-10-15 |
| Real data decodes sanely | bases 89–104, no 0x7f/0xff, positions sorted and in range, 0.2–1.0 % exceptions | probe in the image on layer 0 expert 0, 2026-10-15 |
| Only routed-expert scales are compressed | 913 pairs in shard 1; MTP/vision/PLE/shared stay plain | model card (HF `Qwen3.8-Flash-Next-NVFP4-MXFP8-CSF-QAD`), config `quantized_layers[…experts].weight_scale_encoding=csf` |
| Seam | `DefaultModelLoader.get_all_weights(self, model_config, model)` is a generator over `_get_weights_iterator` | `inspect.getsource` in image `…g4d9a8d6`, 2026-10-15 |
| U32 reads as `torch.uint32`; `.view(torch.int32)` works | | same probe |
| Nibble order and the decode as a whole are **unverified against ground truth** | only the b12x fixture vouches for it; the boot's output quality is the real test | unconfirmed — settle in T4 |

### Rejected Claims

- "The `tensors/` subdirectory / index paths are the bug" — already fixed on disk (top-level symlinks + bare-name index, `model.safetensors.index.json.orig` kept); not a repo change.

---

## Step-by-Step Tasks

### T1: The patch and its host test

**File**: `container/patches/csf-scale-decode/{csf_scale_decode.py,csf_scale_decode.pth,UPSTREAM,test_csf_scale_decode.py}`

**What to do**: `install()` registers a meta-path hook (same shape as `draft_load_filter`) on `vllm.model_executor.model_loader.default_loader`, wrapping `DefaultModelLoader.get_all_weights`. The wrapper passes every `(name, tensor)` through unchanged except `*.nvfp4_csf_fixed` / `*.nvfp4_csf_exceptions`, which it buffers until the pair is complete, then yields `(name minus suffix, decoded float8_e4m3fn[rows, cols])`. `rows`/`cols` come from `model_config.hf_text_config` (`hidden_size`, `moe_intermediate_size`) and the projection in the name, cross-checked against `fixed.numel() == rows·(1+cols/2)`; any mismatch, unpaired tensor at end of stream, unsorted/out-of-range exception or base > 240 raises (a silently wrong scale is worse than no boot). `CSF_SCALE_DECODE=0` disables. A target that vanished prints `NOT applied` and changes nothing. Logs one summary line (count, seconds).

**Verify**:
```bash
python3 container/patches/csf-scale-decode/test_csf_scale_decode.py   # → ALL TESTS PASS
```
Expected: tests cover decode vs an independent encoder, exception override, interleaved order (exceptions-before-fixed across a shard), bad base, unpaired tensor, shape mismatch, disabled env, missing target, double install.

**Commit**: `[P4][T1][csf-decode] add CSF expert-scale decode patch and test`

### T2: Real shards through the real seam (CPU, in the image)

**File**: tracker evidence only (probe script lives in the scratchpad).

**What to do**: in the image, with `-v models:ro` and the patch mounted, call `DefaultModelLoader.get_all_weights` on the real checkpoint with a stand-in `model` and `model_config`, take the first shard's worth of yielded names, and assert: no `csf` name leaks; every decoded `weight_scale` is `float8_e4m3fn` with the expected shape; values equal the independent probe's numbers.

**Expected observation**: *Confirms if* every decoded tensor has the right dtype/shape and spot-values match the 2026-10-15 probe. *Refutes if* a leaked `csf` name, a raise, or a shape mismatch. Wall time is recorded, not judged.

**Commit**: `[P4][T2][csf-decode] record real-shard seam check`

### T3: Wire the launcher, docs and gate counts

**File**: `qwen3.8-flash-next/run-fp4local`, `README.md`, `context/distilled.md`, `context/pitfalls.md`. (The gate-count bump in `RULES.md` moved into T1: the count changes in the commit that adds the files.)

**What to do**: mount the patch dir + `.pth` as `draft-load-filter` is mounted, behind `CSF_SCALE_DECODE=${:-1}`, and add it to the `## Running` line. Record the format facts (distilled) and the `tensors/`-layout trap (pitfalls). `run-fp4local` is untracked in this tree; the edit is made but **not committed** (it is the owner's file) — recorded in the tracker.

**Verify**: `bash -n qwen3.8-flash-next/run-fp4local`; gate steps 1–5.

**Commit**: `[P4][T3][csf-decode] document patch, bump gate counts`

### T4: Owner boot (gate step 6)

**What to do**: owner runs `./qwen3.8-flash-next/run-fp4local`; the log must show `[csf-scale-decode] decoded N …`, no `hooks NOT applied`, then a chat completion that is coherent. Not run by the agent.

**Expected observation**: *Confirms if* the engine finishes loading and generates coherent text. *Refutes if* garbage output (suspect nibble order or an exceptions bug before anything else) or a different loader error (new blocker, record it).

**Commit**: `[P4][T4][csf-decode] record boot result`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `container/patches/csf-scale-decode/csf_scale_decode.py` | the decoder + hook | P0 |
| `container/patches/csf-scale-decode/test_csf_scale_decode.py` | host test | P0 |
| `qwen3.8-flash-next/run-fp4local` | mount (uncommitted, owner's file) | P1 |
| `context/plans/RULES.md` | gate counts | P1 |

---

## Open Questions / Risks

1. **Is the nibble/exception reading right?** Only the b12x fixture vouches. *How we'll settle it*: T4 output coherence; T2 cannot.
2. **Do other tensors differ?** The community loader also casts 100 FP32 norm tensors to BF16 and the model card lists a quantized vision tower. *How we'll settle it*: T4; a new loader error is a recorded blocker, not scope creep.
3. **Load time.** ~73k small CPU decodes on top of a 95 GiB NFS read. *How we'll settle it*: T2 measures decode cost per shard.
4. **Risk: the seam moves on a repin.** *Mitigation*: `NOT applied` line + host test + `UPSTREAM` hook table.

---

## Verification Checklist

- [x] T1: `python3 container/patches/csf-scale-decode/test_csf_scale_decode.py` → `ALL TESTS PASS`
- [ ] T2: real-shard seam run prints the expected dtype/shape for every decoded tensor and no `csf` name; numbers in the tracker
- [ ] T3: `bash -n` clean; gate steps 1, 3 report the new counts
- [ ] T4: owner boot log shows the decode line and a coherent completion — or the tracker says it did not run
- [ ] All: the project gate passes on the final commit, skips named
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker moved to `completed/`, `SERIES.md` marked done (Rule C)
