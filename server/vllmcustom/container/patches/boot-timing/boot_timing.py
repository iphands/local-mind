"""boot_timing — startup-phase stopwatch for vLLM, with NO upstream-file overlays.

sitecustomize.py (same dir, mounted via PYTHONPATH) imports this at
interpreter start in EVERY python process of the container: the APIServer
CLI, the spawn'd EngineCore/worker, helper subprocesses. That is the earliest
hookable point (T0). From there a sys.meta_path hook wraps a table of known
upstream functions (post-import, by dotted name), so each boot phase appends
enter/exit marks to one shared file, and the APIServer renders a boot
waterfall report the moment uvicorn finishes starting up.

Design rules:
  * No overlay of any upstream file -> nothing to re-derive on nightly bumps.
    A renamed/removed upstream function degrades to a "not hooked" note in
    the report; the boot itself is never touched.
  * Every mark/wrap path is guarded. A broken timer must not break the engine.
  * Render logic (format_report) is PURE over parsed mark rows so it can be
    unit-tested on a host without vLLM installed.

Marks file: $VLLM_BOOT_TIMING_DIR/marks.log (default /tmp/vllm-boot-timing,
which is per-container: every `docker run` boots into a clean /tmp).
One line per mark, tab-separated:

    ts \t pid \t role \t kind \t name \t detail     kind: start|enter|exit|miss
"""

from __future__ import annotations

import functools
import inspect
import os
import sys
import time
import types
from collections import deque
from importlib.abc import Loader
from typing import Any, Callable, NamedTuple

_DIR = os.environ.get("VLLM_BOOT_TIMING_DIR", "/tmp/vllm-boot-timing")
_MARKS = os.path.join(_DIR, "marks.log")

# ──────────────────────────────────────────────────────────────────────────────
# phase metadata: stable mark name -> report label
# ──────────────────────────────────────────────────────────────────────────────

LABELS: dict[tuple[str, str], str] = {
    ("api.serve", ""): "serve entry (run_server)",
    ("api.engine_build", ""): "engine build: config → spawn → core ready",
    ("api.mm_warmup", ""): "mm-processor warmup",
    ("api.core_ready", ""): "engine core spawn → ready (wait)",
    ("api.app_state", ""): "app state: tokenizer/templates/parsers",
    ("api.uvicorn", ""): "uvicorn bind + lifespan",
    ("core.proc", ""): "engine proc main (spawn → serving loop)",
    ("core.ctor", ""): "EngineCore init (load+profile+kv+warmup)",
    ("worker.ctor", ""): "worker construction",
    ("worker.device", ""): "device/context init",
    ("worker.load", ""): "WEIGHT LOAD (model build + shards)",
    ("worker.profile", ""): "memory profiling (dummy run)",
    ("worker.kv_alloc", ""): "KV cache allocation",
    ("worker.warmup", ""): "warmup: kernels + autotune + capture",
    ("warmup.kernels", ""): "kernel JIT warmup",
    ("warmup.autotune", ""): "flashinfer autotune",
    ("runner.graphs", "final"): "cudagraph capture (final)",
    ("runner.graphs", "profile"): "cudagraph capture (memory probe)",
}


class Span(NamedTuple):
    name: str
    detail: str
    t0: float
    t1: float


class Row(NamedTuple):
    ts: float
    pid: int
    role: str
    kind: str
    name: str
    detail: str


# ──────────────────────────────────────────────────────────────────────────────
# pure render logic (unit-testable without vLLM)
# ──────────────────────────────────────────────────────────────────────────────


def _fmt_dur(s: float) -> str:
    if s >= 60:
        return f"{int(s // 60)}m{int(s) % 60:02d}s"
    if s >= 10:
        return f"{s:.0f}s"
    return f"{s:.1f}s"


def _fmt_off(s: float) -> str:
    m, sec = divmod(int(s), 60)
    return f"+{m}m{sec:02d}s"


def _bar(d: float, total: float, width: int = 30) -> str:
    if total <= 0 or d <= 0:
        return ""
    n = round(width * d / total)
    return "█" * max(1, n)


def match_spans(rows: list[Row], end_default: float) -> list[Span]:
    """Pair enter/exit marks per (name, detail); unmatched enters end at
    `end_default` (a wrapper that never returns, like the uvicorn serve
    loop, is an open span)."""
    open_q: dict[tuple[str, str], deque[float]] = {}
    out: list[Span] = []
    for r in sorted(rows, key=lambda r: r.ts):
        key = (r.name, r.detail)
        if r.kind == "enter":
            open_q.setdefault(key, deque()).append(r.ts)
        elif r.kind == "exit":
            q = open_q.get(key)
            t0 = q.popleft() if q else r.ts
            out.append(Span(r.name, r.detail, t0, r.ts))
    for (name, detail), q in open_q.items():
        for t0 in q:
            out.append(Span(name, detail, t0, end_default))
    return out


def _depth(sp: Span, spans: list[Span]) -> int:
    """Containment depth within one process (children draw indented)."""
    d = 0
    for o in spans:
        if (
            o is not sp
            and o.t0 <= sp.t0
            and o.t1 >= sp.t1
            and (o.t1 - o.t0) > (sp.t1 - sp.t0)
        ):
            d += 1
    return d


def _proc_name(role: str, pid: int, seen_engine: list[int]) -> str:
    if role == "api":
        return f"APIServer (pid {pid})"
    if role == "engine" and pid not in seen_engine:
        seen_engine.append(pid)
        return f"EngineCore+worker (pid {pid})"
    return f"engine helper (pid {pid})"


def format_report(rows: list[Row], now: float, width: int = 74) -> str:
    """The boot-waterfall report. `rows` = all mark rows of one boot."""
    procs: dict[int, list[Row]] = {}
    for r in rows:
        procs.setdefault(r.pid, []).append(r)
    starts = {
        pid: min(r.ts for r in rws if r.kind == "start")
        for pid, rws in procs.items()
        if any(r.kind == "start" for r in rws)
    }
    total = now - min(starts.values()) if starts else 0.0

    out: list[str] = []
    out.append("".ljust(width, "═"))
    out.append("v L L M   B O O T   T I M I N G".center(width))
    out.append("".ljust(width, "═"))
    out.append(
        f" TOTAL  {_fmt_dur(total)}   first python process → serving ready"
    )
    out.append("   (docker image/entrypoint time before python: not measurable here)")
    out.append("")

    seen_engine: list[int] = []
    order = sorted(
        procs,
        key=lambda p: (
            0 if any(r.role == "api" for r in procs[p]) else 1,
            p,
        ),
    )
    for pid in order:
        rws = procs[pid]
        role = next((r.role for r in rws if r.kind == "start"), rws[0].role)
        spans = [
            s
            for s in match_spans([r for r in rws if r.kind in ("enter", "exit")], now)
            if s.t1 - s.t0 >= 0.05
        ]
        if not spans and pid not in starts:
            continue
        spans.sort(key=lambda s: (s.t0, -(s.t1 - s.t0)))
        start = starts.get(pid, spans[0].t0 if spans else now)
        wall = max((s.t1 for s in spans), default=start) - start
        head = f" ── {_proc_name(role, pid, seen_engine)} ── wall {_fmt_dur(wall)} "
        out.append(head[:width])

        first_t0 = spans[0].t0 if spans else now
        imports = first_t0 - start
        cols = f"{'':14s}"
        if imports >= 0.3:
            out.append(
                f"   {_fmt_off(0):<7s}  {'python imports + bootstrap':<46s}"
                f"{_fmt_dur(imports):>7s} {_bar(imports, total)}"
            )
        for sp in spans:
            if sp.t1 - sp.t0 < 0.5 and sp.name != "api.uvicorn":
                continue
            depth = _depth(sp, spans)
            pad = max(12, 46 - 2 * depth)
            label = LABELS.get((sp.name, sp.detail), sp.name)
            out.append(
                f"   {_fmt_off(sp.t0 - start):<7s}  {'  ' * depth}{label:<{pad}.{pad}s}"
                f"{_fmt_dur(sp.t1 - sp.t0):>7s} {_bar(sp.t1 - sp.t0, total)}"
            )
        covered = imports + sum(s.t1 - s.t0 for s in spans if _depth(s, spans) == 0)
        if wall - covered >= 1.0:
            out.append(
                f"{cols}{'other glue/gaps':<44s}{_fmt_dur(wall - covered):>7s}"
            )
        out.append("")

    # notes ────────────────────────────────────────────────────────────────
    all_spans: list[tuple[int, Span]] = []
    for pid, rws in procs.items():
        all_spans += [
            (pid, s)
            for s in match_spans([r for r in rws if r.kind in ("enter", "exit")], now)
        ]
    notes: list[str] = []
    for pid, sp in all_spans:
        if sp.name == "worker.load" and total > 0:
            notes.append(
                f"weight load = {round(100 * (sp.t1 - sp.t0) / total)}% "
                "of total boot (cold NFS read; page cache helps the next boot)"
            )
    if any(sp.name == "warmup.autotune" for _, sp in all_spans):
        notes.append("flashinfer autotune result is cached under /root/.cache/vllm "
                     "(~free on later boots with the same build)")
    misses = sorted({r.detail for r in rows if r.kind == "miss"})
    if misses:
        notes.append("hooks NOT applied (upstream drift — those rows are missing "
                     "above): " + ", ".join(misses))
    if notes:
        out.append(" ── notes ──")
        out += [f"   · {n}" for n in notes]
    out.append(f"   · raw marks: {_MARKS}")
    out.append("".ljust(width, "═"))
    return "\n".join(out)


# ──────────────────────────────────────────────────────────────────────────────
# process-local state / mark I/O
# ──────────────────────────────────────────────────────────────────────────────

_PID = os.getpid()
_ROLE = "none"
_LOCK_STATE = "init"
_FD: int | None = None
_OWN_SPANS: list[Span] = []
_REPORT_DONE = False


def _record(kind: str, name: str, detail: str = "") -> None:
    global _FD
    if _ROLE == "none":
        return
    try:
        if _FD is None:
            os.makedirs(_DIR, exist_ok=True)
            _FD = os.open(_MARKS, os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o644)
        os.write(_FD, f"{time.time():.6f}\t{_PID}\t{_ROLE}\t{kind}\t{name}\t{detail}\n".encode())
    except OSError:
        pass


def _detect_role() -> str:
    """api = the vllm CLI process; engine = multiprocessing spawn children
    (EngineCore and workers); none = everything else (loky/inductor/tracker
    helpers — they must not pollute the report)."""
    try:
        joined = " ".join(
            p for p in open("/proc/self/cmdline", "rb").read().decode("utf-8", "replace").split("\0") if p
        )
    except OSError:
        return "none"
    if "multiprocessing.spawn" in joined or "--multiprocessing-fork" in joined:
        return "engine"
    if "loky" in joined or "resource_tracker" in joined or "sitecustomize" in joined:
        return "none"
    return "api" if "vllm" in joined else "none"


# ──────────────────────────────────────────────────────────────────────────────
# hook table: module -> (dotted target, mark name, opts)
#   opts: enter_only (wraps never-returning serve loops), detail (kwargs ->
#   detail str), cb (call after the exit mark)
# ──────────────────────────────────────────────────────────────────────────────


def _profile_flag(args: tuple, kwargs: dict) -> str:
    return "profile" if kwargs.get("profile_only") else "final"


def _engine_summary() -> None:
    try:
        d = {s.name: s for s in _OWN_SPANS}

        def dur(n: str) -> str:
            s = d.get(n)
            return _fmt_dur(s.t1 - s.t0) if s else "-"

        ctor = d.get("core.ctor")
        total = f" · engine total {_fmt_dur(ctor.t1 - ctor.t0)}" if ctor else ""
        print(
            f"[boot-timing] EngineCore (pid {_PID}): weights {dur('worker.load')}"
            f" · profile {dur('worker.profile')} · kv {dur('worker.kv_alloc')}"
            f" · warmup {dur('worker.warmup')}{total}",
            flush=True,
        )
    except Exception:
        pass


def _report_now() -> None:
    global _REPORT_DONE
    if _REPORT_DONE:
        return
    _REPORT_DONE = True
    try:
        rows = read_marks(_MARKS)
        print("\n" + format_report(rows, time.time()) + "\n", flush=True)
    except Exception as e:  # never take the API server down with us
        print(f"[boot-timing] report failed: {e!r}", file=sys.stderr, flush=True)


TARGETS: dict[str, list[dict[str, Any]]] = {
    # APIServer process
    "vllm.entrypoints.launchers.api_server.entry": [
        dict(qual="run_server", name="api.serve", enter_only=True),
        dict(qual="build_async_engine_client", name="api.engine_build"),
    ],
    "vllm.entrypoints.launchers.api_server.app_state": [
        dict(qual="init_app_state", name="api.app_state"),
    ],
    "vllm.renderers.base": [
        dict(qual="warmup_mm", name="api.mm_warmup"),  # method, class-scanned
    ],
    "vllm.v1.engine.core_client": [
        dict(qual="make_client", name="api.core_ready"),  # method, class-scanned
    ],
    "uvicorn.server": [
        dict(qual="Server.startup", name="api.uvicorn", cb=_report_now),
    ],
    # EngineCore process (worker is in-process at TP1)
    "vllm.v1.engine.core": [
        dict(qual="EngineCoreProc.run_engine_core", name="core.proc", enter_only=True),
        dict(qual="EngineCore.__init__", name="core.ctor"),
    ],
    "vllm.v1.worker.gpu_worker": [
        dict(qual="Worker.__init__", name="worker.ctor"),
        dict(qual="Worker.init_device", name="worker.device"),
        dict(qual="Worker.load_model", name="worker.load"),
        dict(qual="Worker.determine_available_memory", name="worker.profile"),
        dict(qual="Worker.initialize_from_config", name="worker.kv_alloc"),
        dict(qual="Worker.compile_or_warm_up_model", name="worker.warmup", cb=_engine_summary),
    ],
    "vllm.v1.worker.gpu.model_runner": [
        dict(qual="GPUModelRunner.capture_model", name="runner.graphs", detail=_profile_flag),
    ],
    "vllm.model_executor.warmup.kernel_warmup": [
        dict(qual="kernel_warmup", name="warmup.kernels"),
        dict(qual="flashinfer_autotune", name="warmup.autotune"),
    ],
}

_HOOKED: list[str] = []
_MISSING: list[str] = []


def _make_wrapper(orig: Callable, spec: dict) -> Callable:
    name = spec["name"]
    detail = spec.get("detail")
    enter_only = spec.get("enter_only", False)
    cb = spec.get("cb")

    def _det(a: tuple, k: dict) -> str:
        try:
            return detail(a, k) if detail else ""
        except Exception:
            return ""

    def _close(d: str, t0: float) -> None:
        try:
            _record("exit", name, d)
            # completed own spans feed the per-process one-liner
            _OWN_SPANS.append(Span(name, d, t0, time.time()))
        except Exception:
            pass

    if inspect.iscoroutinefunction(orig):

        @functools.wraps(orig)
        async def coro_wrapper(*a: Any, **k: Any) -> Any:
            d = _det(a, k)
            _record("enter", name, d)
            t0 = time.time()
            try:
                return await orig(*a, **k)
            finally:
                if not enter_only:
                    _close(d, t0)
                    if cb:
                        try:
                            cb()
                        except Exception:
                            pass

        w = coro_wrapper
    else:

        @functools.wraps(orig)
        def wrapper(*a: Any, **k: Any) -> Any:
            d = _det(a, k)
            _record("enter", name, d)
            t0 = time.time()
            try:
                return orig(*a, **k)
            finally:
                if not enter_only:
                    _close(d, t0)
                    if cb:
                        try:
                            cb()
                        except Exception:
                            pass

        w = wrapper
    w.__boot_timed__ = True  # type: ignore[attr-defined]
    return w


def _resolve(module: types.ModuleType, qual: str) -> list[tuple[Any, str]]:
    """[(holder, attr)] to wrap. Dotted quals walk attributes; a bare qual is
    treated as a method name and class-scanned within the module (drift-proof
    against class renames) or as a module-level function."""
    parts = qual.split(".")
    if len(parts) > 1 and hasattr(module, parts[0]):
        obj: Any = module
        for p in parts[:-1]:
            obj = getattr(obj, p)
        return [(obj, parts[-1])] if hasattr(obj, parts[-1]) else []
    attr = parts[0]
    out: list[tuple[Any, str]] = []
    top = getattr(module, attr, None)
    if top is not None and not inspect.isclass(top) and callable(top):
        out.append((module, attr))
    for cls in vars(module).values():
        if (
            inspect.isclass(cls)
            and cls.__module__ == module.__name__
            and callable(getattr(cls, attr, None))
        ):
            out.append((cls, attr))
    return out


def _apply(module: types.ModuleType) -> None:
    for spec in TARGETS.get(module.__name__, ()):
        qual, name = spec["qual"], spec["name"]
        try:
            holders = _resolve(module, qual)
        except Exception:
            holders = []
        done = False
        for holder, attr in holders:
            orig = getattr(holder, attr, None)
            if orig is None or getattr(orig, "__boot_timed__", False):
                continue
            try:
                wrapped = _make_wrapper(orig, spec)
                setattr(holder, attr, wrapped)
                done = True
                _HOOKED.append(f"{module.__name__}.{qual}")
            except Exception as e:
                _record("miss", name, f"{module.__name__}.{qual} ({e!r})")
        if not done and not any(h.startswith(f"{module.__name__}.{qual}") for h in _HOOKED):
            _MISSING.append(f"{module.__name__}.{qual}")
            _record("miss", name, f"{module.__name__}.{qual}")


class _HookLoader(Loader):
    def __init__(self, inner: Any) -> None:
        self._inner = inner

    def create_module(self, spec: Any) -> Any:
        return self._inner.create_module(spec)

    def exec_module(self, module: types.ModuleType) -> None:
        self._inner.exec_module(module)
        try:
            _apply(module)
        except Exception as e:
            print(f"[boot-timing] apply failed for {module.__name__}: {e!r}", file=sys.stderr)

    def __getattr__(self, name: str) -> Any:
        return getattr(self._inner, name)


class _HookFinder:
    """Meta-path finder: only intercepts modules in TARGETS, wraps their real
    loader, applies hooks right after execution."""

    def find_spec(self, fullname: str, path: Any = None, target: Any = None) -> Any:
        if fullname not in TARGETS:
            return None
        for finder in sys.meta_path:
            if isinstance(finder, _HookFinder):
                continue
            try:
                spec = finder.find_spec(fullname, path, target)
            except Exception:
                continue
            if spec is not None and spec.loader is not None:
                spec.loader = _HookLoader(spec.loader)
                return spec
        return None


def read_marks(path: str) -> list[Row]:
    rows: list[Row] = []
    try:
        with open(path) as f:
            for line in f:
                p = line.rstrip("\n").split("\t")
                if len(p) < 6:
                    continue
                try:
                    rows.append(Row(float(p[0]), int(p[1]), p[2], p[3], p[4], "\t".join(p[5:])))
                except ValueError:
                    continue
    except OSError:
        pass
    return rows


def init() -> None:
    """Called from sitecustomize.py at interpreter start, in every process."""
    global _ROLE, _LOCK_STATE
    if os.environ.get("BOOT_TIMING", "1") == "0":
        return
    _ROLE = _detect_role()
    if _ROLE == "none":
        return
    _LOCK_STATE = "on"
    _record("start", "process.start")
    sys.meta_path.insert(0, _HookFinder())
    # defensive: apply to any target already imported (shouldn't happen —
    # sitecustomize runs before any vllm import)
    for mod_name in list(TARGETS):
        mod = sys.modules.get(mod_name)
        if mod is not None:
            _apply(mod)


def hooked() -> list[str]:
    return list(_HOOKED)


def missing() -> list[str]:
    return list(_MISSING)
