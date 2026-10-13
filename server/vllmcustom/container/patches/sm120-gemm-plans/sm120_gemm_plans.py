"""sm120_gemm_plans — give Qwen4Exp's CuTe-DSL skinny-GEMM path a plan table on SM120
(RTX PRO 6000 Blackwell), with NO upstream-file overlays.

Why: vllm/models/qwen4_exp/nvidia/low_latency_gemm.py replaces F.linear with a
shape-specialised CuTe-DSL skinny GEMM for the BF16 projections and the lm_head, but
its plan tables are keyed by GPU capability and exist only for SM103, SM100, SM90 and
SM121 (QWEN4_EXP_GEMM_PLANS_BY_CAPABILITY). On SM120 enable_qwen4_exp_low_latency_gemm()
returns at `not plans`, so every BF16 dense layer and the 248320-row lm_head runs as plain
torch.nn.functional.linear. In nvidia/Qwen3.8-Flash-Next-NVFP4 those BF16 layers are most
of the weight bytes read per token (see qwen3.8-flash-next/run, "ONLINE_FP8").
vllm#59753 added SM121 TP=1 plans for exactly this model's shapes (lm_head 248320x2560,
GDN 16384x2560, QSA 13952x2560, HC 10240x320, shared expert, router ...). SM120 and SM121
are the same consumer-Blackwell family and the kernel has no arch gate
(skinny_gemm.ShapeDynamicSkinnyGemm.is_available only imports cutlass.cute).

How: post-import, set QWEN4_EXP_GEMM_PLANS_BY_CAPABILITY[(12, 0)] to the SM121 table.
Both lookups (enable_qwen4_exp_low_latency_gemm at model init, and the custom op at
runtime) read that module-level dict by capability, so nothing else is touched.
Caveat: the SM121 tile choices were tuned on GB10 (273 GB/s LPDDR); on a 1.8 TB/s GDDR7
card they may be suboptimal, though likely still ahead of cuBLAS at M <= 16. UNVERIFIED on
the GPU; A/B it (SM120_SKINNY=1 in the launcher).

Loaded from sm120_gemm_plans.pth in the venv's site-packages (every interpreter,
including the spawn'd EngineCore / worker). Only installs when SM120_SKINNY=1.

Design rules (same as container/patches/spec-live, boot-timing, draft-load-filter):
  * No overlay of any upstream file; a renamed/removed target degrades to a note and the
    server runs exactly as upstream does.
  * Every hook path is guarded. stdlib-only at import time.
"""

import os
import sys
import types
from importlib.abc import Loader
from typing import Any

ENV = "SM120_SKINNY"  # "1" enables; the launcher sets it
TARGET = "vllm.models.qwen4_exp.nvidia.low_latency_gemm"
SOURCE_CAPABILITY = (12, 1)  # the table that is copied (SM121, TP=1 shapes included)
CAPABILITY = (12, 0)  # SM120

_applied: set[str] = set()
_noted: set[str] = set()


def _log(msg: str) -> None:
    print(f"[sm120-gemm-plans] {msg}", file=sys.stderr, flush=True)


def _note_once(key: str, msg: str) -> None:
    if key not in _noted:
        _noted.add(key)
        _log(msg)


def _apply(module: types.ModuleType) -> None:
    if module.__name__ != TARGET:
        return
    try:
        plans = getattr(module, "QWEN4_EXP_GEMM_PLANS_BY_CAPABILITY", None)
        if not isinstance(plans, dict):
            _note_once("table", f"NOT applied: {TARGET}.QWEN4_EXP_GEMM_PLANS_BY_CAPABILITY not found")
            return
        source = plans.get(SOURCE_CAPABILITY)
        if not isinstance(source, dict) or not source:
            _note_once("source", f"NOT applied: no plan table for capability {SOURCE_CAPABILITY} to copy")
            return
        if CAPABILITY in plans:
            _note_once("present", f"NOT applied: capability {CAPABILITY} already has a plan table upstream")
            return
        plans[CAPABILITY] = source
        _applied.add(TARGET)
        _log(f"SM120 skinny-GEMM plans on: reusing the SM121 table ({len(source)} shapes)")
    except Exception as e:
        _note_once("apply", f"NOT applied: {e!r}")


class _HookLoader(Loader):
    def __init__(self, inner: Any) -> None:
        self._inner = inner

    def create_module(self, spec: Any) -> Any:
        return self._inner.create_module(spec)

    def exec_module(self, module: types.ModuleType) -> None:
        self._inner.exec_module(module)
        _apply(module)

    def __getattr__(self, name: str) -> Any:
        return getattr(self._inner, name)


class _HookFinder:
    """Meta-path finder: only intercepts TARGET, wraps its real loader and applies the
    hook right after the module executes. Skips itself while delegating, so it chains
    with the other overlays' finders in either order."""

    def find_spec(self, fullname: str, path: Any = None, target: Any = None) -> Any:
        if fullname != TARGET:
            return None
        for finder in sys.meta_path:
            if isinstance(finder, _HookFinder):
                continue
            find_spec = getattr(finder, "find_spec", None)
            if find_spec is None:
                continue
            try:
                spec = find_spec(fullname, path, target)
            except Exception:
                continue
            if spec is not None and spec.loader is not None:
                spec.loader = _HookLoader(spec.loader)
                return spec
        return None


def install() -> None:
    """Called from sm120_gemm_plans.pth at interpreter start."""
    if os.environ.get(ENV, "0") != "1":
        return
    if any(isinstance(f, _HookFinder) for f in sys.meta_path):
        return
    sys.meta_path.insert(0, _HookFinder())
    mod = sys.modules.get(TARGET)  # defensive: imported before us (not from a .pth)
    if mod is not None:
        _apply(mod)
