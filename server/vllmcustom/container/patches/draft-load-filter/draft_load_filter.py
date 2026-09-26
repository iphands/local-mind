"""draft_load_filter — skip checkpoint tensors a draft model will not load,
BEFORE they are read, with NO upstream-file overlays.

Why: a speculative-decoding draft (Qwen4ExpMTP) is a second model that
vLLM's DefaultModelLoader loads from the SAME checkpoint as the target. The
default safetensors path (weight_utils.safetensors_weights_iterator) calls
f.get_tensor() on every tensor of every file, and the draft's load_weights
then throws away everything it does not map. For
nvidia/Qwen3.8-Flash-Next-NVFP4 that keeps 4.88 GiB of 123.5 GiB (4.0%), so
the draft pass re-read ~118 GiB over NFS (171 s of a 418 s load, 2026-09-26)
-- the page cache cannot help once the pinned PLE table leaves ~46 GiB free.

How: upstream already has a skip-before-read seam for EP expert filtering:
the iterator consults weight_utils.should_skip_weight(name, local_expert_ids)
before get_tensor (lazy, eager and torchao branches). Post-import this module
  * wraps weight_utils.should_skip_weight: while a filter is active, a name
    the model will not keep is skipped (never read); otherwise it delegates,
    so EP filtering is unchanged;
  * wraps DefaultModelLoader.load_weights: for a model class listed in
    FILTERS it activates that model's keep-predicate for the duration of the
    call (the checkpoint generator is consumed inside it, synchronously).
The drafter's predicate is its OWN name-remap function, imported from the
model's own module -- so the filter keeps exactly what load_weights keeps,
and follows upstream if that mapping changes. The target
(Qwen4ExpForConditionalGeneration) gets the mirror image: its load_weights
drops every "mtp." name, so those 2.5 GiB are skipped too -- applied only
while its load_weights still carries that drop rule.

Loaded from draft_load_filter.pth in the venv's site-packages (every
interpreter, including the spawn'd EngineCore; before sitecustomize, so
independent of boot-timing). DRAFT_LOAD_FILTER=0 disables it.

Design rules (same as container/patches/boot-timing):
  * No overlay of any upstream file -> nothing to re-derive on snapshot bumps.
    A renamed/removed target degrades to a "NOT applied" note; the load then
    reads everything, exactly as upstream does.
  * Every hook path is guarded. A broken filter must not break the engine.
  * stdlib-only at import time: vLLM modules are touched only once imported.
"""

from __future__ import annotations

import functools
import inspect
import os
import sys
import time
import types
from importlib.abc import Loader
from typing import Any, Callable

ENV = "DRAFT_LOAD_FILTER"
WEIGHT_UTILS = "vllm.model_executor.model_loader.weight_utils"
DEFAULT_LOADER = "vllm.model_executor.model_loader.default_loader"
TARGETS = (WEIGHT_UTILS, DEFAULT_LOADER)

# model class name -> how to know which checkpoint tensors its load_weights keeps:
#   "fn:<name>"      a function in THAT CLASS'S OWN MODULE mapping a checkpoint
#                    name to the loaded name, or None when load_weights drops
#                    it. keep(name) = fn(name) is not None.
#   "drop:<substr>"  load_weights maps every name containing <substr> to None
#                    (WeightsMapper orig_to_new_substr={"<substr>": None}, which
#                    is checked before any prefix rewrite). keep(name) = substr
#                    not in name. Applied only while that literal is still in the
#                    class's load_weights source -- the model's own statement
#                    that it drops those tensors.
FILTERS: dict[str, str] = {
    # the MTP drafter keeps 4.9 of 123.5 GiB (the big win, ~160 s over NFS)
    "Qwen4ExpMTP": "fn:_remap_mtp_weight_name",
    # the target drops the drafter's 2.5 GiB of mtp.* (~4.7 s with MTP off;
    # ~a wash with MTP on, where that read warmed the page cache for the drafter)
    "Qwen4ExpForConditionalGeneration": "drop:mtp.",
    "Qwen4ExpForCausalLM": "drop:mtp.",
}

_MARK = "__draft_load_filter__"

_active: Callable[[str], bool] | None = None
_stats = {"kept": 0, "skipped": 0}
_applied: set[str] = set()
_noted: set[str] = set()


def _log(msg: str) -> None:
    print(f"[draft-load-filter] {msg}", file=sys.stderr, flush=True)


def _note_once(key: str, msg: str) -> None:
    if key not in _noted:
        _noted.add(key)
        _log(msg)


# ---------------------------------------------------------------- wrappers


def _wrap_should_skip(orig: Callable[..., bool]) -> Callable[..., bool]:
    @functools.wraps(orig)
    def should_skip_weight(weight_name: str, *args: Any, **kwargs: Any) -> bool:
        keep = _active
        if keep is not None:
            try:
                if not keep(weight_name):
                    _stats["skipped"] += 1
                    return True
                _stats["kept"] += 1
            except Exception:
                pass  # unsure -> read it, exactly as upstream would
        return orig(weight_name, *args, **kwargs)

    setattr(should_skip_weight, _MARK, True)
    return should_skip_weight


def _keep_for(model: Any) -> Callable[[str], bool] | None:
    cls = type(model)
    rule = FILTERS.get(cls.__name__)
    if rule is None:
        return None
    kind, _, arg = rule.partition(":")
    if kind == "fn":
        fn = getattr(sys.modules.get(cls.__module__), arg, None)
        if not callable(fn):
            _note_once(
                f"fn:{cls.__module__}.{arg}",
                f"NOT applied for {cls.__name__}: {cls.__module__}.{arg} not found "
                "(upstream renamed it?) -- loading reads the whole checkpoint",
            )
            return None
        return lambda name: fn(name) is not None
    if kind == "drop":
        literal = f'"{arg}": None'
        try:
            src = inspect.getsource(cls.load_weights)
        except (OSError, TypeError):
            src = ""
        if literal not in src:
            _note_once(
                f"drop:{cls.__module__}.{cls.__name__}",
                f"NOT applied for {cls.__name__}: its load_weights no longer says "
                f"{literal} -- loading reads the whole checkpoint",
            )
            return None
        return lambda name: arg not in name
    _note_once(f"rule:{rule}", f"NOT applied for {cls.__name__}: bad FILTERS rule {rule!r}")
    return None


def _wrap_load_weights(orig: Callable[..., Any]) -> Callable[..., Any]:
    @functools.wraps(orig)
    def load_weights(self: Any, model: Any, *args: Any, **kwargs: Any) -> Any:
        global _active
        try:
            keep = _keep_for(model)
        except Exception:
            keep = None
        if keep is None:
            return orig(self, model, *args, **kwargs)
        if WEIGHT_UTILS not in _applied:
            _note_once(
                "skip-hook",
                f"NOT applied for {type(model).__name__}: "
                f"{WEIGHT_UTILS}.should_skip_weight was not hooked",
            )
            return orig(self, model, *args, **kwargs)

        prev, _active = _active, keep
        _stats["kept"] = _stats["skipped"] = 0
        t0 = time.monotonic()
        try:
            return orig(self, model, *args, **kwargs)
        finally:
            _active = prev
            kept, skipped = _stats["kept"], _stats["skipped"]
            name = type(model).__name__
            if kept == 0 and skipped == 0:
                _log(
                    f"{name}: filter was active but the weight iterator never "
                    "consulted should_skip_weight (non-default load format, or "
                    "upstream changed the iterator) -- nothing was skipped"
                )
            else:
                _log(
                    f"{name}: read {kept} checkpoint tensors, skipped {skipped} "
                    f"before reading them ({time.monotonic() - t0:.1f} s)"
                )

    setattr(load_weights, _MARK, True)
    return load_weights


# ---------------------------------------------------------------- hooking


def _apply(module: types.ModuleType) -> None:
    name = module.__name__
    try:
        if name == WEIGHT_UTILS:
            orig = getattr(module, "should_skip_weight", None)
            if callable(orig) and not getattr(orig, _MARK, False):
                module.should_skip_weight = _wrap_should_skip(orig)  # type: ignore[attr-defined]
                _applied.add(name)
            elif not callable(orig):
                _note_once(name, f"NOT applied: {name}.should_skip_weight not found")
        elif name == DEFAULT_LOADER:
            cls = getattr(module, "DefaultModelLoader", None)
            orig = getattr(cls, "load_weights", None)
            if callable(orig) and not getattr(orig, _MARK, False):
                cls.load_weights = _wrap_load_weights(orig)  # type: ignore[union-attr]
                _applied.add(name)
            elif not callable(orig):
                _note_once(name, f"NOT applied: {name}.DefaultModelLoader.load_weights not found")
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
    """Meta-path finder: only intercepts TARGETS, wraps their real loader and
    applies the hooks right after the module executes. Skips itself while
    delegating, so it chains with boot-timing's finder in either order."""

    def find_spec(self, fullname: str, path: Any = None, target: Any = None) -> Any:
        if fullname not in TARGETS:
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
    """Called from draft_load_filter.pth at interpreter start."""
    if os.environ.get(ENV, "1") == "0":
        return
    if any(isinstance(f, _HookFinder) for f in sys.meta_path):
        return
    sys.meta_path.insert(0, _HookFinder())
    # defensive: a target imported before us (should not happen from a .pth)
    for mod_name in TARGETS:
        mod = sys.modules.get(mod_name)
        if mod is not None:
            _apply(mod)
