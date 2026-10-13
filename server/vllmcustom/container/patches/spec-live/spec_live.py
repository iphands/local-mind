"""spec_live — change the MTP draft depth of a RUNNING vLLM server over HTTP,
with NO upstream-file overlays.

Why: --speculative-config num_speculative_tokens is read once at engine init
(scheduler, CUDA-graph capture, QSA key ring), so tuning it costs a full restart
(~9 min on nvidia/Qwen3.8-Flash-Next-NVFP4). Nothing upstream sets it live.

How: vLLM's Dynamic SD (vllm#32374) already lets the scheduler pick the draft
depth per step BELOW the configured maximum: Scheduler.schedule() computes
num_spec_tokens_to_schedule from `self.dynamic_sd_lookup[batch_size]`, puts it in
SchedulerOutput, and the V2 worker obeys it (propose(num_speculative_tokens=...)).
Every allocation is sized for the maximum, so any K in 0..max is memory-safe. The
only thing missing is a MUTABLE source of K instead of the batch-size table.
Post-import this module
  * wraps Scheduler.__init__: when the scheduler runs Dynamic SD it swaps
    `dynamic_sd_lookup` for a list subclass that returns one live K for every
    batch size (scheduler.py needs no change);
  * adds EngineCore.set_spec_tokens / get_spec_tokens. The API process reaches
    them through the generic utility RPC (EngineCore dispatches UTILITY requests
    with getattr(self, name), the same path as reset_prefix_cache), and they run
    on the engine loop between steps, so no locking is needed;
  * wraps register_api_routers to add GET/POST /spec_tokens.
The launcher (qwen3.8-flash-next/run, SPEC_LIVE=1) supplies the boot-time
num_speculative_tokens_per_batch_size that turns Dynamic SD on and makes the
workers capture K-specialised CUDA graphs; at runtime that table is ignored.
A K with no captured graph still works (the dispatcher falls back to PIECEWISE).

Loaded from spec_live.pth in the venv's site-packages (every interpreter,
including the spawn'd EngineCore). Only installs when SPEC_LIVE=1.

Design rules (same as container/patches/boot-timing and draft-load-filter):
  * No overlay of any upstream file -> nothing to re-derive on snapshot bumps.
    A renamed/removed target degrades to a "NOT applied" note and the server
    runs with its static depth, exactly as upstream does.
  * Every hook path is guarded. A broken hook must not break the engine.
  * stdlib-only at import time: vLLM / FastAPI are touched only once imported.
"""

import functools
import os
import sys
import types
from importlib.abc import Loader
from typing import Any, Callable

ENV = "SPEC_LIVE"  # "1" enables the whole thing; the launcher sets it
ENV_K = "SPEC_LIVE_K"  # initial live depth (default: the configured maximum)
SCHEDULER = "vllm.v1.core.sched.scheduler"
ENGINE_CORE = "vllm.v1.engine.core"
ROUTERS = "vllm.entrypoints.launchers.api_server.routers"
TARGETS = (SCHEDULER, ENGINE_CORE, ROUTERS)
ROUTE = "/spec_tokens"

_MARK = "__spec_live__"

_applied: set[str] = set()
_noted: set[str] = set()


def _log(msg: str) -> None:
    print(f"[spec-live] {msg}", file=sys.stderr, flush=True)


def _note_once(key: str, msg: str) -> None:
    if key not in _noted:
        _noted.add(key)
        _log(msg)


# ------------------------------------------------------------- live lookup


class LiveLookup(list):
    """Stand-in for Scheduler.dynamic_sd_lookup: indexable by batch size, but
    every batch size answers with the one live K. `max_k` is the configured
    num_speculative_tokens (what everything was allocated for)."""

    def __init__(self, template: list, max_k: int, k: int) -> None:
        super().__init__(template)
        self.max_k = max_k
        self.k = k

    def __getitem__(self, index: Any) -> Any:
        if isinstance(index, slice):
            return list.__getitem__(self, index)
        return self.k


# ---------------------------------------------------------------- scheduler


def _wrap_scheduler_init(orig: Callable[..., None]) -> Callable[..., None]:
    @functools.wraps(orig)
    def __init__(self: Any, *args: Any, **kwargs: Any) -> None:
        orig(self, *args, **kwargs)
        try:
            lookup = getattr(self, "dynamic_sd_lookup", None)
            max_k = int(getattr(self, "num_spec_tokens", 0) or 0)
            if lookup is None or max_k <= 0:
                _note_once(
                    "no-dynamic",
                    "NOT applied: the scheduler is not running Dynamic SD "
                    "(num_speculative_tokens_per_batch_size unset, or MTP off) "
                    "-- the draft depth stays static",
                )
                return
            try:
                k = int(os.environ.get(ENV_K, max_k))
            except ValueError:
                k = max_k
            k = max(0, min(k, max_k))
            self.dynamic_sd_lookup = LiveLookup(lookup, max_k, k)
            _log(f"live draft depth on: K={k} (max {max_k})")
        except Exception as e:
            _note_once("sched-init", f"NOT applied to the scheduler: {e!r}")

    setattr(__init__, _MARK, True)
    return __init__


# -------------------------------------------------------------- engine core


def _live_lookup(core: Any) -> LiveLookup | None:
    lookup = getattr(getattr(core, "scheduler", None), "dynamic_sd_lookup", None)
    return lookup if isinstance(lookup, LiveLookup) else None


def set_spec_tokens(self: Any, k: int) -> list[int]:
    """Utility RPC: set the live draft depth. Returns [previous, current, max]."""
    lookup = _live_lookup(self)
    if lookup is None:
        raise RuntimeError("live draft depth is not enabled in this engine")
    if isinstance(k, bool) or not isinstance(k, int) or not 0 <= k <= lookup.max_k:
        raise ValueError(f"num_speculative_tokens {k!r} must be an int in [0, {lookup.max_k}]")
    previous, lookup.k = lookup.k, k
    _log(f"draft depth {previous} -> {k} (max {lookup.max_k})")
    return [previous, k, lookup.max_k]


def get_spec_tokens(self: Any) -> list[int]:
    """Utility RPC: [current, max], or [] when live depth is not enabled."""
    lookup = _live_lookup(self)
    return [] if lookup is None else [lookup.k, lookup.max_k]


# ------------------------------------------------------------------- routes


def build_router() -> Any:
    """The /spec_tokens routes. FastAPI is imported here, not at module import,
    and the handlers' annotations are real classes (no `from __future__
    import annotations`), which FastAPI needs to recognise `Request`."""
    from fastapi import APIRouter, Request
    from fastapi.responses import JSONResponse

    router = APIRouter()

    def _core(request: Request) -> Any:
        return request.app.state.engine_client.engine_core

    def _err(status: int, message: str, **extra: Any) -> Any:
        return JSONResponse(content={"error": message, **extra}, status_code=status)

    async def _state(request: Request) -> list[int]:
        return list(await _core(request).call_utility_async("get_spec_tokens"))

    @router.get(ROUTE)
    async def get_depth(request: Request):
        try:
            state = await _state(request)
        except Exception as e:
            return _err(500, f"could not read the draft depth: {e}")
        if not state:
            return _err(409, "live draft depth is not enabled (boot with SPEC_LIVE=1)")
        return {"current": state[0], "max": state[1]}

    @router.post(ROUTE)
    async def set_depth(request: Request):
        try:
            body = await request.json()
        except Exception:
            return _err(400, 'body must be JSON: {"num_speculative_tokens": <int>}')
        k = body.get("num_speculative_tokens") if isinstance(body, dict) else None
        if isinstance(k, bool) or not isinstance(k, int):
            return _err(400, 'body must be JSON: {"num_speculative_tokens": <int>}')
        try:
            state = await _state(request)
        except Exception as e:
            return _err(500, f"could not read the draft depth: {e}")
        if not state:
            return _err(409, "live draft depth is not enabled (boot with SPEC_LIVE=1)")
        if not 0 <= k <= state[1]:
            return _err(400, f"num_speculative_tokens must be in [0, {state[1]}]", max=state[1])
        try:
            previous, current, maximum = await _core(request).call_utility_async(
                "set_spec_tokens", k
            )
        except Exception as e:
            return _err(500, f"could not set the draft depth: {e}")
        return {"previous": previous, "current": current, "max": maximum}

    return router


def _wrap_register_routers(orig: Callable[..., Any]) -> Callable[..., Any]:
    @functools.wraps(orig)
    def register_api_routers(*args: Any, **kwargs: Any) -> Any:
        out = orig(*args, **kwargs)
        try:
            app = kwargs.get("app") if "app" in kwargs else args[1]
            app.include_router(build_router())
            _log(f"registered GET/POST {ROUTE}")
        except Exception as e:
            _note_once("routes", f"NOT applied: could not register {ROUTE}: {e!r}")
        return out

    setattr(register_api_routers, _MARK, True)
    return register_api_routers


# ---------------------------------------------------------------- hooking


def _apply(module: types.ModuleType) -> None:
    name = module.__name__
    try:
        if name == SCHEDULER:
            cls = getattr(module, "Scheduler", None)
            if cls is None:
                _note_once(name, f"NOT applied: {name}.Scheduler not found")
            else:
                orig = getattr(cls, "__init__", None)
                if callable(orig) and not getattr(orig, _MARK, False):
                    cls.__init__ = _wrap_scheduler_init(orig)
                    _applied.add(name)
                elif not callable(orig):
                    _note_once(name, f"NOT applied: {name}.Scheduler.__init__ not found")
        elif name == ENGINE_CORE:
            cls = getattr(module, "EngineCore", None)
            if cls is None:
                _note_once(name, f"NOT applied: {name}.EngineCore not found")
            elif not hasattr(cls, "set_spec_tokens"):
                cls.set_spec_tokens = set_spec_tokens
                cls.get_spec_tokens = get_spec_tokens
                _applied.add(name)
            else:
                _note_once(name, f"NOT applied: {name}.EngineCore already has set_spec_tokens")
        elif name == ROUTERS:
            orig = getattr(module, "register_api_routers", None)
            if callable(orig) and not getattr(orig, _MARK, False):
                module.register_api_routers = _wrap_register_routers(orig)  # type: ignore[attr-defined]
                _applied.add(name)
            elif not callable(orig):
                _note_once(name, f"NOT applied: {name}.register_api_routers not found")
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
    """Called from spec_live.pth at interpreter start."""
    if os.environ.get(ENV, "0") != "1":
        return
    if any(isinstance(f, _HookFinder) for f in sys.meta_path):
        return
    sys.meta_path.insert(0, _HookFinder())
    # defensive: a target imported before us (should not happen from a .pth)
    for mod_name in TARGETS:
        mod = sys.modules.get(mod_name)
        if mod is not None:
            _apply(mod)
