"""Host-side test for boot_timing's pure render logic (stdlib only, no vLLM).

Run:  python3 container/patches/boot-timing/test_boot_timing.py
Synthesizes a boot that mirrors the real Qwen3.8-Flash-Next timeline
(API pid 1, EngineCore pid 381, weights ~4m45s, open serve spans) and checks
the report renders every section, the nesting, and the drift note.
"""

import importlib.util
import os
import sys
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

rows = []


def add(pid, role, kind, name, dt, detail=""):
    rows.append(Row(T + dt, pid, role, kind, name, detail))


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
    "B O O T   T I M I N G",
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
]
missing = [c for c in checks if c not in report]
assert not missing, f"report missing: {missing}"

assert bt._fmt_dur(59.4) == "59s" and bt._fmt_dur(5) == "5.0s" and bt._fmt_dur(125) == "2m05s"
assert bt._fmt_off(65) == "+1m05s"

# marks-file roundtrip through the real writer/parser
os.environ["VLLM_BOOT_TIMING_DIR"] = "/tmp/opencode/bt-test"
importlib.reload(bt)
setattr(bt, "_ROLE", "engine")
bt._record("enter", "worker.load")
time.sleep(0.01)
bt._record("exit", "worker.load")
back = bt.read_marks("/tmp/opencode/bt-test/marks.log")
assert any(r.kind == "exit" and r.name == "worker.load" for r in back), back
print("ALL TESTS PASS")
