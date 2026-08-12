# SPDX-License-Identifier: Apache-2.0
"""Load upstream's in-flight native Muse Glimmer support, out of tree.

vLLM 0.27.1 has no Muse Glimmer, so run-muse serves it through the generic
Transformers backend:

    WARNING [utils.py:217] TransformersMultiModalForCausalLM has no vLLM
    implementation, falling back to Transformers implementation.

That fallback is the reason patches/ exists at all -- it drops the embedding
RMSNorm, ignores output_multiplier, reads layer_types from the wrong config
level, and silently fails to capture the aux hidden states DFlash needs.

vllm-project/vllm#51655 adds a native implementation. This loads the files that
PR ADDS directly from vendor/, so the native path can be exercised without
rebuilding the image -- a rebuild being multi-hour here, and the PR still moving.

    ./scripts/muse-native-sync      refresh vendor/ from the PR
    NATIVE=1 ./run-muse             serve with it

vendor/ is byte-identical to upstream and never edited, so a refresh is a copy
rather than a merge. The handful of edits the PR makes to EXISTING core files
cannot be vendored (that would drag in an entire newer tree), so they are
mirrored here and in sitecustomize instead:

    registry.py:530   MuseGlimmerForConditionalGeneration -> MuseGlimmerForCausalLM
                      (run-muse passes this via --model-class-overrides)
    registry.py:630   MuseGlimmerAssistantModel -> qwen3_dflash.DFlashQwen3ForCausalLM
                      (note: upstream drives the drafter with the STOCK class,
                       same as patches/muse-dflash does)
    config.py:104-107 the four muse_glimmer* config classes
                      (register_configs below)

Delete the whole directory once the PR merges into a release ./build pins.
"""

import importlib.abc
import importlib.util
import os
import sys

_HERE = os.path.dirname(os.path.abspath(__file__))
VENDOR = os.path.join(_HERE, "vendor")
STAMP = os.path.join(_HERE, "UPSTREAM")


def upstream_ref() -> str:
    """The vendored commit, so logs say WHICH upstream this ran."""
    try:
        with open(STAMP) as fh:
            fields = dict(
                line.split(": ", 1)
                for line in fh.read().splitlines()
                if ": " in line and not line.startswith("#")
            )
        return f"{fields.get('commit', '?')[:12]} ({fields.get('date', '?')}, pr {fields.get('pr', '?')})"
    except OSError:
        return "unknown -- run ./scripts/muse-native-sync"


def _vendored(fullname: str) -> str | None:
    """Path to a vendored module, or None.

    Deliberately narrow: only `vllm.*` names that mention muse/glimmer. No such
    module exists in 0.27.1, so this can only ADD modules and never shadow a
    core one -- which is what keeps a stale vendor tree from quietly overriding
    vLLM itself.
    """
    if not fullname.startswith("vllm."):
        return None
    low = fullname.lower()
    if "muse" not in low and "glimmer" not in low:
        return None
    path = os.path.join(VENDOR, fullname.replace(".", os.sep) + ".py")
    return path if os.path.isfile(path) else None


class _VendorFinder(importlib.abc.MetaPathFinder):
    """Resolve upstream's new modules to vendor/ under their real dotted names.

    They have to keep their real names because the vendored files import each
    other that way (the model does `from vllm.transformers_utils.processors
    .muse_glimmer import ...`). Resolving lazily through the import system also
    means load ORDER stops mattering, and a file added by a future revision of
    the PR is picked up with no change here.
    """

    def find_spec(self, fullname, path=None, target=None):
        vendored = _vendored(fullname)
        if vendored is None:
            return None
        return importlib.util.spec_from_file_location(fullname, vendored)


def install() -> None:
    if not any(isinstance(f, _VendorFinder) for f in sys.meta_path):
        sys.meta_path.insert(0, _VendorFinder())


def register_configs() -> list[str]:
    """Mirror upstream's _CONFIG_REGISTRY entries (transformers_utils/config.py).

    LazyConfigDict.__getitem__ returns a value unchanged when it is already a
    type (config.py:63-69), so the class objects go straight in -- no need to
    graft them onto the vllm.transformers_utils.configs package first.
    """
    install()
    from vllm.transformers_utils.config import _CONFIG_REGISTRY
    from vllm.transformers_utils.configs.muse_glimmer import (
        MuseGlimmerAssistantConfig,
        MuseGlimmerConfig,
        MuseGlimmerTextConfig,
        MuseGlimmerVisionConfig,
    )

    mapping = {
        "muse_glimmer": MuseGlimmerConfig,
        "muse_glimmer_text": MuseGlimmerTextConfig,
        "muse_glimmer_vision": MuseGlimmerVisionConfig,
        "muse_glimmer_assistant": MuseGlimmerAssistantConfig,
    }
    for model_type, cls in mapping.items():
        _CONFIG_REGISTRY[model_type] = cls
    return sorted(mapping)


def _compat_decoder_is_its_own_model(native) -> None:
    """Give the decoder a `.model` that is itself, for 0.27.1's spec-decode path.

    0.27.1 expects one more wrapper layer than this model has. Both the EAGLE3
    aux-layer lookup and the DFlash embedding/lm_head sharing do

        target_inner = target_language_model.model

    but MuseGlimmerForCausalLM marks its inner MuseGlimmerModel AS the language
    model, so get_language_model() already returns the decoder and there is no
    further `.model`. Newer main -- which this PR is branched off -- writes

        getattr(target_language_model, "model", target_language_model)

    in both places (#51655 touches interfaces.py and
    v1/worker/gpu/spec_decode/dflash/utils.py for exactly this). Rewriting lines
    inside vLLM functions is not something a shim can do cleanly, so express the
    same unwrap from our side instead: the decoder answers `.model` with itself,
    which is what both call sites are reaching for. It already carries the
    `embed_tokens` they then read.

    Inert on a vLLM that has the fallback -- getattr finds this and returns the
    same object. Drop it when ./build pins a vLLM new enough.
    """
    cls = getattr(native, "MuseGlimmerModel", None)
    if cls is None or "model" in vars(cls):
        return
    cls.model = property(lambda self: self)


# PEP 562: resolve the model class on attribute access, so
# --model-class-overrides can name `muse_native:MuseGlimmerForCausalLM` without
# importing 66 KB of model code just to import this module.
def __getattr__(name: str):
    if name in ("MuseGlimmerForCausalLM", "MuseGlimmerModel"):
        install()
        import vllm.model_executor.models.muse_glimmer as native

        _compat_decoder_is_its_own_model(native)
        return getattr(native, name)
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")


install()
