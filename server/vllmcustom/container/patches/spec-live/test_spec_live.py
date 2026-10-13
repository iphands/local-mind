"""Host-side test for spec_live (stdlib only, no vLLM, no FastAPI).

Run:  python3 container/patches/spec-live/test_spec_live.py

Builds a tiny fake `vllm` package (+ a stub `fastapi`) on disk with the same shape
as the real seams: Scheduler/AsyncScheduler with `dynamic_sd_lookup` indexed by batch
size, EngineCore with a getattr-dispatched utility RPC, and
routers.register_api_routers(args, app, ...). Installs the real import hook,
imports the fake package through it, and checks the live lookup, the utility
RPC, the HTTP handlers (all status codes) and every graceful-degradation path.
"""

import asyncio
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
    "fastapi/__init__.py": """
        class Request:
            pass
        class APIRouter:
            def __init__(self):
                self.routes = {}
            def _add(self, method, path):
                def deco(fn):
                    self.routes[(method, path)] = fn
                    return fn
                return deco
            def get(self, path):
                return self._add("GET", path)
            def post(self, path):
                return self._add("POST", path)
    """,
    "fastapi/responses.py": """
        class JSONResponse:
            def __init__(self, content, status_code=200):
                self.content = content
                self.status_code = status_code
    """,
    "vllm/__init__.py": "",
    "vllm/v1/__init__.py": "",
    "vllm/v1/core/__init__.py": "",
    "vllm/v1/core/sched/__init__.py": "",
    "vllm/v1/core/sched/scheduler.py": """
        class Scheduler:
            def __init__(self, lookup=None, num_spec_tokens=0):
                self.dynamic_sd_lookup = lookup
                self.num_spec_tokens = num_spec_tokens
            def spec_tokens_for(self, num_scheduled):  # scheduler.py:1485-1489
                k = self.num_spec_tokens
                if self.dynamic_sd_lookup is not None and num_scheduled > 0:
                    k = self.dynamic_sd_lookup[num_scheduled]
                return k
        class AsyncScheduler(Scheduler):
            def __init__(self, *args, **kwargs):
                super().__init__(*args, **kwargs)
                self.placeholders = [-1] * self.num_spec_tokens
    """,
    "vllm/v1/engine/__init__.py": "",
    "vllm/v1/engine/core.py": """
        class EngineCore:
            def __init__(self, scheduler):
                self.scheduler = scheduler
        class EngineCoreProc(EngineCore):
            def utility(self, name, *args):  # core.py:1675-1688
                return getattr(self, name)(*args)
    """,
    "vllm/entrypoints/__init__.py": "",
    "vllm/entrypoints/launchers/__init__.py": "",
    "vllm/entrypoints/launchers/api_server/__init__.py": "",
    "vllm/entrypoints/launchers/api_server/routers.py": """
        CALLS = []
        def register_api_routers(args, app, supported_tasks, model_config=None):
            CALLS.append((args, supported_tasks))
            return "orig-result"
    """,
}

# lookup for batch sizes 1..4, as build_dynamic_sd_schedule_lookup would produce
TEMPLATE = [0, 3, 3, 3, 3]


def load_module(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, "spec_live.py"))
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


def run(coro):
    return asyncio.run(coro)


class FakeApp:
    def __init__(self):
        self.included = []
        self.state = types.SimpleNamespace()

    def include_router(self, router):
        self.included.append(router)


class FakeRequest:
    def __init__(self, app, body=None, bad_json=False):
        self.app = app
        self._body = body
        self._bad = bad_json

    async def json(self):
        if self._bad:
            raise ValueError("not json")
        return self._body


class FakeClient:
    """engine_client.engine_core: call_utility_async -> EngineCoreProc.utility."""

    def __init__(self, core, fail=None):
        self.core = core
        self.fail = fail
        self.calls = []

    async def call_utility_async(self, method, *args):
        self.calls.append((method, args))
        if self.fail == method:
            raise RuntimeError(f"Call to {method} method failed: boom")
        return self.core.utility(method, *args)


tmpd = tempfile.mkdtemp(prefix="spec-live-test-")
try:
    for rel, src in FAKE.items():
        p = os.path.join(tmpd, rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w") as f:
            f.write(textwrap.dedent(src))
    sys.path.insert(0, tmpd)

    # --- SPEC_LIVE unset / not "1": install() is a no-op
    os.environ.pop("SPEC_LIVE", None)
    off = load_module("sl_off")
    before = list(sys.meta_path)
    off.install()
    assert sys.meta_path == before, "install() must not hook when SPEC_LIVE is unset"
    os.environ["SPEC_LIVE"] = "0"
    off.install()
    assert sys.meta_path == before, "install() must not hook when SPEC_LIVE=0"

    # --- real install + import through the finder
    os.environ["SPEC_LIVE"] = "1"
    os.environ["SPEC_LIVE_K"] = "5"
    sl = load_module("spec_live")
    sl.install()
    sl.install()  # idempotent
    assert sum(isinstance(f, sl._HookFinder) for f in sys.meta_path) == 1

    import vllm.entrypoints.launchers.api_server.routers as routers  # noqa: E402
    import vllm.v1.core.sched.scheduler as sched  # noqa: E402
    import vllm.v1.engine.core as core_mod  # noqa: E402

    assert sl._applied == set(sl.TARGETS), sl._applied
    assert getattr(sched.Scheduler.__init__, sl._MARK, False)
    assert getattr(routers.register_api_routers, sl._MARK, False)
    assert hasattr(core_mod.EngineCore, "set_spec_tokens")

    # --- scheduler: Dynamic SD lookup becomes the live lookup (K from SPEC_LIVE_K)
    s, err = captured(sched.Scheduler, list(TEMPLATE), 8)
    assert isinstance(s.dynamic_sd_lookup, sl.LiveLookup), type(s.dynamic_sd_lookup)
    assert "K=5 (max 8)" in err, err
    lk = s.dynamic_sd_lookup
    assert [lk[b] for b in (1, 2, 3, 4)] == [5, 5, 5, 5], "every batch size answers the live K"
    assert len(lk) == len(TEMPLATE) and list(lk) == TEMPLATE, "length / iteration unchanged"
    assert lk[1:3] == TEMPLATE[1:3], "slices behave like a list"
    assert s.spec_tokens_for(3) == 5 and s.spec_tokens_for(0) == 8, "scheduler semantics kept"

    # --- the subclass path (async scheduling) is covered by the base-class hook, once
    a, err = captured(sched.AsyncScheduler, list(TEMPLATE), 8)
    assert isinstance(a.dynamic_sd_lookup, sl.LiveLookup) and a.placeholders == [-1] * 8
    assert err.count("live draft depth on") == 1, err

    # --- SPEC_LIVE_K clamps / garbage falls back to the maximum
    os.environ["SPEC_LIVE_K"] = "99"
    assert captured(sched.Scheduler, list(TEMPLATE), 8)[0].dynamic_sd_lookup.k == 8
    os.environ["SPEC_LIVE_K"] = "-3"
    assert captured(sched.Scheduler, list(TEMPLATE), 8)[0].dynamic_sd_lookup.k == 0
    os.environ["SPEC_LIVE_K"] = "abc"
    assert captured(sched.Scheduler, list(TEMPLATE), 8)[0].dynamic_sd_lookup.k == 8
    os.environ["SPEC_LIVE_K"] = "5"

    # --- a scheduler that is NOT running Dynamic SD is left alone, noted once
    plain, err = captured(sched.Scheduler, None, 4)
    assert plain.dynamic_sd_lookup is None and "not running Dynamic SD" in err, err
    _, err2 = captured(sched.Scheduler, None, 4)
    assert err2 == "", "the note must print once"
    nospec, _ = captured(sched.Scheduler, list(TEMPLATE), 0)
    assert type(nospec.dynamic_sd_lookup) is list, "num_spec_tokens=0: untouched"

    # --- engine core utility RPC (same getattr dispatch as EngineCoreProc)
    core = core_mod.EngineCoreProc(s)
    assert core.utility("get_spec_tokens") == [5, 8]
    assert core.utility("set_spec_tokens", 3) == [5, 3, 8]
    assert core.utility("get_spec_tokens") == [3, 8]
    assert s.spec_tokens_for(2) == 3, "the scheduler sees the new K on its next step"
    assert core.utility("set_spec_tokens", 0) == [3, 0, 8] and s.spec_tokens_for(4) == 0
    assert core.utility("set_spec_tokens", 8) == [0, 8, 8] and s.spec_tokens_for(1) == 8
    for bad in (-1, 9, 3.5, "4", None, True):
        try:
            core.utility("set_spec_tokens", bad)
            raise AssertionError(f"accepted {bad!r}")
        except ValueError:
            pass
    assert core.utility("get_spec_tokens") == [8, 8], "a rejected value must not change K"

    # --- engine core whose scheduler has no live lookup
    dead = core_mod.EngineCoreProc(plain)
    assert dead.utility("get_spec_tokens") == []
    try:
        dead.utility("set_spec_tokens", 2)
        raise AssertionError("expected RuntimeError")
    except RuntimeError as e:
        assert "not enabled" in str(e)

    # --- routes are registered by wrapping register_api_routers
    app = FakeApp()
    out, err = captured(routers.register_api_routers, "ARGS", app, ("generate",))
    assert out == "orig-result" and routers.CALLS[-1] == ("ARGS", ("generate",)), "original not called"
    assert len(app.included) == 1 and "registered GET/POST /spec_tokens" in err, err
    router = app.included[0]
    get_h = router.routes[("GET", "/spec_tokens")]
    post_h = router.routes[("POST", "/spec_tokens")]
    app2 = FakeApp()
    captured(routers.register_api_routers, args="A", app=app2, supported_tasks=())
    assert len(app2.included) == 1, "keyword call path"

    def wired(core_obj, fail=None):
        a = FakeApp()
        a.state.engine_client = types.SimpleNamespace(engine_core=FakeClient(core_obj, fail))
        return a

    live = wired(core)
    r = run(get_h(FakeRequest(live)))
    assert r == {"current": 8, "max": 8}, r
    r, _ = captured(lambda: run(post_h(FakeRequest(live, {"num_speculative_tokens": 4}))))
    assert r == {"previous": 8, "current": 4, "max": 8}, r
    assert run(get_h(FakeRequest(live))) == {"current": 4, "max": 8}
    r, _ = captured(lambda: run(post_h(FakeRequest(live, {"num_speculative_tokens": 0}))))
    assert r["current"] == 0

    for body in ({"num_speculative_tokens": 9}, {"num_speculative_tokens": -1}):
        r = run(post_h(FakeRequest(live, body)))
        assert r.status_code == 400 and r.content["max"] == 8, (body, r.content)
    for body in ({"num_speculative_tokens": "4"}, {"num_speculative_tokens": 2.5},
                 {"num_speculative_tokens": True}, {}, [], None):
        r = run(post_h(FakeRequest(live, body)))
        assert r.status_code == 400, (body, r.status_code)
    r = run(post_h(FakeRequest(live, bad_json=True)))
    assert r.status_code == 400
    assert run(get_h(FakeRequest(live))) == {"current": 0, "max": 8}, "rejected posts changed K"
    # out-of-range was refused before the RPC: only get_spec_tokens / the 2 valid sets were sent
    sent = [m for m, _ in live.state.engine_client.engine_core.calls if m == "set_spec_tokens"]
    assert len(sent) == 2, sent

    off_app = wired(dead)
    assert run(get_h(FakeRequest(off_app))).status_code == 409
    assert run(post_h(FakeRequest(off_app, {"num_speculative_tokens": 2}))).status_code == 409

    boom_get = wired(core, fail="get_spec_tokens")
    assert run(get_h(FakeRequest(boom_get))).status_code == 500
    boom_set = wired(core, fail="set_spec_tokens")
    assert run(post_h(FakeRequest(boom_set, {"num_speculative_tokens": 1}))).status_code == 500

    # --- upstream drift: every degradation is a note, never an exception
    sl._noted.clear()
    m = types.ModuleType(sl.ENGINE_CORE)
    _, err = captured(sl._apply, m)
    assert "EngineCore not found" in err, err
    sl._noted.clear()
    m = types.ModuleType(sl.ENGINE_CORE)
    m.EngineCore = type("EngineCore", (), {"set_spec_tokens": lambda self, k: None})
    _, err = captured(sl._apply, m)
    assert "already has set_spec_tokens" in err, err
    m = types.ModuleType(sl.ROUTERS)
    _, err = captured(sl._apply, m)
    assert "register_api_routers not found" in err, err
    m = types.ModuleType(sl.SCHEDULER)
    m.Scheduler = object()  # no usable __init__ wrapping target -> must not raise
    captured(sl._apply, m)
    sl._noted.clear()
    m = types.ModuleType(sl.SCHEDULER)
    _, err = captured(sl._apply, m)
    assert "Scheduler not found" in err, err

    # --- a router that cannot be built (e.g. FastAPI missing) leaves the app serving
    saved = sys.modules.pop("fastapi"), sys.modules.pop("fastapi.responses")
    sys.path.remove(tmpd)
    try:
        app3 = FakeApp()
        out, err = captured(routers.register_api_routers, "A", app3, ())
        assert out == "orig-result" and app3.included == [] and "could not register" in err, err
    finally:
        sys.path.insert(0, tmpd)
        sys.modules["fastapi"], sys.modules["fastapi.responses"] = saved

    # --- _apply is idempotent (a second pass must not double-wrap)
    w1, w2 = sched.Scheduler.__init__, routers.register_api_routers
    sl._apply(sched)
    sl._apply(routers)
    assert sched.Scheduler.__init__ is w1 and routers.register_api_routers is w2
finally:
    shutil.rmtree(tmpd, ignore_errors=True)

print("ALL TESTS PASS")
