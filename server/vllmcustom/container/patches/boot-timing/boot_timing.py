"""boot_timing — startup-phase stopwatch for vLLM, with NO upstream-file overlays.

sitecustomize.py (same dir, mounted via PYTHONPATH) imports this at
interpreter start in EVERY python process of the container: the APIServer
CLI, the spawn'd EngineCore/worker, helper subprocesses. That is the earliest
hookable point (T0). From there a sys.meta_path hook wraps a table of known
upstream functions (post-import, by dotted name), so each boot phase appends
enter/exit marks to one shared file, and the APIServer renders a boot
waterfall report the moment uvicorn finishes starting up. On top of the
timing waterfall the report attributes GPU memory per phase (torch.cuda
sampling around the same wrapped calls — zero extra hooks) and prints host
RAM lines plus the gpu_memory_utilization headroom arithmetic.

Design rules:
  * No overlay of any upstream file -> nothing to re-derive on nightly bumps.
    A renamed/removed upstream function degrades to a "not hooked" note in
    the report; the boot itself is never touched.
  * Every mark/wrap/sample path is guarded. A broken timer must not break
    the engine.
  * Render logic (format_report and the mem renderers) is PURE over parsed
    mark rows so it can be unit-tested on a host without vLLM installed.

Marks file: $VLLM_BOOT_TIMING_DIR/marks.log (default /tmp/vllm-boot-timing,
which is per-container: every `docker run` boots into a clean /tmp).
One line per mark, tab-separated:

    ts \t pid \t role \t kind \t name \t detail     kind: start|enter|exit|miss|mem

    mem rows carry comma-separated key=value bytes in detail (util is a float);
the renderers skip any value they cannot parse, so a partial mem section is
always safe.

# allow: SIZE_OK — must stay ONE stdlib-only module importable as `boot_timing`
# from sitecustomize via the PYTHONPATH mount (sitecustomize.py is frozen);
# the natural split (marks / render / sample) would change that mount contract.
"""

from __future__ import annotations

import functools
import inspect
import math
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
            # LIFO: same-(name, detail) calls NEST (a super()-chain re-enters
            # the wrapper: parent enter, child enter, child exit, parent
            # exit), so the pending mark to close is the most recently
            # opened one. FIFO would cross-pair parent with child and
            # produce two overlapping, wrong spans.
            t0 = q.pop() if q else r.ts
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


# ──────────────────────────────────────────────────────────────────────────────
# memory report render — pure over the parsed mem.* rows (no torch, no vLLM)
#
# The headroom floors rest on one structural fact: the KV cache is the ONLY
# gpu_memory_utilization-scaled component, so "how low could util go" is
# exact arithmetic on measured bytes, not an estimate.
# ──────────────────────────────────────────────────────────────────────────────

_GIB = 2**30
_MIN_KV = 1.0 * _GIB       # below this KV vLLM refuses to boot at all
_SAFETY_MIN = 1.5 * _GIB   # safe-floor margin: absolute floor ...
_SAFETY_FRAC = 0.02        # ... or 2% of the card, whichever is bigger


def _parse_kv(detail: str) -> dict[str, str]:
    out: dict[str, str] = {}
    for part in detail.split(","):
        k, sep, v = part.partition("=")
        if sep and k:
            out[k] = v
    return out


def _mem_map(rows: list[Row]) -> dict[str, dict[str, str]]:
    """mem.* mark name -> parsed key=value map (latest row wins; boot is
    sequential). The CUDA-graph marks recur once per capture phase, so the
    phase rides in the key: 'mem.gr.x|final'."""
    m: dict[str, dict[str, str]] = {}
    for r in sorted(rows, key=lambda r: r.ts):
        if r.kind == "mem" and r.role == "engine":
            kv = _parse_kv(r.detail)
            m[f"{r.name}|{kv.get('phase', '')}"] = kv
    return m


def _mv(m: dict[str, dict[str, str]], mark: str, key: str) -> float | None:
    """One numeric value out of the mem map. Any missing mark, missing key
    or unparseable value yields None so each render line degrades alone."""
    v = m.get(mark if "|" in mark else mark + "|", {}).get(key)
    if v is None:
        return None
    try:
        return float(v)
    except ValueError:
        return None


def _mdelta(
    m: dict[str, dict[str, str]], e_mark: str, x_mark: str, key: str = "alloc"
) -> float | None:
    a, b = _mv(m, e_mark, key), _mv(m, x_mark, key)
    return None if a is None or b is None else b - a


def _mem_total(m: dict[str, dict[str, str]]) -> float | None:
    """Card size in bytes; mem.final preferred (latest read), device fallback."""
    t = _mv(m, "mem.final", "total")
    if t is None or t <= 0:
        t = _mv(m, "mem.device", "total")
    return t if t is not None and t > 0 else None


def _mem_pid(rows: list[Row]) -> int:
    for r in sorted(rows, key=lambda r: r.ts):
        if r.kind == "mem" and r.role == "engine":
            return r.pid
    return -1


def _gpu_parts(m: dict[str, dict[str, str]]) -> dict[str, float | None]:
    """Every GPU-memory derivation at once; entries the marks cannot support
    are None (byte values, not GiB)."""
    total = _mem_total(m)
    util = _mv(m, "mem.cfg", "util")
    budget = util * total if util is not None and total is not None else None
    free_dev = _mv(m, "mem.device", "free")
    res = _mv(m, "mem.final", "res")
    alloc = _mv(m, "mem.final", "alloc")
    free = _mv(m, "mem.final", "free")
    return {
        "total": total,
        "util": util,
        "budget": budget,
        # weights = the alloc step across the model load
        "weights": _mdelta(m, "mem.load.e", "mem.load.x"),
        # kv = the alloc step across initialize_from_config
        "kv": _mdelta(m, "mem.kv.e", "mem.kv.x"),
        # graphs = the alloc step across the FINAL capture_model call only
        "graphs": _mdelta(m, "mem.gr.e|final", "mem.gr.x|final"),
        # context+torch runtime = what the driver/had already taken at CUDA init
        "ctx": None if free_dev is None or total is None else total - free_dev,
        "overhead": None if res is None or alloc is None else res - alloc,
        # outside = card memory the torch allocator never owned at all
        "outside": (
            None if total is None or free is None or res is None
            else total - free - res
        ),
        "final_used": None if total is None or free is None else total - free,
    }


def _act_peak(m: dict[str, dict[str, str]]) -> float | None:
    """Transient activation peak (bytes) of the dummy profiling run: peak
    above the allocated baseline determine_available_memory started from."""
    peak = _mv(m, "mem.prof.x", "peak")
    base = _mv(m, "mem.prof.e", "alloc")
    return None if peak is None or base is None else peak - base


def _mem_tokens(m: dict[str, dict[str, str]]) -> float | None:
    blocks = _mv(m, "mem.kvcfg", "blocks")
    bsize = _mv(m, "mem.kvcfg", "block_size")
    return None if blocks is None or bsize is None else blocks * bsize


def _gib_row(
    ind: str, label: str, gib: float, bar: str = "", pct: str = ""
) -> str:
    """One value row: label / GiB at one decimal / optional bar / optional
    percent, columns aligned across the section."""
    line = f"{ind}{label:<45s}{gib / _GIB:>8.1f} GiB"
    if bar:
        line += f"  {bar:<17s}{pct}"
    elif pct:
        line += f"   {pct}"
    return line


def _kv_label(m: dict[str, dict[str, str]]) -> str:
    tokens = _mem_tokens(m)
    if tokens is None or tokens <= 0:
        return "KV cache"
    return f"KV cache  (~{int(tokens) // 1000}k tokens @ full ctx)"


def _render_gpu(m: dict[str, dict[str, str]], pid: int) -> list[str]:
    p = _gpu_parts(m)
    total = p["total"]
    if total is None:
        return []
    out = [f" ── GPU memory — {total / _GIB:.1f} GiB total (pid {pid}) ──"]
    budget = p["budget"]
    if budget is not None:
        out.append(
            _gib_row(
                "   ",
                f"vLLM budget (gpu_memory_utilization={p['util']:.2f})",
                budget,
                "",
                f"{budget / total * 100:.1f}%",
            )
        )
    bars: list[tuple[str, float]] = []
    for label, v in (
        ("model weights (NVFP4, PLE excluded)", p["weights"]),
        (_kv_label(m), p["kv"]),
        ("CUDA graphs (final capture)", p["graphs"]),
        ("CUDA context + torch runtime", p["ctx"]),
        ("allocator overhead (reserved − allocated)", p["overhead"]),
    ):
        if v is not None:
            bars.append((label, v))
    # headroom = what vLLM granted inside the budget but never used; negative
    # means the parts exceeded the grant -> a note, not a bar (see _mem_notes)
    if budget is not None and len(bars) == 5:
        headroom = budget - sum(v for _, v in bars)
        if headroom >= 0:
            bars.append(("budget headroom (granted but unused)", headroom))
    for i, (label, v) in enumerate(bars):
        if budget is None:
            # no util mark -> absolutes only; nothing sane to divide by
            out.append(_gib_row("     ", label, v))
        else:
            # _bar at width 16 matches the approved section width; its
            # min-one-block rule keeps 1% items visible.
            pct = f"{v / budget * 100:.1f}%" + (" of budget" if i == 0 else "")
            out.append(_gib_row("     ", label, v, _bar(v, budget, 16), pct))
    if p["outside"] is not None:
        out.append(
            _gib_row("   ", "outside budget (driver, other)", p["outside"])
        )
    return out


def _render_host(m: dict[str, dict[str, str]], api_rss: float | None) -> list[str]:
    avail = _mv(m, "mem.final", "avail")
    rss = _mv(m, "mem.final", "rss")
    avail0 = _mv(m, "mem.host0", "avail")
    mtotal = _mv(m, "mem.final", "mtotal")
    lines: list[str] = []
    # MemAvailable is machine-wide, so another process can invert the delta
    # between the two marks; a negative "consumed" is noise, not a number.
    if avail is not None and avail0 is not None and avail0 >= avail:
        lines.append(
            _gib_row(
                "     ", "host RAM consumed by boot (pinned weights)", avail0 - avail
            )
        )
    parts: list[str] = []
    # a sub-0.05-GiB reading renders "0.0 GiB" — that is an unknown/empty
    # process, not a number; drop the term instead of printing it, and the
    # whole line when both terms are gone
    if rss is not None and rss / _GIB >= 0.05:
        parts.append(f"engine {rss / _GIB:.1f} GiB")
    if api_rss and api_rss / _GIB >= 0.05:
        parts.append(f"api {api_rss / _GIB:.1f} GiB")
    if parts:
        lines.append(f"     RSS: {' · '.join(parts)}")
    if avail is not None:
        tail = f" / {mtotal / _GIB:.1f} GiB" if mtotal is not None else ""
        lines.append(_gib_row("     ", "MemAvailable at serve-ready", avail) + tail)
    if not lines:
        return []
    return [" ── host RAM ──", *lines]


def _render_headroom(m: dict[str, dict[str, str]]) -> list[str]:
    p = _gpu_parts(m)
    total, util, kv, final_used = p["total"], p["util"], p["kv"], p["final_used"]
    if total is None or util is None or kv is None or final_used is None:
        return []
    non_kv = final_used - kv
    safety = max(_SAFETY_MIN, _SAFETY_FRAC * total)
    hard = non_kv / total
    # ceil to 2 decimals: the suggested util must never under-cover the
    # floor it is computed from (2 dp = the CLI's own util granularity)
    safe = math.ceil((non_kv + _MIN_KV + safety) / total * 100) / 100
    out = [" ── headroom ──", f"   gpu_memory_utilization = {util:.2f} now"]
    if safe >= util:
        out.append("   already at/below safe floor — no headroom to reclaim")
        return out
    tokens = _mem_tokens(m)
    freed = (util - safe) * total
    # KV retained at the safe util: safe*total covers the non-KV floor, what
    # is left is KV. (kv - freed) is WRONG when the granted budget has
    # unused slack: the freed bytes consume that slack FIRST, so the naive
    # form under-reports retained KV by exactly the slack and contradicts
    # this section's own min-KV promise.)
    kv_at_safe = max(0.0, safe * total - non_kv)
    tok_tail = (
        f" (~{int(tokens * kv_at_safe / kv) // 1000}k tok)"
        if tokens is not None and kv > 0
        else ""
    )
    out.append(
        f"   could drop to  {safe:.2f} safe   -> frees {freed / _GIB:.1f} GiB"
        f" · KV {kv / _GIB:.1f} -> {kv_at_safe / _GIB:.1f} GiB{tok_tail}"
    )
    out.append(
        f"   hard floor     {hard:.2f}        -> below this: no KV allocatable,"
        " vLLM refuses to boot"
    )
    trade = 0.01 * total
    rate_tok = (
        f"  (~{tokens * trade / kv / 1000:.1f}k tokens per 0.01)"
        if tokens is not None and kv > 0
        else ""
    )
    out.append(f"   trade rate     {trade / _GIB:.2f} GiB per 0.01 util{rate_tok}")
    # The build-up is the DISPLAY decomposition of the floor (transient
    # activation peak instead of allocator overhead); the floors themselves
    # come from the non_kv formula above.
    ap = _act_peak(m)
    terms = [
        ("weights", p["weights"]),
        ("graphs", p["graphs"]),
        ("context", p["ctx"]),
        ("act.peak", ap),
        ("off-torch", p["outside"]),
        ("margin", safety),
        ("min-KV", _MIN_KV),
    ]
    parts = [f"{k} {v / _GIB:.1f}" for k, v in terms if v is not None]
    if parts:
        first = "   floor build-up: "
        keep: list[str] = []
        for t in parts:
            if keep and len(first + " + ".join([*keep, t])) > 76:
                break
            keep.append(t)
        rest = parts[len(keep):]
        ssum = sum(v for _, v in terms if v is not None)
        tail = f"  = {ssum / _GIB:.1f} GiB / {total / _GIB:.1f}"
        if rest:
            out.append(first + " + ".join(keep))
            out.append(f"                   + {' + '.join(rest)}{tail}")
        else:
            out.append(first + " + ".join(parts) + tail)
    mbt = _mv(m, "mem.cfg", "tokens")
    if ap is not None and mbt is not None:
        out.append(
            f"   caveat: act.peak measured at max_num_batched_tokens={int(mbt)} —"
            " raising it or mm"
        )
        out.append("           spikes eat the margin; hard floor stays valid")
    return out


def _mem_notes(m: dict[str, dict[str, str]]) -> list[str]:
    """The three memory cross-checks, phrased as notes. The profiling peak
    stays a NOTE and never a bar: it is transient (dummy run), charging it
    against live memory would double-count."""
    notes: list[str] = []
    ap = _act_peak(m)
    if ap is not None and ap > 0:
        notes.append(
            f"profiling peak {ap / _GIB:.1f} GiB was transient (dummy run): it "
            "shrank the KV budget then, holds nothing now"
        )
    p = _gpu_parts(m)
    kvb = _mv(m, "mem.kvbudget", "bytes")
    if p["kv"] is not None and kvb is not None:
        diff = abs(p["kv"] - kvb) / _GIB
        if diff <= 0.5:
            notes.append(
                "KV cross-check: measured vs vLLM's own budget agree "
                "(within 0.5 GiB) ✓"
            )
        else:
            notes.append(
                f"KV cross-check: measured vs vLLM's own budget differ by "
                f"{diff:.1f} GiB"
            )
    five = (p["weights"], p["kv"], p["graphs"], p["ctx"], p["overhead"])
    if p["budget"] is not None and None not in five:
        headroom = p["budget"] - sum(five)  # type: ignore[arg-type]
        if headroom < 0:
            notes.append(f"budget oversubscribed by {-headroom / _GIB:.1f} GiB")
    if None not in (*five, p["outside"], p["final_used"]):
        s = sum((*five, p["outside"]))  # type: ignore[arg-type]
        assert p["final_used"] is not None
        if abs(s - p["final_used"]) / _GIB > 2.0:
            notes.append(
                f"GPU accounting drift: sum-of-parts {s / _GIB:.1f} GiB vs "
                f"final used {p['final_used'] / _GIB:.1f} GiB — a phase mark "
                "may be missing"
            )
    return notes


def _mem_report(
    rows: list[Row], api_rss: float | None
) -> tuple[list[str], list[str]]:
    """(section lines, note lines) for the memory block. No mem marks at all
    (engine died before init_device) -> no sections, one explanatory note."""
    m = _mem_map(rows)
    if not m:
        return [], ["memory: insufficient marks"]
    lines = (
        _render_gpu(m, _mem_pid(rows))
        + _render_host(m, api_rss)
        + _render_headroom(m)
    )
    if lines:
        lines.append("")
    return lines, _mem_notes(m)


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
    out.append("v L L M   P O S T - S T A R T U P   S T A T S".center(width))
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
        if not spans:
            # start mark but zero spans: transient helper with a vllm-ish
            # cmdline (role false-positive) or an engine that died before
            # importing vllm. An "imports + bootstrap" bar here would
            # measure start → report-time — unbounded garbage (a 21 m ghost
            # bar was observed in a live report). Name the emptiness
            # instead, with no header and no bars.
            out.append(f"   {_proc_name(role, pid, seen_engine)}: no phases recorded")
            out.append("")
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

    # memory sections: GPU attribution + host RAM + util headroom
    api_rss = _host_mem().get("rss")
    mem_lines, mem_notes = _mem_report(rows, api_rss)
    out += mem_lines

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
    notes += mem_notes
    # a label that never fired is either drifted-out or not on this boot's
    # path — say which names, so a silently shrinking report stays visible
    unfired = sorted({name for name, _ in LABELS} - {r.name for r in rows})
    if unfired:
        notes.append("spans not seen this boot: " + ", ".join(unfired))
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
_FD: int | None = None
_OWN_SPANS: list[Span] = []
_REPORT_DONE = False


def _record(kind: str, name: str, detail: str = "") -> None:
    global _FD
    if _ROLE == "none":
        return
    # detail rides a tab-separated, newline-terminated field: a stray \t or
    # newline (e.g. inside an exception repr) would forge extra fields or
    # rows — flatten them to single spaces at write time, once.
    detail = detail.replace("\t", " ").replace("\n", " ").replace("\r", " ")
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
# memory sampling (writer side). Every path here is guarded twice: a missing
# torch/CUDA records NOTHING (the report degrades silently everywhere
# downstream), and _mem_sample never raises into the wrapped boot call.
# ──────────────────────────────────────────────────────────────────────────────


def _host_mem() -> dict[str, int]:
    """VmRSS / MemAvailable / MemTotal in bytes; keys omitted when unreadable."""
    out: dict[str, int] = {}
    try:
        with open("/proc/self/status") as f:
            for line in f:
                if line.startswith("VmRSS:"):
                    out["rss"] = int(line.split()[1]) * 1024
                    break
    except (OSError, ValueError):
        pass
    try:
        with open("/proc/meminfo") as f:
            for line in f:
                if line.startswith("MemTotal:"):
                    out["mtotal"] = int(line.split()[1]) * 1024
                elif line.startswith("MemAvailable:"):
                    out["avail"] = int(line.split()[1]) * 1024
    except (OSError, ValueError):
        pass
    return out


def _cfg_detail(a: tuple) -> str:
    """gpu_memory_utilization + max_num_batched_tokens off the Worker's
    vllm_config. Each key rides its own guarded getattr chain: an upstream
    rename shrinks the report, never breaks the boot."""
    parts: list[str] = []
    try:
        util = float(a[0].vllm_config.cache_config.gpu_memory_utilization)
        parts.append(f"util={util}")
    except Exception:
        pass
    try:
        parts.append(
            f"tokens={int(a[0].vllm_config.scheduler_config.max_num_batched_tokens)}"
        )
    except Exception:
        pass
    return ",".join(parts)


def _kvcfg_detail(a: tuple) -> str:
    """blocks × block_size from initialize_from_config's KVCacheConfig list
    (a dict-shaped TypedDict upstream). Any shape surprise -> no mark."""
    try:
        cfg0 = a[1][0]
        blocks = cfg0["num_blocks"] if isinstance(cfg0, dict) else cfg0.num_blocks
        bsize = a[0].vllm_config.cache_config.block_size
        return f"blocks={int(blocks)},block_size={int(bsize)}"
    except Exception:
        return ""


def _mem_sample(
    tag: str, where: str, flag: str = "", a: tuple = (), rv: Any = None
) -> None:
    """torch.cuda (+ host where asked) snapshot at a wrapped call, written as
    a mem.* mark. Engine role only; NEVER raises — it runs inline in the
    boot path, and no torch / no CUDA means no marks at all."""
    try:
        if _ROLE != "engine":
            return
        try:
            import torch

            if not torch.cuda.is_available():
                return
            if tag == "prof" and where == "e":
                # peak counters start HERE so mem.prof.x's peak covers only
                # the dummy run, not the weight load that preceded it
                try:
                    torch.cuda.reset_peak_memory_stats()
                except Exception:
                    pass
            free, total = torch.cuda.mem_get_info()
            base = (
                f"alloc={int(torch.cuda.memory_allocated())},"
                f"res={int(torch.cuda.memory_reserved())},"
                f"free={int(free)},total={int(total)}"
            )
        except Exception:
            return
        if tag == "device" and where == "x":
            _record("mem", "mem.device", base)
            if (cfg := _cfg_detail(a)):
                _record("mem", "mem.cfg", cfg)
        elif tag == "load":
            _record("mem", f"mem.load.{where}", base)
        elif tag == "prof":
            if where == "x":
                try:
                    base += f",peak={int(torch.cuda.max_memory_allocated())}"
                except Exception:
                    pass
            _record("mem", f"mem.prof.{where}", base)
            if where == "x" and isinstance(rv, (int, float)):
                # vLLM's OWN kv-cache budget, straight off the return value
                _record("mem", "mem.kvbudget", f"bytes={int(rv)}")
        elif tag == "kv":
            _record("mem", f"mem.kv.{where}", base)
            if where == "x" and (kvc := _kvcfg_detail(a)):
                _record("mem", "mem.kvcfg", kvc)
        elif tag == "gr":
            d = f"{base},phase={flag}" if flag else base
            _record("mem", f"mem.gr.{where}", d)
        elif tag == "final" and where == "x":
            host = _host_mem()
            extra = "".join(f",{k}={v}" for k, v in host.items())
            _record("mem", "mem.final", base + extra)
    except Exception:
        pass


def _mem_host0() -> None:
    """Host-RAM baseline at engine process start; the MemAvailable delta to
    mem.final is what the boot consumed (pinned weights live in host RAM)."""
    try:
        host = _host_mem()
        if host:
            _record("mem", "mem.host0", ",".join(f"{k}={v}" for k, v in host.items()))
    except Exception:
        pass


# ──────────────────────────────────────────────────────────────────────────────
# hook table: module -> (dotted target, mark name, opts)
#   opts: enter_only (wraps never-returning serve loops), detail (kwargs ->
#   detail str), cb (call after the exit mark), mem (memory sample tag —
#   rides the SAME wrapper, so memory adds ZERO new upstream hooks)
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
        dict(qual="Worker.init_device", name="worker.device", mem="device"),
        dict(qual="Worker.load_model", name="worker.load", mem="load"),
        dict(
            qual="Worker.determine_available_memory",
            name="worker.profile",
            mem="prof",
        ),
        dict(qual="Worker.initialize_from_config", name="worker.kv_alloc", mem="kv"),
        dict(
            qual="Worker.compile_or_warm_up_model",
            name="worker.warmup",
            cb=_engine_summary,
            mem="final",
        ),
    ],
    "vllm.v1.worker.gpu.model_runner": [
        dict(
            qual="GPUModelRunner.capture_model",
            name="runner.graphs",
            detail=_profile_flag,
            mem="gr",
        ),
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
    mem = spec.get("mem")

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
            if mem:
                _mem_sample(mem, "e", d, a)
            t0 = time.time()
            try:
                rv = await orig(*a, **k)
                if mem:
                    # the exit sample sees the return value (vLLM's own KV
                    # budget comes out of determine_available_memory); on a
                    # raise there is no x-mark and the renderers degrade
                    _mem_sample(mem, "x", d, a, rv)
                return rv
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
            if mem:
                _mem_sample(mem, "e", d, a)
            t0 = time.time()
            try:
                rv = orig(*a, **k)
                if mem:
                    _mem_sample(mem, "x", d, a, rv)
                return rv
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
        # one full dotted name per spec row: _HOOKED stores exactly this
        # string, so dedup is set membership — the old startswith scan also
        # matched any qual that merely prefixed another hooked qual
        full = f"{module.__name__}.{qual}"
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
                _HOOKED.append(full)
            except Exception as e:
                _record("miss", name, f"{full} ({e!r})")
        if not done and full not in _HOOKED:
            _MISSING.append(full)
            _record("miss", name, full)


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
    global _ROLE
    if os.environ.get("BOOT_TIMING", "1") == "0":
        return
    _ROLE = _detect_role()
    if _ROLE == "none":
        return
    _record("start", "process.start")
    if _ROLE == "engine":
        _mem_host0()
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
