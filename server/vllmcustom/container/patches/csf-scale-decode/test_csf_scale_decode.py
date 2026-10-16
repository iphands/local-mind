"""Host-side test for csf_scale_decode (stdlib only, no vLLM, no torch).

Run:  python3 container/patches/csf-scale-decode/test_csf_scale_decode.py

The codec core is tested through the pure-Python ReferenceBackend against an
independent ENCODER written here (the inverse of what the patch does, copied in
shape from b12x tests/quantization/test_nvfp4_csf.py::_fixture codec 0). The
torch backend is the same arithmetic vectorised; it cannot run on this host and
is compared against the reference on real shards in the image (Plan 04 T2).
The hook is tested on a tiny fake `vllm` package on disk, including coexistence
with draft-load-filter, which hooks the same module.
"""

import contextlib
import importlib
import importlib.util
import io
import os
import random
import shutil
import sys
import tempfile
import textwrap
import types

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import csf_scale_decode as csd  # noqa: E402

HIDDEN, INTER = 64, 32  # gate/up plane 32x4 (96 B fixed), down plane 64x2 (128 B fixed)
rng = random.Random(7)


def encode(plane, rows, cols):
    """Independent encoder: row base = min, nibble = value - base if in 0..15 else exception."""
    stream, words = bytearray(), []
    for slab in range(rows // 16):
        bases = []
        for i in range(16):
            row = plane[(slab * 16 + i) * cols : (slab * 16 + i + 1) * cols]
            bases.append(min(min(row), csd._MAX_BASE))
        stream += bytes(bases)
        for i in range(16):
            r = slab * 16 + i
            nib = []
            for c in range(cols):
                d = plane[r * cols + c] - bases[i]
                if 0 <= d <= 15:
                    nib.append(d)
                else:
                    nib.append(0)
                    words.append((r * cols + c) | (plane[r * cols + c] << 24))
            stream += bytes(nib[j] | (nib[j + 1] << 4) for j in range(0, cols, 2))
    return bytes(stream), words


def make_plane(rows, cols, spread=15, outliers=0):
    base = [rng.randrange(0, 200) for _ in range(rows)]
    p = bytearray(base[r] + rng.randrange(0, spread + 1) for r in range(rows) for _ in range(cols))
    for _ in range(outliers):
        p[rng.randrange(len(p))] = rng.randrange(0, 256)
    return bytes(p)


checks = []


def check(name, cond, detail=""):
    checks.append((name, bool(cond), detail))
    print(("ok   " if cond else "FAIL ") + name + (f"  [{detail}]" if detail and not cond else ""))


def raises(fn, exc=ValueError):
    try:
        fn()
    except exc:
        return True
    except Exception:
        return False
    return False


class Cfg:
    class T:
        hidden_size = HIDDEN
        moe_intermediate_size = INTER

    hf_text_config = T


def planes(n_experts=3):
    """{base: (plane, rows, cols)} for gate/up/down of a few experts + their stream entries."""
    out = {}
    for e in range(n_experts):
        for proj, (r, c) in (("gate_proj", (INTER, HIDDEN // 16)), ("up_proj", (INTER, HIDDEN // 16)),
                             ("down_proj", (HIDDEN, INTER // 16))):
            out[f"model.layers.0.mlp.experts.{e}.{proj}.weight_scale"] = (make_plane(r, c, outliers=3), r, c)
    return out


def shard_order(pl):
    """Safetensors lists U32 tensors first, then U8: every exceptions before any fixed."""
    items = []
    for base, (plane, r, c) in pl.items():
        fixed, words = encode(plane, r, c)
        items.append((base, fixed, words))
    stream = [("model.layers.0.input_layernorm.weight", "norm"), ("lm_head.weight", "head")]
    stream += [(b + csd.SUFFIX_EXC, w) for b, _, w in items]
    stream += [(b + ".extra_plain", "plain") for b, _, _ in items[:1]]
    stream += [(b + csd.SUFFIX_FIXED, f) for b, f, _ in items]
    return stream


# ------------------------------------------------------------------ codec

p = make_plane(32, 4, outliers=5)
fx, wd = encode(p, 32, 4)
check("reference decode inverts the independent encoder (with exceptions)",
      bytes(csd.decode_reference(fx, wd, 32, 4)) == p and len(wd) > 0)
p2 = make_plane(64, 2, spread=15, outliers=0)
fx2, wd2 = encode(p2, 64, 2)
check("no-exception plane roundtrips", bytes(csd.decode_reference(fx2, wd2, 64, 2)) == p2 and wd2 == [])
check("nibble order: low nibble is the EVEN column",
      csd.decode_reference(bytes([10] * 16) + bytes([0x21] * 16), [], 16, 2) == bytearray([11, 12] * 16))
check("exception overrides the decoded byte",
      csd.decode_reference(bytes([10] * 16) + bytes([0x00] * 16), [3 | (200 << 24)], 16, 2)[3] == 200)
bad = bytearray(fx); bad[0] = 241
check("base above 240 raises", raises(lambda: csd.decode_reference(bytes(bad), wd, 32, 4)))
check("unsorted exception positions raise", raises(lambda: csd.decode_reference(fx, wd[::-1], 32, 4)) or len(wd) < 2)
check("out-of-range exception position raises", raises(lambda: csd.decode_reference(fx, [32 * 4 | (1 << 24)], 32, 4)))
check("wrong fixed length raises", raises(lambda: csd.decode_reference(fx[:-1], wd, 32, 4)))

# ------------------------------------------------------------------ stream

csd._backend = csd.ReferenceBackend
pl = planes()
stream = shard_order(pl)
got = list(csd.decode_stream(iter(stream), Cfg))
names = [n for n, _ in got]
check("no csf name leaks", not any(n.endswith((csd.SUFFIX_FIXED, csd.SUFFIX_EXC)) for n in names))
check("plain tensors pass through unchanged and first",
      got[:2] == stream[:2] and ("model.layers.0.experts.0.gate_proj.weight_scale.extra_plain" in names or
                               any(n.endswith(".extra_plain") for n in names)))
dec = {n: bytes(t) for n, t in got if n in pl}
check("every pair decodes to its source plane (all 9, exceptions-before-fixed order)",
      len(dec) == len(pl) and all(dec[b] == pl[b][0] for b in pl))
gate = "model.layers.0.mlp.experts.0.gate_proj.weight_scale"
fg, wg = encode(pl[gate][0], pl[gate][1], pl[gate][2])
check("a gate plane under a down_proj name is rejected (projection picks the geometry)",
      raises(lambda: list(csd.decode_stream(iter([(gate.replace("gate_proj", "down_proj") + csd.SUFFIX_FIXED, fg),
                                                  (gate.replace("gate_proj", "down_proj") + csd.SUFFIX_EXC, wg)]), Cfg))))
check("unpaired tensor at end of stream raises",
      raises(lambda: list(csd.decode_stream(iter(stream[:-1]), Cfg))))
check("duplicate csf tensor raises",
      raises(lambda: list(csd.decode_stream(iter([stream[2], stream[2]]), Cfg))))
b0 = next(iter(pl))
f0, w0 = encode(pl[b0][0], pl[b0][1], pl[b0][2])
check("fixed stream that fits no plane raises",
      raises(lambda: list(csd.decode_stream(iter([(b0 + csd.SUFFIX_FIXED, f0[:-2]), (b0 + csd.SUFFIX_EXC, w0)]), Cfg))))
class NoCfg: pass
check("missing hidden/intermediate size raises",
      raises(lambda: list(csd.decode_stream(iter([(b0 + csd.SUFFIX_FIXED, f0), (b0 + csd.SUFFIX_EXC, w0)]), NoCfg))))
check("stream with no csf tensors is untouched (and silent)",
      list(csd.decode_stream(iter([("a", 1), ("b", 2)]), NoCfg)) == [("a", 1), ("b", 2)])

# ------------------------------------------------------------------ hook

FAKE = {
    "vllm/__init__.py": "",
    "vllm/model_executor/__init__.py": "",
    "vllm/model_executor/model_loader/__init__.py": "",
    "vllm/model_executor/model_loader/ep_weight_filter.py": """
        def should_skip_weight(weight_name, local_expert_ids):
            return False
    """,
    "vllm/model_executor/model_loader/weight_utils.py": """
        from vllm.model_executor.model_loader.ep_weight_filter import should_skip_weight
    """,
    "vllm/model_executor/model_loader/default_loader.py": """
        STREAM = []
        class DefaultModelLoader:
            def get_all_weights(self, model_config, model):
                yield from STREAM
            def load_weights(self, model, model_config):
                return list(self.get_all_weights(model_config, model))
    """,
}


def build(tmp, files=FAKE):
    for rel, src in files.items():
        path = os.path.join(tmp, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w") as f:
            f.write(textwrap.dedent(src))


def reset():
    for k in [k for k in sys.modules if k == "vllm" or k.startswith("vllm.")]:
        del sys.modules[k]
    sys.meta_path[:] = [f for f in sys.meta_path if type(f).__name__ != "_HookFinder"]
    csd._applied.clear(); csd._noted.clear()


def load_draft():
    spec = importlib.util.spec_from_file_location(
        "draft_load_filter", os.path.join(HERE, "..", "draft-load-filter", "draft_load_filter.py"))
    mod = importlib.util.module_from_spec(spec)
    sys.modules["draft_load_filter"] = mod
    spec.loader.exec_module(mod)
    mod._applied.clear(); mod._noted.clear()
    return mod


tmp = tempfile.mkdtemp(suffix="fakevllm")
try:
    build(tmp)
    sys.path.insert(0, tmp)

    # --- installed alone
    reset(); csd._backend = csd.ReferenceBackend
    err = io.StringIO()
    with contextlib.redirect_stderr(err):
        csd.install(); csd.install()
        dl = importlib.import_module("vllm.model_executor.model_loader.default_loader")
    check("install is idempotent (one finder)",
          sum(type(f).__name__ == "_HookFinder" for f in sys.meta_path) == 1)
    check("get_all_weights is wrapped", getattr(dl.DefaultModelLoader.get_all_weights, csd._MARK, False))
    dl.STREAM[:] = stream
    with contextlib.redirect_stderr(err):
        out = list(dl.DefaultModelLoader().get_all_weights(Cfg, object()))
    check("through the hook: same result as decode_stream", [n for n, _ in out] == names)
    check("summary line logged", "decoded 9 compressed expert scale planes" in err.getvalue(), err.getvalue())
    with contextlib.redirect_stderr(err):
        viaload = dl.DefaultModelLoader().load_weights(object(), Cfg)
    check("load_weights (the real consumer) sees decoded tensors only",
          not any(n.endswith((csd.SUFFIX_FIXED, csd.SUFFIX_EXC)) for n, _ in viaload))

    # --- CSF_SCALE_DECODE=0
    reset(); os.environ[csd.ENV] = "0"
    csd.install()
    check("CSF_SCALE_DECODE=0 installs nothing", not any(type(f).__name__ == "_HookFinder" for f in sys.meta_path))
    importlib.import_module("vllm.model_executor.model_loader.default_loader")
    check("...and leaves the method alone",
          not getattr(sys.modules["vllm.model_executor.model_loader.default_loader"].DefaultModelLoader.get_all_weights, csd._MARK, False))
    del os.environ[csd.ENV]

    # --- upstream renamed the target
    reset()
    ren = dict(FAKE)
    ren["vllm/model_executor/model_loader/default_loader.py"] = """
        class DefaultModelLoader:
            def get_every_weight(self, model_config, model):
                yield from []
    """
    shutil.rmtree(tmp); os.makedirs(tmp); build(tmp, ren)
    err = io.StringIO()
    with contextlib.redirect_stderr(err):
        csd.install()
        importlib.import_module("vllm.model_executor.model_loader.default_loader")
    check("renamed target -> 'NOT applied' note, import still works", "NOT applied" in err.getvalue())

    # --- coexistence with draft-load-filter, both install orders
    for order in ("ours-first", "draft-first"):
        reset(); shutil.rmtree(tmp); os.makedirs(tmp); build(tmp)
        csd._backend = csd.ReferenceBackend
        draft = load_draft()
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            if order == "ours-first":
                csd.install(); draft.install()
            else:
                draft.install(); csd.install()
            importlib.import_module("vllm.model_executor.model_loader.weight_utils")
            dl = importlib.import_module("vllm.model_executor.model_loader.default_loader")
        check(f"{order}: both patches applied to default_loader (no recursion)",
              getattr(dl.DefaultModelLoader.get_all_weights, csd._MARK, False)
              and getattr(dl.DefaultModelLoader.load_weights, draft._MARK, False), err.getvalue())
        dl.STREAM[:] = stream
        with contextlib.redirect_stderr(err):
            out = list(dl.DefaultModelLoader().get_all_weights(Cfg, object()))
        check(f"{order}: decode still correct", {n: bytes(t) for n, t in out if n in pl} == {b: pl[b][0] for b in pl})
finally:
    reset()
    sys.path[:] = [p for p in sys.path if p != tmp]
    shutil.rmtree(tmp, ignore_errors=True)

failed = [n for n, ok, _ in checks if not ok]
if failed:
    print(f"\n{len(failed)} FAILED of {len(checks)}")
    sys.exit(1)
print(f"\nALL TESTS PASS ({len(checks)})")
