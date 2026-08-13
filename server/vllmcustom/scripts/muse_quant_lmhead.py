#!/usr/bin/env python3
"""Quantize lm_head to weight-only FP8 (W8A16) in a compressed-tensors checkpoint.

Runs inside the vLLM image (needs torch + safetensors; compressed_tensors 0.17.0 is
present but only its *constants* matter here -- the math is 15 lines and hand-rolling it
is clearer than threading a compressor through a checkpoint that is already compressed
under a different scheme).

Why this is safe to do as a pure tensor edit, with no calibration and no GPTQ:
lm_head was EXCLUDED from the original NVFP4 run (recipe `ignore: [..., lm_head, ...]`),
so it is still bf16 in the checkpoint. Weight-only FP8 needs no activation statistics,
and symmetric per-channel absmax is data-free. Nothing about the other 51 quantized
layers is touched or re-derived.

vLLM side, all verified before writing this:
  * compressed_tensors.py::get_quant_method has an explicit `isinstance(layer,
    ParallelLMHead)` branch that binds CompressedTensorsLinearMethod when a scheme matches.
  * _is_fp8_w8a16 requires exactly: type=float, symmetric, NOT dynamic, and strategy in
    {tensor, channel, block}. This writes `channel`.
  * get_scheme_dict falls back to the top-level format only when the group omits one, so a
    per-group "float-quantized" coexists with group_0's "nvfp4-pack-quantized" and the
    top-level field must stay as it is.
  * should_ignore_layer runs FIRST, so "lm_head" has to come out of the ignore list.
  * find_matched_target matches layer name or module class -- ParallelLMHead will not match
    group_0's targets ["Linear"], so an explicit group targeting "lm_head" is required.
"""

import argparse
import json
import os
import shutil
import sys

import torch
from safetensors import safe_open
from safetensors.torch import save_file

FP8 = torch.float8_e4m3fn
FP8_MAX = 448.0  # max finite magnitude of e4m3


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("model_dir")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    d = args.model_dir
    idx_path = os.path.join(d, "model.safetensors.index.json")
    cfg_path = os.path.join(d, "config.json")
    for p in (idx_path, cfg_path):
        if not os.path.exists(p):
            print(f"!! missing {p}", file=sys.stderr)
            return 2

    idx = json.load(open(idx_path))
    wm = idx["weight_map"]
    if "lm_head.weight" not in wm:
        print("!! no lm_head.weight in the index", file=sys.stderr)
        return 2

    # --- idempotency: refuse to quantize already-quantized values -------------------
    if "lm_head.weight_scale" in wm:
        print("== lm_head.weight_scale already present -- already quantized, nothing to do")
        return 0

    shard = wm["lm_head.weight"]
    shard_path = os.path.join(d, shard)
    print(f"== lm_head.weight lives in {shard}")

    with safe_open(shard_path, framework="pt") as f:
        meta = f.metadata()
        names = list(f.keys())
        if str(f.get_slice("lm_head.weight").get_dtype()).lower().find("f8") >= 0:
            print("== lm_head.weight is already fp8 -- nothing to do")
            return 0
        tensors = {n: f.get_tensor(n) for n in names}

    w = tensors["lm_head.weight"]
    print(f"== lm_head.weight {tuple(w.shape)} {w.dtype}  {w.numel()*w.element_size()/1e9:.2f} GB")
    if w.dtype == FP8:
        print("== already fp8 -- nothing to do")
        return 0

    # --- symmetric per-output-channel absmax ----------------------------------------
    # vLLM dequantizes as (weight * weight_scale), so scale = absmax / FP8_MAX.
    wf = w.to(torch.float32)
    scale = wf.abs().amax(dim=1, keepdim=True) / FP8_MAX      # [out, 1]
    # A dead row (all-zero) would divide by zero and poison the whole tensor with NaN.
    scale = torch.clamp(scale, min=torch.finfo(torch.float32).tiny)
    wq = torch.clamp(wf / scale, -FP8_MAX, FP8_MAX).to(FP8)

    err = (wq.to(torch.float32) * scale - wf).abs()
    denom = wf.abs().mean().item()
    print(f"== quant error: mean {err.mean().item():.3e}  max {err.max().item():.3e}"
          f"  (mean |w| = {denom:.3e}, rel {err.mean().item()/denom:.4%})")
    print(f"== zero-rows: {(scale <= torch.finfo(torch.float32).tiny).sum().item()}")

    tensors["lm_head.weight"] = wq
    tensors["lm_head.weight_scale"] = scale.to(torch.float32).contiguous()

    new_bytes = sum(t.numel() * t.element_size() for t in tensors.values())
    print(f"== new shard payload {new_bytes/1e9:.2f} GB (was {os.path.getsize(shard_path)/1e9:.2f} GB)")

    if args.dry_run:
        print("== dry-run, nothing written")
        return 0

    # --- write shard to a temp in the SAME dir, then rename --------------------------
    # Rename is atomic within a filesystem, so an interrupted run cannot leave a
    # half-written shard that loads as silent garbage.
    tmp = shard_path + ".tmp"
    save_file(tensors, tmp, metadata=meta or {"format": "pt"})
    os.replace(tmp, shard_path)
    print(f"== wrote {shard}")

    # --- index ----------------------------------------------------------------------
    wm["lm_head.weight_scale"] = shard
    total = 0
    for fn in sorted(set(wm.values())):
        total += os.path.getsize(os.path.join(d, fn))
    idx.setdefault("metadata", {})["total_size"] = total
    json.dump(idx, open(idx_path, "w"), indent=2)
    print(f"== index updated (total_size {total/1e9:.2f} GB)")

    # --- config ---------------------------------------------------------------------
    shutil.copy2(cfg_path, cfg_path + ".orig")
    cfg = json.load(open(cfg_path))
    q = cfg["quantization_config"]

    before = len(q.get("ignore", []))
    q["ignore"] = [x for x in q.get("ignore", []) if x != "lm_head"]
    print(f"== ignore list {before} -> {len(q['ignore'])} (removed lm_head)")

    # A dedicated group: ParallelLMHead does not match group_0's targets ["Linear"],
    # and we want FP8 here rather than group_0's NVFP4 anyway.
    q["config_groups"]["group_1"] = {
        "format": "float-quantized",
        "targets": ["lm_head"],
        "input_activations": None,
        "output_activations": None,
        "weights": {
            "num_bits": 8,
            "type": "float",
            "symmetric": True,
            "dynamic": False,
            "strategy": "channel",
            "group_size": None,
            "block_structure": None,
            "actorder": None,
            "observer": "minmax",
            "observer_kwargs": {},
            "scale_dtype": None,
            "zp_dtype": None,
        },
    }
    json.dump(cfg, open(cfg_path, "w"), indent=2)
    print("== config.json patched (group_1 added; top-level format left alone)")
    print("\n== done. Boot with VLLM_LOGGING_LEVEL=DEBUG and REQUIRE this line:")
    print("     Using scheme: CompressedTensorsW8A16Fp8 for lm_head")
    print("   absent = silent fallback to UnquantizedLinearMethod, i.e. no gain.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
