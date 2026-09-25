"""Host-side test for boot_timing's pure render logic (stdlib only, no vLLM).

Run:  python3 container/patches/boot-timing/test_boot_timing.py
Synthesizes a boot that mirrors the real Qwen3.8-Flash-Next timeline
(API pid 1, EngineCore pid 381, weights ~4m45s, open serve spans) plus a
consistent set of mem.* marks, and checks the report renders the timing
waterfall, the GPU/host memory attribution, the headroom arithmetic
(NUMERICALLY, not just by key presence), the nesting fix, the ghost-process
guard, and the graceful-degradation paths.

Set BT_MOCK_OUT=<path> to also write the full rendered report to <path>.
"""

import importlib.util
import math
import os
import shutil
import sys
import tempfile
import time

spec = importlib.util.spec_from_file_location(
    "boot_timing", os.path.join(os.path.dirname(__file__), "boot_timing.py")
)
assert spec is not None and spec.loader is not None
bt = importlib.util.module_from_spec(spec)
sys.modules["boot_timing"] = bt
spec.loader.exec_module(bt)

Row = bt.Row
T = 1_000.0
G = 2**30


def B(gib: float) -> int:
    return round(gib * G)


rows = []


def add(pid, role, kind, name, dt, detail=""):
    rows.append(Row(T + dt, pid, role, kind, name, detail))


def base(a, r, f, t=96.0):
    return f"alloc={B(a)},res={B(r)},free={B(f)},total={B(t)}"


# APIServer: start 0, serve entry 18, engine_build 25..603 (mm warmup inside,
# then the core-ready wait), app_state, uvicorn; serve/engine_build left open.
add(1, "api", "start", "process.start", 0)
add(1, "api", "enter", "api.serve", 18)
add(1, "api", "enter", "api.engine_build", 25)
add(1, "api", "enter", "api.mm_warmup", 26)
add(1, "api", "exit", "api.mm_warmup", 44)
add(1, "api", "enter", "api.core_ready", 45)
add(1, "api", "exit", "api.core_ready", 588)
add(1, "api", "exit", "api.engine_build", 589)
add(1, "api", "enter", "api.app_state", 590)
add(1, "api", "exit", "api.app_state", 593)
add(1, "api", "enter", "api.uvicorn", 594)
add(1, "api", "exit", "api.uvicorn", 596)
add(1, "api", "miss", "gone.away", 30, "vllm.gone.away")

# EngineCore: start 30, everything nested in core.ctor; graphs called twice
# (profile probe inside worker.profile, final inside worker.warmup).
add(381, "engine", "start", "process.start", 30)
add(381, "engine", "enter", "core.proc", 44)
add(381, "engine", "enter", "core.ctor", 46)
add(381, "engine", "enter", "worker.ctor", 47)
add(381, "engine", "exit", "worker.ctor", 52)
add(381, "engine", "enter", "worker.device", 52)
add(381, "engine", "exit", "worker.device", 57)
add(381, "engine", "enter", "worker.load", 57)
add(381, "engine", "exit", "worker.load", 342)
add(381, "engine", "enter", "worker.profile", 343)
add(381, "engine", "enter", "runner.graphs", 380, "profile")
add(381, "engine", "exit", "runner.graphs", 391, "profile")
add(381, "engine", "exit", "worker.profile", 398)
add(381, "engine", "enter", "worker.kv_alloc", 399)
add(381, "engine", "exit", "worker.kv_alloc", 400)
add(381, "engine", "enter", "worker.warmup", 401)
add(381, "engine", "enter", "warmup.kernels", 401)
add(381, "engine", "exit", "warmup.kernels", 407)
add(381, "engine", "enter", "warmup.autotune", 407)
add(381, "engine", "exit", "warmup.autotune", 423)
add(381, "engine", "enter", "runner.graphs", 424, "final")
add(381, "engine", "exit", "runner.graphs", 426, "final")
add(381, "engine", "exit", "worker.warmup", 428)
add(381, "engine", "exit", "core.ctor", 428)

# mem.* marks, a fully consistent 96 GiB-card boot (GiB chosen so every
# displayed value and percent is far from a .x5 rounding boundary):
#   ctx 0.9 · weights 47.3 · act.peak 1.6 · kv 38.9 · graphs(final) 2.5
#   overhead 1.1 · outside 0.9 · headroom 3.4 · util 0.98
add(381, "engine", "mem", "mem.host0", 30, f"avail={B(119.6)},mtotal={B(125.3)},rss={B(1.2)}")
add(381, "engine", "mem", "mem.device", 57, base(0.9, 1.0, 95.1))
add(381, "engine", "mem", "mem.cfg", 57, "util=0.98,tokens=8192")
add(381, "engine", "mem", "mem.load.e", 57, base(0.9, 1.0, 95.1))
add(381, "engine", "mem", "mem.load.x", 342, base(48.2, 49.0, 47.8))
add(381, "engine", "mem", "mem.prof.e", 343, base(48.2, 49.0, 47.8))
add(381, "engine", "mem", "mem.prof.x", 398, base(48.2, 52.0, 44.0) + f",peak={B(49.8)}")
add(381, "engine", "mem", "mem.kvbudget", 398, f"bytes={B(38.9)}")
add(381, "engine", "mem", "mem.kv.e", 399, base(48.2, 52.0, 44.0))
add(381, "engine", "mem", "mem.kv.x", 400, base(87.1, 88.0, 8.9))
add(381, "engine", "mem", "mem.kvcfg", 400, "blocks=13486,block_size=32")
add(381, "engine", "mem", "mem.gr.e", 380, base(48.2, 52.0, 44.0) + ",phase=profile")
add(381, "engine", "mem", "mem.gr.x", 391, base(48.6, 52.0, 44.0) + ",phase=profile")
add(381, "engine", "mem", "mem.gr.e", 424, base(87.1, 88.0, 8.9) + ",phase=final")
add(381, "engine", "mem", "mem.gr.x", 426, base(89.6, 90.0, 6.5) + ",phase=final")
add(
    381, "engine", "mem", "mem.final", 428,
    base(89.6, 90.7, 4.4) + f",rss={B(12.1)},avail={B(24.0)},mtotal={B(125.3)}",
)

report = bt.format_report(rows, now=T + 597.0)
print(report)
print()

spans = bt.match_spans([r for r in rows if r.pid == 381 and r.kind in ("enter", "exit")], T + 597)
graphs = [s for s in spans if s.name == "runner.graphs"]
assert len(graphs) == 2, graphs
assert {g.detail for g in graphs} == {"profile", "final"}, graphs
serve = next(s for s in bt.match_spans([r for r in rows if r.pid == 1], T + 597) if s.name == "api.serve")
assert abs(serve.t1 - (T + 597)) < 1e-9, "open span must end at report time"

checks = [
    "P O S T - S T A R T U P   S T A T S",
    "TOTAL",
    "APIServer (pid 1)",
    "EngineCore+worker (pid 381)",
    "python imports + bootstrap",
    "WEIGHT LOAD (model build + shards)",
    "memory profiling (dummy run)",
    "cudagraph capture (memory probe)",
    "cudagraph capture (final)",
    "flashinfer autotune",
    "KV cache allocation",
    "uvicorn bind + lifespan",
    "weight load = ",
    "% of total boot",
    "hooks NOT applied",
    "vllm.gone.away",
    # GPU memory section: header, budget, all six bars, outside-budget line
    " ── GPU memory — 96.0 GiB total (pid 381) ──",
    "vLLM budget (gpu_memory_utilization=0.98)",
    "94.1 GiB",
    "98.0%",
    "model weights (NVFP4, PLE excluded)",
    "47.3 GiB",
    " of budget",
    "KV cache  (~431k tokens @ full ctx)",
    "38.9 GiB",
    "CUDA graphs (final capture)",
    "2.5 GiB",
    "CUDA context + torch runtime",
    "allocator overhead (reserved − allocated)",
    "budget headroom (granted but unused)",
    "outside budget (driver, other)",
    # host RAM section
    " ── host RAM ──",
    "host RAM consumed by boot (pinned weights)",
    "95.6 GiB",
    "RSS: engine 12.1 GiB",
    "MemAvailable at serve-ready",
    "24.0 GiB / 125.3 GiB",
    # headroom section
    " ── headroom ──",
    "gpu_memory_utilization = 0.98 now",
    "could drop to",
    "hard floor",
    "trade rate",
    "GiB per 0.01 util",
    "floor build-up:",
    "+ min-KV 1.0",
    "caveat: act.peak measured at max_num_batched_tokens=8192",
    # memory notes
    "profiling peak 1.6 GiB was transient (dummy run)",
    "KV cross-check",
    "agree (within 0.5 GiB) ✓",
]
missing = [c for c in checks if c not in report]
assert not missing, f"report missing: {missing}"

# every GPU bar's percent, derived from the SAME byte inputs (independent
# re-derivation: bytes here, renderer's own float path there)
total_b = B(96.0)
budget_b = 0.98 * total_b
w = B(48.2) - B(0.9)
kv = B(87.1) - B(48.2)
gr = B(89.6) - B(87.1)
ctx = total_b - B(95.1)
oh = B(90.7) - B(89.6)
outside = total_b - B(4.4) - B(90.7)
final_used = total_b - B(4.4)
for gib in (w, kv, gr, ctx, oh):
    assert f"{gib / budget_b * 100:.1f}%" in report, gib
assert f"{w / budget_b * 100:.1f}% of budget" in report
headroom_b = budget_b - (w + kv + gr + ctx + oh)
assert headroom_b > 0 and f"{headroom_b / budget_b * 100:.1f}%" in report

# headroom math, asserted NUMERICALLY: safe == ceil2dp of the byte inputs
non_kv = final_used - kv
safety = max(1.5 * G, 0.02 * total_b)
hard = non_kv / total_b
safe = math.ceil((non_kv + G + safety) / total_b * 100) / 100
assert safe == 0.58, safe          # ceil2dp((52.7 + 1.0 + 1.92) / 96)
assert hard == 0.5489583333333333 or abs(hard - 0.55) < 5e-3, hard
freed = (0.98 - safe) * total_b
kv_at_safe = max(0.0, safe * total_b - non_kv)
# the safe floor's byte math must honor the section's own min-KV promise
# (the old kv - freed form came out 0.5 GiB here: freed ate the granted-but-
# unused budget slack first)
assert kv_at_safe > G, kv_at_safe
assert f"could drop to  {safe:.2f} safe" in report
assert f"hard floor     {hard:.2f}" in report
assert f"-> frees {freed / G:.1f} GiB · KV {kv / G:.1f} -> {kv_at_safe / G:.1f} GiB" in report
assert f"(~{int(431552.0 * kv_at_safe / kv) // 1000}k tok)" in report

# --- load-rate suffix ----------------------------------------------------
# numeric: 47.3 GiB GPU-placed over the 285 s worker.load span. Whitelist,
# not threshold: KV's "rate" would measure cudaMalloc caching behavior and
# the host line's would measure cudaHostRegister page-lock fill — neither
# is a load rate, so only worker.load may carry an " @ ".
mibs = w / G * 1024 / 285
assert f" @ {mibs:.0f} MiB/s ({w / G:.1f} GiB)" in report, mibs
for ln in report.splitlines():
    if "█" in ln and ln.lstrip().startswith("+"):
        assert ln.count(" @ ") <= (1 if "WEIGHT LOAD" in ln else 0), ln
# _fmt_rate: honest binary labels; the 1023.5 branch keeps the rounded
# MiB/s leg from printing "1024 MiB/s" one tick before "1.000 GiB/s"
assert bt._fmt_rate(2**20 * 1023.4, 1.0) == "1023 MiB/s"
assert bt._fmt_rate(2**20 * 1023.6, 1.0) == "1.000 GiB/s"
assert bt._fmt_rate(2**30 * 2, 1.0) == "2.000 GiB/s"
assert bt._fmt_rate(0, 10) is None and bt._fmt_rate(-5, 10) is None
assert bt._fmt_rate(100, 0.04) is None and bt._fmt_rate(None, 10) is None
# containment pairing: the marks must bracket the span, not merely share
# its name — a second load cycle must not borrow the first cycle's bytes
pm = bt._pid_mem([r for r in rows if r.pid == 381])
assert bt._rate_suffix(pm, bt.Span("worker.load", "", T + 57, T + 342)) == (
    f" @ {mibs:.0f} MiB/s ({w / G:.1f} GiB)")
assert bt._rate_suffix(pm, bt.Span("worker.load", "", T + 50, T + 60)) == ""
pm_nox = bt._pid_mem([r for r in rows
                      if r.pid == 381 and r.name != "mem.load.x"])
assert bt._rate_suffix(pm_nox, bt.Span("worker.load", "", T + 57, T + 342)) == ""

# zero-slack sub-case: with budget == final_used (no granted-but-unused
# slack) the corrected formula and the naive kv - freed must agree
u0_zs = final_used / total_b
rows_zs = [
    r._replace(detail=f"util={u0_zs},tokens=8192") if r.name == "mem.cfg" else r
    for r in rows
]
report5 = bt.format_report(rows_zs, now=T + 597.0)
kvas_naive = kv - (u0_zs - safe) * total_b
line_zs = report5.split("could drop to")[1].splitlines()[0]
assert f"KV {kv / G:.1f} -> {kvas_naive / G:.1f} GiB" in line_zs, line_zs
assert f"trade rate     {0.01 * total_b / G:.2f} GiB per 0.01 util" in report

# negative-path notes must be ABSENT for consistent data, and the unfired-
# spans note must be suppressed when every LABELS name was observed
for absent in ("memory: insufficient marks", "GPU accounting drift",
               "oversubscribed", "spans not seen this boot"):
    assert absent not in report, absent

# nesting regression (fix a): same-(name, detail) calls nest, so pairing is
# LIFO — parent(1)→child(2)→child exit(3)→parent exit(4) must close child
# first; FIFO would cross-pair into (1,3)+(2,4)
nest = [
    Row(T + 1, 7, "engine", "enter", "worker.load", ""),
    Row(T + 2, 7, "engine", "enter", "worker.load", ""),
    Row(T + 3, 7, "engine", "exit", "worker.load", ""),
    Row(T + 4, 7, "engine", "exit", "worker.load", ""),
]
assert set(bt.match_spans(nest, T + 100)) == {
    bt.Span("worker.load", "", T + 2, T + 3),   # inner call
    bt.Span("worker.load", "", T + 1, T + 4),   # enclosing call
}

# ghost guard (fix f), minimal: start-only pid (role false-positive) must
# name its emptiness — no header, no imports bar, no bars at all
ghost = bt.format_report([Row(T, 202, "api", "start", "process.start", "")], T + 600)
assert "APIServer (pid 202): no phases recorded" in ghost
assert "── APIServer (pid 202)" not in ghost
assert "python imports + bootstrap" not in ghost
assert "█" not in ghost

# ghost guard, real-marks-shaped (real_marks.log pid 202 was exactly this:
# transient helper, vllm-ish cmdline, start-only) at now=start+600
grows = []


def gadd(pid, role, kind, name, dt, detail=""):
    grows.append(Row(T + dt, pid, role, kind, name, detail))


gadd(1, "api", "start", "process.start", 0)
gadd(1, "api", "enter", "api.serve", 10)
gadd(1, "api", "enter", "api.engine_build", 10.006)
gadd(1, "api", "exit", "api.engine_build", 10.007)
gadd(202, "api", "start", "process.start", 12)
gadd(381, "engine", "start", "process.start", 27)
gadd(381, "engine", "enter", "core.proc", 32)
gadd(381, "engine", "enter", "worker.load", 35)
gadd(381, "engine", "exit", "worker.load", 340)
report2 = bt.format_report(grows, now=T + 600.0)
assert "APIServer (pid 202): no phases recorded" in report2
# only the two pids WITH spans get an imports bar — the ghost's would have
# measured start→report-time (the production incident: a 9m01s bar)
assert report2.count("python imports + bootstrap") == 2
lines2 = report2.splitlines()
gi = next(i for i, ln in enumerate(lines2) if "no phases recorded" in ln)
assert "█" not in lines2[gi + 1]
# mem-marks-absent case: no GPU section, one explanatory note, no crash
assert "memory: insufficient marks" in report2
assert "GPU memory" not in report2
# ... and the waterfall degrades with it: span rendered, rate silent
gl = next(ln for ln in lines2 if "WEIGHT LOAD" in ln)
assert " @ " not in gl, gl
# unfired-spans note: this boot never reached most LABELS phases
assert "spans not seen this boot: " in report2
assert "api.app_state" in report2

# malformed mem detail (garbage/empty values) -> every line skips gracefully
mal = [
    Row(T, 1, "api", "start", "process.start", ""),
    Row(T + 1, 381, "engine", "start", "process.start", ""),
    Row(T + 2, 381, "engine", "mem", "mem.final", "alloc=,res=garbage,total=,avail=abc"),
    Row(T + 3, 381, "engine", "mem", "mem.kvcfg", "blocks=,block_size=xyz"),
]
report3 = bt.format_report(mal, now=T + 10)
assert "GPU memory" not in report3          # nothing derivable -> no section
assert "memory: insufficient marks" not in report3  # marks exist, just useless
# garbage kvcfg values only -> KV line keeps its absolute value, no token clause
rows_nokv = [
    r if r.name != "mem.kvcfg"
    else Row(T + 400, 381, "engine", "mem", "mem.kvcfg", "blocks=,block_size=abc")
    for r in rows
]
report4 = bt.format_report(rows_nokv, now=T + 597.0)
assert "KV cache" in report4 and "KV cache  (~" not in report4
assert "(~" not in report4.split("could drop to")[1].splitlines()[0]

# RSS line: a missing/zero-unknown term is omitted with its separator, and
# the whole line vanishes when both are unknown ("api 0.0 GiB" is noise)
m_rss = bt._mem_map([rows[i] for i in range(len(rows)) if rows[i].name == "mem.final"])
assert next(ln for ln in bt._render_host(m_rss, None) if "RSS:" in ln).strip() \
    == "RSS: engine 12.1 GiB"
m_rss0 = bt._mem_map([Row(T, 381, "engine", "mem", "mem.host0", f"avail={B(119.6)}")])
assert not any("RSS:" in ln for ln in bt._render_host(m_rss0, 0.0))

assert bt._fmt_dur(59.4) == "59s" and bt._fmt_dur(5) == "5.0s" and bt._fmt_dur(125) == "2m05s"
assert bt._fmt_off(65) == "+1m05s"

# marks-file roundtrip through the real writer/parser, in a throwaway dir
tmpd = tempfile.mkdtemp(prefix="bt-test-")
try:
    os.environ["VLLM_BOOT_TIMING_DIR"] = tmpd
    importlib.reload(bt)
    setattr(bt, "_ROLE", "engine")
    bt._record("enter", "worker.load")
    time.sleep(0.01)
    bt._record("exit", "worker.load")
    # fix (c): detail is sanitized at write time — no field/row forgery
    bt._record("mem", "mem.cfg", "util=0.98,\ttab\nline")
    marks_path = os.path.join(tmpd, "marks.log")
    with open(marks_path) as f:
        fields = [ln.split("\t") for ln in f.read().splitlines()]
    assert all(len(p) == 6 for p in fields), fields
    back = bt.read_marks(marks_path)
    assert any(r.kind == "exit" and r.name == "worker.load" for r in back), back
    memrow = next(r for r in back if r.kind == "mem")
    assert memrow.detail == "util=0.98, tab line", memrow.detail
finally:
    shutil.rmtree(tmpd, ignore_errors=True)

if (mock := os.environ.get("BT_MOCK_OUT")):
    with open(mock, "w") as f:
        f.write(report + "\n")

print("ALL TESTS PASS")
