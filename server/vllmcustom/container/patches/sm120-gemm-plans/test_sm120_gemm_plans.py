"""Host-side test for sm120_gemm_plans (stdlib only, no vLLM).

Run:  python3 container/patches/sm120-gemm-plans/test_sm120_gemm_plans.py

Builds a tiny fake vllm.models.qwen4_exp.nvidia.low_latency_gemm with the same shape as
the real seam (a module-level dict keyed by capability tuples, looked up with
.get(capability, {}) by both the init-time enabler and the runtime op), installs the real
import hook, imports the fake through it, and checks the aliasing and every
graceful-degradation path.
"""

import contextlib
import importlib.util
import io
import os
import shutil
import sys
import tempfile
import textwrap
import types

HERE = os.path.dirname(os.path.abspath(__file__))

FAKE = {
    "vllm/__init__.py": "",
    "vllm/models/__init__.py": "",
    "vllm/models/qwen4_exp/__init__.py": "",
    "vllm/models/qwen4_exp/nvidia/__init__.py": "",
    "vllm/models/qwen4_exp/nvidia/low_latency_gemm.py": """
        SM90 = {(4096, 2560): {1: "cfg90"}}
        SM121 = {(248320, 2560): {1: "lm-head-cfg"}, (16384, 2560): {1: "gdn-cfg"}}
        QWEN4_EXP_GEMM_PLANS_BY_CAPABILITY = {(9, 0): SM90, (12, 1): SM121}
        QWEN4_EXP_SM121_GEMM_PLANS = SM121
        def plans_for(capability):  # what both upstream lookups do
            return QWEN4_EXP_GEMM_PLANS_BY_CAPABILITY.get(capability, {})
    """,
}


def load_module(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, "sm120_gemm_plans.py"))
    assert spec is not None and spec.loader is not None
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


def captured(fn, *a, **kw):
    buf = io.StringIO()
    with contextlib.redirect_stderr(buf):
        out = fn(*a, **kw)
    return out, buf.getvalue()


class Cap(tuple):  # vLLM's DeviceCapability is a tuple subclass
    pass


tmpd = tempfile.mkdtemp(prefix="sm120-plans-test-")
try:
    for rel, src in FAKE.items():
        p = os.path.join(tmpd, rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w") as f:
            f.write(textwrap.dedent(src))
    sys.path.insert(0, tmpd)

    # --- SM120_SKINNY unset / 0: install() is a no-op
    os.environ.pop("SM120_SKINNY", None)
    off = load_module("sp_off")
    before = list(sys.meta_path)
    off.install()
    assert sys.meta_path == before, "install() must not hook when SM120_SKINNY is unset"
    os.environ["SM120_SKINNY"] = "0"
    off.install()
    assert sys.meta_path == before, "install() must not hook when SM120_SKINNY=0"

    # --- real install + import through the finder
    os.environ["SM120_SKINNY"] = "1"
    sp = load_module("sm120_gemm_plans")
    sp.install()
    sp.install()  # idempotent
    assert sum(isinstance(f, sp._HookFinder) for f in sys.meta_path) == 1

    import io as _io  # noqa: F401  (keep the redirect_stderr import path obvious)
    mod, err = captured(__import__, "vllm.models.qwen4_exp.nvidia.low_latency_gemm", fromlist=["x"])
    assert sp._applied == {sp.TARGET}, sp._applied
    assert "reusing the SM121 table (2 shapes)" in err, err

    # --- SM120 now resolves to the SM121 table, including for a tuple subclass key
    assert mod.QWEN4_EXP_GEMM_PLANS_BY_CAPABILITY[(12, 0)] is mod.QWEN4_EXP_SM121_GEMM_PLANS
    assert mod.plans_for((12, 0)) == mod.QWEN4_EXP_SM121_GEMM_PLANS
    assert mod.plans_for(Cap((12, 0)))[(248320, 2560)][1] == "lm-head-cfg"
    # --- everything else is untouched
    assert mod.plans_for((9, 0)) == {(4096, 2560): {1: "cfg90"}}
    assert mod.plans_for((12, 1)) is mod.QWEN4_EXP_SM121_GEMM_PLANS
    assert mod.plans_for((8, 9)) == {}, "unknown capabilities still get no plans"
    assert sorted(mod.QWEN4_EXP_GEMM_PLANS_BY_CAPABILITY) == [(9, 0), (12, 0), (12, 1)]

    # --- _apply is idempotent and notes (not raises) once the entry exists
    sp._noted.clear()
    _, err = captured(sp._apply, mod)
    assert "already has a plan table upstream" in err, err
    assert mod.plans_for((12, 0)) is mod.QWEN4_EXP_SM121_GEMM_PLANS

    # --- upstream drift: each degradation is a note, never an exception
    for label, build, needle in (
        ("no table", lambda m: None, "QWEN4_EXP_GEMM_PLANS_BY_CAPABILITY not found"),
        ("no source", lambda m: setattr(m, "QWEN4_EXP_GEMM_PLANS_BY_CAPABILITY", {(9, 0): {1: 1}}),
         "no plan table for capability (12, 1)"),
        ("empty source", lambda m: setattr(m, "QWEN4_EXP_GEMM_PLANS_BY_CAPABILITY", {(12, 1): {}}),
         "no plan table for capability (12, 1)"),
        ("not a dict", lambda m: setattr(m, "QWEN4_EXP_GEMM_PLANS_BY_CAPABILITY", [1]),
         "QWEN4_EXP_GEMM_PLANS_BY_CAPABILITY not found"),
    ):
        sp._noted.clear()
        fake = types.ModuleType(sp.TARGET)
        build(fake)
        _, err = captured(sp._apply, fake)
        assert needle in err, (label, err)

    # --- other modules are ignored
    sp._noted.clear()
    _, err = captured(sp._apply, types.ModuleType("vllm.something.else"))
    assert err == "", err
finally:
    shutil.rmtree(tmpd, ignore_errors=True)

print("ALL TESTS PASS")
