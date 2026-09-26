"""Host-side test for draft_load_filter (stdlib only, no vLLM).

Run:  python3 container/patches/draft-load-filter/test_draft_load_filter.py

Builds a tiny fake `vllm` package on disk with the same shape as the real
seams (ep_weight_filter.should_skip_weight imported BY NAME into
weight_utils, an iterator that consults it before "reading", and
DefaultModelLoader.load_weights consuming that iterator), installs the real
import hook, imports the fake package through it, and checks: what gets
"read" for a draft vs. a target model, EP filtering unchanged, reset on
exceptions, and every graceful-degradation path.
"""

import contextlib
import importlib.util
import io
import os
import shutil
import sys
import tempfile
import textwrap

HERE = os.path.dirname(os.path.abspath(__file__))

FAKE = {
    "vllm/__init__.py": "",
    "vllm/model_executor/__init__.py": "",
    "vllm/model_executor/model_loader/__init__.py": "",
    "vllm/model_executor/model_loader/ep_weight_filter.py": """
        import re
        def should_skip_weight(weight_name, local_expert_ids):
            if local_expert_ids is None:
                return False
            m = re.search(r"\\.experts\\.(\\d+)\\.", weight_name)
            return m is not None and int(m.group(1)) not in local_expert_ids
    """,
    "vllm/model_executor/model_loader/weight_utils.py": """
        from vllm.model_executor.model_loader.ep_weight_filter import (
            should_skip_weight,
        )
        READS = []
        def safetensors_weights_iterator(names, local_expert_ids=None):
            for name in names:
                if should_skip_weight(name, local_expert_ids):
                    continue
                READS.append(name)  # stands in for f.get_tensor(name)
                yield name, "tensor:" + name
        def unfiltered_iterator(names, local_expert_ids=None):
            for name in names:
                READS.append(name)
                yield name, "tensor:" + name
    """,
    "vllm/model_executor/model_loader/default_loader.py": """
        from vllm.model_executor.model_loader.weight_utils import (
            safetensors_weights_iterator,
        )
        class DefaultModelLoader:
            def __init__(self, names, local_expert_ids=None, iterator=None):
                self.names = names
                self.local_expert_ids = local_expert_ids
                self.iterator = iterator or safetensors_weights_iterator
            def load_weights(self, model, model_config):
                return model.load_weights(
                    self.iterator(self.names, self.local_expert_ids)
                )
    """,
    "vllm/models/__init__.py": "",
    "vllm/models/qwen4_exp/__init__.py": "",
    "vllm/models/qwen4_exp/nvidia/__init__.py": "",
    "vllm/models/qwen4_exp/nvidia/mtp.py": """
        def _remap_mtp_weight_name(name):
            if name.startswith("model.language_model."):
                name = name.removeprefix("model.language_model.")
            if name.startswith("mtp."):
                return name.replace("mtp.", "model.", 1)
            if name.startswith(("embed_tokens.", "lm_head.")):
                return name
            return None
        class Qwen4ExpMTP:
            def load_weights(self, weights):
                return {n for n, _ in weights if _remap_mtp_weight_name(n) is not None}
        class Boom(Qwen4ExpMTP):
            pass
    """,
    "vllm/models/renamed.py": """
        class Qwen4ExpMTP:  # a draft class whose module lost the remap fn
            def load_weights(self, weights):
                return {n for n, _ in weights}
    """,
    "vllm/models/target.py": """
        class Qwen4ExpForConditionalGeneration:
            def load_weights(self, weights):
                return {n for n, _ in weights}
    """,
}

NAMES = [
    "model.language_model.embed_tokens.weight",
    "model.language_model.layers.0.mlp.experts.0.w1.weight",
    "model.language_model.layers.0.mlp.experts.1.w1.weight",
    "model.language_model.layers.1.self_attn.q_proj.weight",
    "model.language_model.mtp.layers.0.mlp.experts.0.w1.weight",
    "model.language_model.mtp.layers.0.mlp.experts.1.w1.weight",
    "model.language_model.mtp.fc_hidden.weight",
    "lm_head.weight",
    "model.language_model.ple.ple_embedding.ngram_embedding.shard_0.weight",
]
DRAFT_KEEP = [
    n for n in NAMES
    if "mtp." in n or n.endswith(("embed_tokens.weight",)) or n == "lm_head.weight"
]


def load_module(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, "draft_load_filter.py"))
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


tmpd = tempfile.mkdtemp(prefix="dlf-test-")
try:
    for rel, src in FAKE.items():
        p = os.path.join(tmpd, rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w") as f:
            f.write(textwrap.dedent(src))
    sys.path.insert(0, tmpd)

    # --- DRAFT_LOAD_FILTER=0: install() is a no-op
    os.environ["DRAFT_LOAD_FILTER"] = "0"
    off = load_module("dlf_off")
    before = list(sys.meta_path)
    off.install()
    assert sys.meta_path == before, "install() must not hook when disabled"
    os.environ.pop("DRAFT_LOAD_FILTER")

    # --- real install + import through the finder
    dlf = load_module("draft_load_filter")
    dlf.install()
    dlf.install()  # idempotent
    assert sum(isinstance(f, dlf._HookFinder) for f in sys.meta_path) == 1

    import vllm.model_executor.model_loader.default_loader as dl  # noqa: E402
    import vllm.model_executor.model_loader.ep_weight_filter as ep  # noqa: E402
    import vllm.model_executor.model_loader.weight_utils as wu  # noqa: E402
    from vllm.models.qwen4_exp.nvidia.mtp import Boom, Qwen4ExpMTP  # noqa: E402
    from vllm.models.renamed import Qwen4ExpMTP as RenamedMTP  # noqa: E402
    from vllm.models.target import Qwen4ExpForConditionalGeneration as Target  # noqa: E402

    assert getattr(wu.should_skip_weight, dlf._MARK, False), "should_skip_weight not hooked"
    assert getattr(dl.DefaultModelLoader.load_weights, dlf._MARK, False), "load_weights not hooked"
    assert dlf._applied == {dlf.WEIGHT_UTILS, dlf.DEFAULT_LOADER}, dlf._applied

    # --- inactive: the wrapper is exactly the original (EP semantics unchanged)
    for n in NAMES:
        for ids in (None, {0}, {1}, set()):
            assert wu.should_skip_weight(n, ids) == ep.should_skip_weight(n, ids), (n, ids)

    # --- draft model: only the tensors its load_weights keeps are read
    wu.READS.clear()
    loaded, err = captured(dl.DefaultModelLoader(NAMES).load_weights, Qwen4ExpMTP(), None)
    assert wu.READS == DRAFT_KEEP, wu.READS
    assert loaded == set(DRAFT_KEEP), loaded
    skipped = len(NAMES) - len(DRAFT_KEEP)
    assert f"read {len(DRAFT_KEEP)} checkpoint tensors, skipped {skipped}" in err, err
    assert dlf._active is None, "filter must be cleared after load_weights"

    # --- target model: no filter, everything read
    wu.READS.clear()
    loaded, err = captured(dl.DefaultModelLoader(NAMES).load_weights, Target(), None)
    assert wu.READS == NAMES and loaded == set(NAMES), wu.READS
    assert err == "", err

    # --- draft + EP: both filters compose (MTP keep AND local expert)
    wu.READS.clear()
    captured(dl.DefaultModelLoader(NAMES, local_expert_ids={1}).load_weights, Qwen4ExpMTP(), None)
    assert wu.READS == [n for n in DRAFT_KEEP if ".experts.0." not in n], wu.READS

    # --- exception inside the load: filter reset, next target load reads all
    class Exploding(Qwen4ExpMTP):
        def load_weights(self, weights):
            next(iter(weights))
            raise RuntimeError("boom")
    Exploding.__name__ = "Qwen4ExpMTP"
    Exploding.__module__ = Qwen4ExpMTP.__module__
    try:
        captured(dl.DefaultModelLoader(NAMES).load_weights, Exploding(), None)
        raise AssertionError("expected RuntimeError")
    except RuntimeError:
        pass
    assert dlf._active is None, "filter leaked past an exception"
    wu.READS.clear()
    captured(dl.DefaultModelLoader(NAMES).load_weights, Target(), None)
    assert wu.READS == NAMES

    # --- a class not in FILTERS (subclass with another name): untouched
    wu.READS.clear()
    captured(dl.DefaultModelLoader(NAMES).load_weights, Boom(), None)
    assert wu.READS == NAMES, "only FILTERS class names get a filter"

    # --- the remap fn vanished upstream: read everything, say so once
    wu.READS.clear()
    _, err = captured(dl.DefaultModelLoader(NAMES).load_weights, RenamedMTP(), None)
    assert wu.READS == NAMES and "NOT applied for Qwen4ExpMTP" in err, err
    _, err2 = captured(dl.DefaultModelLoader(NAMES).load_weights, RenamedMTP(), None)
    assert err2 == "", "the NOT-applied note must print once"

    # --- keep() raising: that tensor is read (never silently dropped)
    orig_filters = dict(dlf.FILTERS)
    import vllm.models.qwen4_exp.nvidia.mtp as mtp_mod  # noqa: E402
    def flaky(name):  # the FILTER's copy of the remap raises; the model's does not
        if "fc_hidden" in name:
            raise ValueError("flaky")
        return mtp_mod._remap_mtp_weight_name(name)
    mtp_mod.flaky_remap = flaky
    dlf.FILTERS["Qwen4ExpMTP"] = "flaky_remap"
    wu.READS.clear()
    captured(dl.DefaultModelLoader(NAMES).load_weights, Qwen4ExpMTP(), None)
    assert wu.READS == DRAFT_KEEP, wu.READS  # fc_hidden still read via fallback
    dlf.FILTERS.clear()
    dlf.FILTERS.update(orig_filters)

    # --- an iterator that never consults should_skip_weight: warn, read all
    wu.READS.clear()
    _, err = captured(
        dl.DefaultModelLoader(NAMES, iterator=wu.unfiltered_iterator).load_weights,
        Qwen4ExpMTP(), None,
    )
    assert wu.READS == NAMES and "never consulted should_skip_weight" in err, err

    # --- should_skip_weight hook missing: draft load falls back, says so
    dlf._applied.discard(dlf.WEIGHT_UTILS)
    wu.READS.clear()
    _, err = captured(dl.DefaultModelLoader(NAMES).load_weights, Qwen4ExpMTP(), None)
    assert wu.READS == NAMES and "should_skip_weight was not hooked" in err, err
    dlf._applied.add(dlf.WEIGHT_UTILS)

    # --- _apply is idempotent (a second pass must not double-wrap)
    w1 = wu.should_skip_weight
    dlf._apply(wu)
    assert wu.should_skip_weight is w1
finally:
    shutil.rmtree(tmpd, ignore_errors=True)

print("ALL TESTS PASS")
