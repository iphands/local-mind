"""csf_scale_decode — turn NVFP4-CSF expert scales back into plain E4M3
`weight_scale` tensors in the weight stream, BEFORE vLLM's loader sees them,
with NO upstream-file overlays.

Why: nvidia/Qwen3.8-Flash-Next-FP4-Local stores each routed-expert block-scale
matrix as two tensors, `<X>.weight_scale.nvfp4_csf_fixed` (uint8) and
`<X>.weight_scale.nvfp4_csf_exceptions` (uint32) -- a lossless compression of the
float8_e4m3fn scales that community vLLM/b12x forks read in place with CUDA
kernels. Our image has no reader (the first MoE layer dies with "has no parameter
'w2_weight_scale.nvfp4_csf_exceptions'"), and our HUMMING/Marlin NVFP4 MoE only
wants ordinary scales. So decode once, on the CPU, in the weight iterator.

Codec (b12x tests/quantization/test_nvfp4_csf.py, codec 0, "byte-window4"):
  rows x cols logical E4M3 byte matrix (cols = K/16). Per 16-row slab the fixed
  stream is 16 row-base bytes then rows(16) x cols/2 nibble bytes, low nibble =
  even column. scale = base[row] + nibble. Each exception word is
  `position | value << 24` (position = row*cols + col, sorted, unique) and
  overwrites the decoded byte.

How: wrap DefaultModelLoader.get_all_weights (a generator over the checkpoint's
(name, tensor) stream). Every tensor passes through untouched except the two
CSF suffixes, which are buffered until their pair is complete (a safetensors
shard lists all U32 tensors before the U8 ones) and replaced by one
`(<X>.weight_scale, float8_e4m3fn[rows, cols])`. rows/cols come from the text
config and the projection in the name, and are cross-checked against the fixed
stream's length.

Loaded from csf_scale_decode.pth in the venv's site-packages (every interpreter,
including the spawn'd EngineCore). CSF_SCALE_DECODE=0 disables.

Design rules (same as draft-load-filter / boot-timing):
  * No overlay of any upstream file. A renamed target degrades to a
    "NOT applied" line and vLLM then fails exactly as it does without us.
  * A wrong scale is worse than no boot: any inconsistency RAISES.
  * stdlib-only at import time; torch is imported on the first decode.
"""

from __future__ import annotations

import functools
import os
import sys
import time
import types
from importlib.abc import Loader
from typing import Any, Callable, Iterator

ENV = "CSF_SCALE_DECODE"
DEFAULT_LOADER = "vllm.model_executor.model_loader.default_loader"
TARGETS = (DEFAULT_LOADER,)

SUFFIX_FIXED = ".nvfp4_csf_fixed"
SUFFIX_EXC = ".nvfp4_csf_exceptions"
_MARK = "__csf_scale_decode__"
_MAX_BASE = 240  # base + 15 must still fit a byte

_applied: set[str] = set()
_noted: set[str] = set()


def _log(msg: str) -> None:
    print(f"[csf-scale-decode] {msg}", file=sys.stderr, flush=True)


def _note_once(key: str, msg: str) -> None:
    if key not in _noted:
        _noted.add(key)
        _log(msg)


# ---------------------------------------------------------------- codec


def exception_fields(word: int) -> tuple[int, int]:
    return word & 0xFFFFFF, word >> 24


def decode_reference(fixed: bytes, words: list[int], rows: int, cols: int) -> bytearray:
    """Pure-Python decode (the spec; slow). Returns rows*cols E4M3 bytes."""
    if rows % 16 or cols % 2 or len(fixed) != rows * (1 + cols // 2):
        raise ValueError(f"CSF fixed stream is {len(fixed)} bytes, not a {rows}x{cols} plane")
    out = bytearray(rows * cols)
    stride = 16 * (1 + cols // 2)
    for slab in range(rows // 16):
        s = slab * stride
        bases = fixed[s : s + 16]
        if max(bases) > _MAX_BASE:
            raise ValueError("CSF row base above 240")
        for i in range(16):
            row = slab * 16 + i
            packed = fixed[s + 16 + i * (cols // 2) : s + 16 + (i + 1) * (cols // 2)]
            o = row * cols
            for j, b in enumerate(packed):
                out[o + 2 * j] = bases[i] + (b & 15)
                out[o + 2 * j + 1] = bases[i] + (b >> 4)
    last = -1
    for w in words:
        pos, val = exception_fields(w)
        if pos <= last or pos >= rows * cols:
            raise ValueError("CSF exception positions must be sorted, unique and in range")
        out[pos] = val
        last = pos
    return out


class ReferenceBackend:
    """Tensors are plain `bytes` (fixed) and `list[int]` (exception words)."""

    @staticmethod
    def numel(t: Any) -> int:
        return len(t)

    @staticmethod
    def decode(fixed: Any, exc: Any, rows: int, cols: int) -> Any:
        return decode_reference(bytes(fixed), list(exc), rows, cols)


class TorchBackend:
    """The production path: same arithmetic, vectorised on the CPU."""

    @staticmethod
    def numel(t: Any) -> int:
        return int(t.numel())

    @staticmethod
    def decode(fixed: Any, exc: Any, rows: int, cols: int) -> Any:
        import torch

        if fixed.dtype != torch.uint8:
            raise TypeError(f"CSF fixed stream is {fixed.dtype}, want uint8")
        if exc.dtype not in (torch.uint32, torch.int32):
            raise TypeError(f"CSF exceptions are {exc.dtype}, want uint32")
        if rows % 16 or cols % 2 or fixed.numel() != rows * (1 + cols // 2):
            raise ValueError(
                f"CSF fixed stream has {fixed.numel()} bytes, not a {rows}x{cols} plane"
            )
        s = fixed.reshape(rows // 16, -1)
        bases = s[:, :16].reshape(rows, 1)
        if int(bases.max()) > _MAX_BASE:
            raise ValueError("CSF row base above 240")
        packed = s[:, 16:].reshape(rows, cols // 2)
        out = torch.empty((rows, cols), dtype=torch.uint8)
        out[:, 0::2] = packed & 15
        out[:, 1::2] = packed >> 4
        out += bases
        if exc.numel():
            e = exc.view(torch.int32).to(torch.int64) & 0xFFFFFFFF
            pos = e & 0xFFFFFF
            if int(pos[-1]) >= rows * cols or bool((pos[1:] <= pos[:-1]).any()):
                raise ValueError("CSF exception positions must be sorted, unique and in range")
            out.view(-1)[pos] = (e >> 24).to(torch.uint8)
        return out.view(torch.float8_e4m3fn)


_backend: Any = TorchBackend


def _plane_dims(base: str, numel: int, hidden: int, inter: int) -> tuple[int, int]:
    """(rows, cols) of the scale plane `base` (…<proj>.weight_scale)."""
    parts = base.split(".")
    proj = parts[-2] if len(parts) >= 2 else ""
    if proj in ("gate_proj", "up_proj", "w1", "w3"):
        cand = [(inter, hidden // 16)]
    elif proj in ("down_proj", "w2"):
        cand = [(hidden, inter // 16)]
    else:
        cand = [(inter, hidden // 16), (hidden, inter // 16)]
    ok = [(r, c) for r, c in cand if numel == r * (1 + c // 2)]
    if len(set(ok)) != 1:
        raise ValueError(
            f"CSF {base}: {numel}-byte fixed stream fits no expected plane "
            f"(hidden={hidden}, moe_intermediate={inter}, candidates={cand})"
        )
    return ok[0]


def _text_dims(model_config: Any) -> tuple[int, int]:
    for attr in ("hf_text_config", "hf_config"):
        cfg = getattr(model_config, attr, None)
        cfg = getattr(cfg, "text_config", cfg)
        hidden = getattr(cfg, "hidden_size", None)
        inter = getattr(cfg, "moe_intermediate_size", None)
        if hidden and inter:
            return int(hidden), int(inter)
    raise ValueError("CSF decode needs hidden_size and moe_intermediate_size in the model config")


def decode_stream(stream: Iterator[tuple[str, Any]], model_config: Any) -> Iterator[tuple[str, Any]]:
    """Pass the stream through, replacing each CSF pair with its decoded scale."""
    backend = _backend
    fixed: dict[str, Any] = {}
    exc: dict[str, Any] = {}
    dims: tuple[int, int] | None = None
    count, t0 = 0, time.monotonic()
    try:
        for name, tensor in stream:
            if name.endswith(SUFFIX_FIXED):
                base, mine, other = name[: -len(SUFFIX_FIXED)], fixed, exc
            elif name.endswith(SUFFIX_EXC):
                base, mine, other = name[: -len(SUFFIX_EXC)], exc, fixed
            else:
                yield name, tensor
                continue
            if base in mine:
                raise ValueError(f"CSF tensor seen twice: {name}")
            if base not in other:
                mine[base] = tensor
                continue
            partner = other.pop(base)
            f, e = (tensor, partner) if mine is fixed else (partner, tensor)
            if dims is None:
                dims = _text_dims(model_config)
            rows, cols = _plane_dims(base, backend.numel(f), dims[0], dims[1])
            yield base, backend.decode(f, e, rows, cols)
            count += 1
        if fixed or exc:
            left = sorted(list(fixed) + list(exc))[:3]
            raise ValueError(f"CSF tensors without their pair at end of stream, e.g. {left}")
    finally:
        if count:
            _log(f"decoded {count} compressed expert scale planes in {time.monotonic() - t0:.1f} s")


# ---------------------------------------------------------------- hooking


def _wrap_get_all_weights(orig: Callable[..., Any]) -> Callable[..., Any]:
    @functools.wraps(orig)
    def get_all_weights(self: Any, model_config: Any, *args: Any, **kwargs: Any) -> Any:
        return decode_stream(orig(self, model_config, *args, **kwargs), model_config)

    setattr(get_all_weights, _MARK, True)
    return get_all_weights


def _apply(module: types.ModuleType) -> None:
    name = module.__name__
    try:
        if name == DEFAULT_LOADER:
            cls = getattr(module, "DefaultModelLoader", None)
            orig = getattr(cls, "get_all_weights", None)
            if callable(orig) and not getattr(orig, _MARK, False):
                cls.get_all_weights = _wrap_get_all_weights(orig)  # type: ignore[union-attr]
                _applied.add(name)
            elif not callable(orig):
                _note_once(
                    name,
                    f"NOT applied: {name}.DefaultModelLoader.get_all_weights not found "
                    "(upstream renamed it?) -- CSF checkpoints will fail to load",
                )
    except Exception as e:
        _note_once(name, f"NOT applied to {name}: {e!r}")


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
    """Intercepts TARGETS only. draft-load-filter hooks the same module with its
    own finder; the re-entrancy guard keeps the two from recursing into each
    other whichever one sits first in sys.meta_path."""

    _busy = False

    def find_spec(self, fullname: str, path: Any = None, target: Any = None) -> Any:
        if fullname not in TARGETS or _HookFinder._busy:
            return None
        _HookFinder._busy = True
        try:
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
        finally:
            _HookFinder._busy = False


def install() -> None:
    """Called from csf_scale_decode.pth at interpreter start."""
    if os.environ.get(ENV, "1") == "0":
        return
    if any(isinstance(f, _HookFinder) for f in sys.meta_path):
        return
    sys.meta_path.insert(0, _HookFinder())
    for mod_name in TARGETS:  # defensive: imported before us (should not happen from a .pth)
        mod = sys.modules.get(mod_name)
        if mod is not None:
            _apply(mod)
