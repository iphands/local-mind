"""Runtime shims for Muse Glimmer under vLLM's Transformers backend.

Activated by putting this directory on PYTHONPATH; CPython imports
`sitecustomize` automatically at startup, so these land in the API server, the
engine core, every spawned worker, and vLLM's model-inspection subprocess. There
can only be one `sitecustomize` module, so this file hosts all three shims. Each
is inert unless its trigger module is imported, and each can be turned off on
its own -- see the env vars below.

SHIM 1 -- restore the embedding RMSNorm (MUSE_EMBED_NORM=0 to disable)
======================================================================
vLLM has no native MuseGlimmerForConditionalGeneration (checked on 0.25.2 and
again on 0.27.1, where this patch still fires), so it falls back to the generic
Transformers backend. That backend unconditionally swaps the model's input
embedding for a plain VocabParallelEmbedding:

    vllm/model_executor/models/transformers/base.py:170-189
        input_embeddings = self.model.get_input_embeddings()
        embed_scale = getattr(input_embeddings, "embed_scale", None)
        ...
        new_input_embeddings = VocabParallelEmbedding(**embedding_kwargs)
        self.model.set_input_embeddings(new_input_embeddings)

It understands a scalar `embed_scale`, but Muse Glimmer does not use one. It uses
a weight-less RMSNorm module held *inside* the embedding --
transformers/models/muse_glimmer/modeling_muse_glimmer.py:436

    class MuseGlimmerTextNormedEmbedding(nn.Embedding):
        def __init__(...):
            super().__init__(num_embeddings, embedding_dim, padding_idx)
            self.embed_norm = MuseGlimmerRMSNorm(eps=norm_eps, with_scale=False)
        def forward(self, input_ids):
            return self.embed_norm(super().forward(input_ids))

Because the replacement keeps only the weight, `embed_norm` is silently dropped and
decoder layer 0 receives unnormalized embeddings. Measured against stock HF on the
same 57-token prompt, hidden states diverge immediately (cosine 0.456 at layer 0,
~0.3 by layer 51) and the model emits pure garbage.

The fix re-attaches the original `embed_norm` to whatever module vLLM installs. It
swaps `__class__` rather than wrapping the module, so the parameter tree is
untouched (`embed_tokens.weight` keeps its name) and vLLM's weight loader and TP
sharding are unaffected. `embed_norm` has no parameters of its own, so nothing new
needs loading.

SHIM 2 -- make aux hidden-state capture actually work (MUSE_DFLASH=0 to disable)
===============================================================================
THIS IS THE ONE THAT MAKES SPEC=1 BOOT AT ALL. Without it the server dies during
the profile run with `assert isinstance(model_output, tuple)`
(vllm/v1/worker/gpu/model_runner.py:1425-1427).

DFlash feeds on the target's hidden states at layers {1,13,25,37,49}. vLLM's
Transformers backend advertises the EAGLE3 interface for exactly this
(transformers/base.py:681-706) but the mechanism silently captures NOTHING on
this model, for four compounding reasons:

  1. transformers 5.15 keys output capture off a MODULE-LEVEL registry,
     _CAN_RECORD_REGISTRY[str(cls)], populated once in PreTrainedModel.__init__
     (modeling_utils.py:1375).
  2. MuseGlimmerModel._can_record_outputs is None (modeling_muse_glimmer.py:433,
     "set on children directly as they are different for text and vision"), so
     that registry entry is permanently None. vLLM's
         if self.model._can_record_outputs is None:
             self.model._can_record_outputs = {}
     creates an INSTANCE attribute the registry never sees.
  3. recursively_install_hooks stops dispatching a parent's capture tasks at
     every PreTrainedModel boundary (output_capturing.py:134-141), so recorders
     aimed at `language_model.layers.N` are dropped before reaching the layers.
  4. MuseGlimmerModel.forward has no @capture_outputs and re-packs only four
     named fields (modeling_muse_glimmer.py:1089-1142), discarding anything the
     inner text model did manage to inject.

So: proxy the registry into the text model's entry (that is where the hooks come
from), give the composite forward a @capture_outputs wrapper, and bubble the aux
entries up from the inner collector. Patched on the CLASSES before any instance
exists -- that ordering is what gets the dict into the registry at __init__.

SHIM 3 -- reshape the DFlash drafter config (MUSE_DFLASH=0 to disable)
=====================================================================
Muse-Glimmer-30B-assistant's config.json does not use the key layout vLLM's
DFlash code reads. It puts `mask_token_id` / `target_layer_ids` / `block_size` at
the top level where vLLM wants a `dflash_config` dict (compare
models/vllm/Qwen3.6-27B-DFlash/config.json), and it carries no `vocab_size` at
all -- the checkpoint ships no embedding table, so there was nothing to size --
which makes DFlashQwen3Model.__init__ raise on `self.config.vocab_size`.

`--hf-overrides` cannot fix this: it is target-only by design. Dict overrides are
deliberately not forwarded to the draft config
(SpeculativeConfig.compose_draft_hf_overrides, vllm/config/speculative.py:639-663).

The supported seam is `SpeculativeConfig.hf_config_override`
(vllm/config/speculative.py:331) -- the staticmethod vLLM installs as the *draft*
model's hf_overrides callable, and the same hook it uses in-tree to reshape every
MTP checkpoint. This wraps it and adds a `muse_glimmer_assistant` branch.

The rest of the drafter is genuinely Qwen3-shaped, so the model class is a thin
subclass of vLLM's own: see patches/muse-dflash/muse_dflash.py.

SHIM 4 -- fallback AutoConfig for muse_glimmer_assistant
========================================================
Insurance only. If the installed transformers does not know the
`muse_glimmer_assistant` model_type, AutoConfig.from_pretrained on the drafter
fails *before* shim 3 ever runs, and the drafter has no `auto_map` to fall back
on. A bare PretrainedConfig subclass is enough, since PretrainedConfig assigns
unknown kwargs as attributes and the drafter config is all plain scalars.
No-ops when transformers already has the type (5.15 does).

SHIM 5 -- image_token_index alias
=================================
Insurance only. vLLM's V1 proposer does, for any multimodal target it does not
recognise by class name (llm_base_proposer.py:1397-1399):

    self.model.config.image_token_index = target_model.config.image_token_index

MuseGlimmerConfig only has `image_token_id` (configuration_muse_glimmer.py:194)
and is @strict, so that is an AttributeError. Inert on the V2 runner -- which is
what this model gets by default, and what run-muse pins -- but one config change
away from mattering.

SHIM 6 -- reasoning-parser plugin in worker processes (MUSE_ATEM_PLUGIN)
========================================================================
NOT insurance: without this, SPEC=1 dies before the model loads. vLLM imports
--reasoning-parser-plugin only in the API server, but DFlash rebuilds a draft
VllmConfig inside the worker, which re-validates and looks the parser up there.
See the shim for the full trace.

Delete all six once vLLM ships a native Muse Glimmer implementation and DFlash
drafter.
"""

import functools
import importlib.abc
import importlib.util
import os
import sys

# --- what the target model contributes, for the drafter's config -------------
# The drafter shares the target's embedding table and lm_head, so it inherits
# the target's vocabulary. These are read from Muse-Glimmer-30B/config.json
# text_config; muse_dflash.py asserts the vocab against the live target model at
# load, so a stale value here fails loudly rather than drafting garbage.
_TARGET_VOCAB_SIZE = 202048
_TARGET_NUM_LAYERS = 52
# text_config.output_multiplier. run-muse passes its LOGIT_SCALE knob through as
# MUSE_DRAFT_LOGIT_SCALE so the A/B stays coherent across target and drafter --
# rejection sampling compares the two models' logits, so they must agree.
_TARGET_LOGIT_SCALE = 0.19611613513818404

_DRAFT_MODEL_TYPE = "muse_glimmer_assistant"
_DRAFT_ARCH = "DFlashMuseGlimmerAssistantModel"

MODELING = "transformers.models.muse_glimmer.modeling_muse_glimmer"
SPECULATIVE = "vllm.config.speculative"
REASONING = "vllm.reasoning.abs_reasoning_parsers"
TRANSFORMERS_BACKEND = "vllm.model_executor.models.transformers.base"
TU_CONFIG = "vllm.transformers_utils.config"
INTERFACES = "vllm.model_executor.models.interfaces"
W8A16FP8 = (
    "vllm.model_executor.layers.quantization.compressed_tensors"
    ".schemes.compressed_tensors_w8a16_fp8"
)


def _log(msg):
    print(msg, file=sys.stderr)


# ---------------------------------------------------------------------------
# SHIM 1 -- embedding RMSNorm
# ---------------------------------------------------------------------------
def _patch_embed_norm(mod):
    if os.environ.get("MUSE_EMBED_NORM", "1") != "1":
        return
    text_model_cls = getattr(mod, "MuseGlimmerTextModel", None)
    if text_model_cls is None or getattr(text_model_cls, "_muse_embed_norm_patched", False):
        return

    def set_input_embeddings(self, value):
        old = getattr(self, "embed_tokens", None)
        norm = getattr(old, "embed_norm", None)
        # Only act when the incoming module is a replacement that lost the norm.
        if norm is not None and getattr(value, "embed_norm", None) is None:
            cls = type(value)
            base_forward = cls.forward

            def forward(self, *args, **kwargs):
                return self.embed_norm(base_forward(self, *args, **kwargs))

            value.__class__ = type(f"EmbedNormed{cls.__name__}", (cls,), {"forward": forward})
            value.embed_norm = norm
            _log(f"[muse-embed-norm] re-attached embed_norm to {cls.__name__}")
        self.embed_tokens = value

    text_model_cls.set_input_embeddings = set_input_embeddings
    text_model_cls._muse_embed_norm_patched = True


# ---------------------------------------------------------------------------
# SHIM 2 -- aux hidden-state capture (see the header; this is the crash fix)
# ---------------------------------------------------------------------------
def _patch_aux_capture(mod):
    if os.environ.get("MUSE_DFLASH", "1") != "1":
        return
    composite_cls = getattr(mod, "MuseGlimmerModel", None)
    text_model_cls = getattr(mod, "MuseGlimmerTextModel", None)
    if composite_cls is None or text_model_cls is None:
        return
    if getattr(composite_cls, "_muse_aux_capture_patched", False):
        return

    class _AuxRegistryProxy(dict):
        """Registry entry for MuseGlimmerModel that mirrors into the text model.

        vLLM registers its OutputRecorders on the composite model, but hooks for
        `language_model.layers.N` are only ever installed from the TEXT model's
        registry entry (recursively_install_hooks re-dispatches at every
        PreTrainedModel boundary, output_capturing.py:134-141). Mirror writes
        there.

        Mirroring into the text model is also what makes this survive
        torch.compile. vLLM compiles the DECODER class, so
        MuseGlimmerTextModel.forward -- and the @capture_outputs already on it --
        run INSIDE the traced region. capture_outputs' collector is a
        CompileableContextVar: its `set` notices is_torchdynamo_compiling() and
        switches to a plain global, so the layer hooks' `get` is traceable. Claim
        the keys anywhere else and the collector gets established OUTSIDE the
        graph, leaving the hooks to call ContextVar.get() inside it:

            torch._dynamo.exc.Unsupported: Dynamo does not know how to trace
            method `get` of class `ContextVar`

        Deliberately a proxy rather than an alias of the text model's dict: an
        alias would leak `hidden_states`/`attentions` into the composite's entry.
        """

        def __setitem__(self, key, value):
            text_model_cls._can_record_outputs[key] = value
            dict.__setitem__(self, key, value)

    # Must be a class attribute set BEFORE any instance exists: __init__ stores
    # this exact object in _CAN_RECORD_REGISTRY, so later mutation stays visible.
    composite_cls._can_record_outputs = _AuxRegistryProxy()
    composite_cls._muse_aux_capture_patched = True
    _log("[muse-dflash] aux capture installed on MuseGlimmerModel")


# ---------------------------------------------------------------------------
# SHIM 5 -- image_token_index alias (V1 insurance)
# ---------------------------------------------------------------------------
def _patch_config_alias():
    if os.environ.get("MUSE_DFLASH", "1") != "1":
        return
    try:
        from transformers.models.muse_glimmer.configuration_muse_glimmer import (
            MuseGlimmerConfig,
        )
    except Exception as exc:
        _log(f"[muse-dflash] could not alias image_token_index: {exc!r}")
        return
    existing = dict(getattr(MuseGlimmerConfig, "attribute_map", None) or {})
    if "image_token_index" in existing:
        return
    existing["image_token_index"] = "image_token_id"
    MuseGlimmerConfig.attribute_map = existing


_AUX_PREFIX = "aux_hidden_state_"


def _install_aux_passthrough(composite):
    """Carry the captured aux states out through MuseGlimmerModel.

    The text model collects them correctly (see _AuxRegistryProxy) and injects
    them into its own BaseModelOutputWithPast -- but MuseGlimmerModel.forward
    re-packs only four named fields (modeling_muse_glimmer.py:1137-1142) and
    drops the rest, so vLLM's Base.forward never sees them and returns a bare
    tensor instead of a tuple.

    Both wrappers go on top of ALREADY-DECORATED entry points, because this runs
    from set_aux_hidden_state_layers -- after Base.__init__ has done its
    torch.compile decoration. That ordering is the point: these stash and
    re-append in plain eager Python, outside the traced region, so nothing here
    has to be dynamo-traceable.

    On the text side that means wrapping __call__, NOT forward.
    vllm/compilation/decorators.py:719 does `cls.__call__ = __call__` and calls
    `self.forward(...)` from inside it, so the compiled region starts at
    __call__; a wrapper on forward would sit INSIDE the graph, where stashing on
    self is an untraceable side effect. The composite is not compiled (vLLM
    decorates only the decoder class), so wrapping its forward is fine.
    """
    text = getattr(composite, "language_model", None) or getattr(
        composite, "text_model", None
    )
    if text is None:
        _log("[muse-dflash] no language_model on the composite; aux pass-through skipped")
        return
    composite_cls, text_cls = type(composite), type(text)
    if getattr(composite_cls, "_muse_passthrough_installed", False):
        return

    text_call = text_cls.__call__

    @functools.wraps(text_call)
    def text_wrapper(self, *args, **kwargs):
        out = text_call(self, *args, **kwargs)
        if hasattr(out, "keys"):
            self._muse_aux = {k: out[k] for k in out.keys() if k.startswith(_AUX_PREFIX)}
        return out

    composite_forward = composite_cls.forward

    @functools.wraps(composite_forward)
    def composite_wrapper(self, *args, **kwargs):
        result = composite_forward(self, *args, **kwargs)
        inner = getattr(self, "language_model", None) or getattr(self, "text_model", None)
        aux = getattr(inner, "_muse_aux", None)
        if not aux:
            return result
        # Ascending aux id, matching the order vLLM registered them and the order
        # it concatenates them in (gpu_model_runner.py's torch.cat over the list).
        ordered = [aux[k] for k in sorted(aux, key=lambda k: int(k[len(_AUX_PREFIX) :]))]
        if isinstance(result, tuple):
            return (*result, *ordered)
        for key, value in aux.items():
            result[key] = value
        return result

    text_cls.__call__ = text_wrapper
    composite_cls.forward = composite_wrapper
    composite_cls._muse_passthrough_installed = True
    _log(f"[muse-dflash] aux pass-through installed ({text_cls.__name__} -> {composite_cls.__name__})")


def _patch_transformers_backend(mod):
    """Install the pass-through once vLLM has finished decorating the classes."""
    if os.environ.get("MUSE_DFLASH", "1") != "1":
        return
    base_cls = getattr(mod, "Base", None)
    if base_cls is None or getattr(base_cls, "_muse_aux_passthrough", False):
        return
    original = base_cls.set_aux_hidden_state_layers

    def set_aux_hidden_state_layers(self, layers):
        original(self, layers)
        _install_aux_passthrough(self.model)

    base_cls.set_aux_hidden_state_layers = set_aux_hidden_state_layers
    base_cls._muse_aux_passthrough = True


def _patch_modeling(mod):
    """Everything that hangs off the muse_glimmer modeling module."""
    _patch_embed_norm(mod)
    _patch_aux_capture(mod)
    _patch_config_alias()


# ---------------------------------------------------------------------------
# SHIM 3 -- DFlash drafter config
# ---------------------------------------------------------------------------
def _normalize_draft_config(cfg):
    """Rewrite Muse-Glimmer-30B-assistant's config into vLLM's DFlash layout."""
    target_layer_ids = list(getattr(cfg, "target_layer_ids", None) or [])
    mask_token_id = getattr(cfg, "mask_token_id", None)
    if not target_layer_ids or mask_token_id is None:
        raise ValueError(
            "Muse Glimmer DFlash drafter config is missing target_layer_ids or "
            f"mask_token_id (got {target_layer_ids!r} / {mask_token_id!r}). This "
            "does not look like Muse-Glimmer-30B-assistant."
        )

    # The drafter has no embedding table of its own, so no vocab_size either.
    # It shares the target's, by construction.
    cfg.vocab_size = _TARGET_VOCAB_SIZE
    cfg.logit_scale = float(
        os.environ.get("MUSE_DRAFT_LOGIT_SCALE", _TARGET_LOGIT_SCALE)
    )
    cfg.num_target_layers = _TARGET_NUM_LAYERS

    # vLLM reads these from a dflash_config dict:
    #   mask_token_id     -> qwen3_dflash.py:391 and gpu/spec_decode/utils.py:58-76
    #   target_layer_ids  -> eagle3_utils.py:47-55 (+1, to aux-layer ids)
    #   causal            -> qwen3_dflash.py:60-65, overrides per-layer causality
    #
    # causal=False, because the reference implementation is non-causal. In
    # transformers/models/muse_glimmer_assistant/modeling_muse_glimmer_assistant.py
    # the attention sets `self.is_causal = False` and the model builds
    # create_bidirectional_mask / create_bidirectional_sliding_window_mask, whose
    # underlying bidirectional_mask_function masks nothing at all. Inside the
    # 16-token block the mask tokens attend to each other AND to later mask
    # tokens; the window is a bidirectional |q_idx - kv_idx| <= 2048.
    # MUSE_DRAFT_CAUSAL=1 flips it back; FLASH_ATTN supports either.
    cfg.dflash_config = {
        "mask_token_id": mask_token_id,
        "target_layer_ids": target_layer_ids,
        "num_target_layers": _TARGET_NUM_LAYERS,
        "causal": os.environ.get("MUSE_DRAFT_CAUSAL", "0") == "1",
    }

    # Pre-prefixed, so EAGLEConfig leaves it alone rather than deriving a name
    # (vllm/transformers_utils/configs/eagle.py:63-72). This is the name
    # --model-class-overrides registers in run-muse.
    cfg.architectures = [_DRAFT_ARCH]

    # rope_theta needs no help: the drafter ships rope_parameters.rope_theta
    # (500000.0), which is what qwen3_dflash.py:279-307 reads.
    _log(
        f"[muse-dflash] draft config: vocab={cfg.vocab_size} "
        f"logit_scale={cfg.logit_scale} target_layers={target_layer_ids} "
        f"mask_token_id={mask_token_id} causal={cfg.dflash_config['causal']}"
    )


def muse_draft_hf_config_override(hf_config):
    """Stand-in for SpeculativeConfig.hf_config_override.

    MUST stay a module-level function: this callable is stored on the draft
    ModelConfig and pickled to spawned engine-core processes, so a closure would
    fail with "Can't get local object" -- exactly the trap documented at
    vllm/config/speculative.py:648-652. `sitecustomize` is importable in every
    process by construction, so pickling by qualified name resolves.
    """
    from vllm.config.speculative import SpeculativeConfig

    hf_config = SpeculativeConfig._muse_dflash_original(hf_config)
    if getattr(hf_config, "model_type", None) == _DRAFT_MODEL_TYPE:
        _normalize_draft_config(hf_config)
    return hf_config


def _patch_speculative(mod):
    if os.environ.get("MUSE_DFLASH", "1") != "1":
        return
    cls = getattr(mod, "SpeculativeConfig", None)
    if cls is None or getattr(cls, "_muse_dflash_original", None) is not None:
        return
    cls._muse_dflash_original = staticmethod(cls.hf_config_override)
    cls.hf_config_override = staticmethod(muse_draft_hf_config_override)
    _register_fallback_auto_config()


# ---------------------------------------------------------------------------
# SHIM 4 -- fallback AutoConfig registration
# ---------------------------------------------------------------------------
def _register_fallback_auto_config():
    """Run from shim 3: `vllm.config.speculative` is imported long after
    transformers is fully loaded (no circular-import hazard) but well before the
    draft ModelConfig parses config.json, which is the deadline that matters."""
    from transformers import AutoConfig
    from transformers.configuration_utils import PretrainedConfig
    from transformers.models.auto.configuration_auto import CONFIG_MAPPING

    try:
        if _DRAFT_MODEL_TYPE in CONFIG_MAPPING:
            return  # transformers already knows it -- nothing to do
    except Exception:
        pass

    class MuseGlimmerAssistantConfig(PretrainedConfig):
        model_type = _DRAFT_MODEL_TYPE

    try:
        AutoConfig.register(_DRAFT_MODEL_TYPE, MuseGlimmerAssistantConfig)
    except Exception as exc:  # already registered, or the API moved
        _log(f"[muse-dflash] could not register fallback AutoConfig: {exc!r}")
        return
    _log(f"[muse-dflash] registered fallback AutoConfig for {_DRAFT_MODEL_TYPE}")


# ---------------------------------------------------------------------------
# SHIM 6 -- make --reasoning-parser-plugin survive into worker processes
# ---------------------------------------------------------------------------
def _patch_parser_lookup(mod):
    """Re-import the ATEM plugin on a parser miss, wherever we are.

    vLLM loads --reasoning-parser-plugin in ONE place: the API server process
    (entrypoints/openai/api_server.py:627-631). That is normally enough, because
    VllmConfig is validated there and then pickled to the workers, and
    __post_init__ does not re-run on unpickle.

    DFlash breaks that assumption. Building the DRAFT config happens inside the
    worker (v1/worker/gpu/spec_decode/dflash/utils.py:23 -> config/utils.py:127
    -> cls(**dataclass_dict)), which re-runs VllmConfig.__post_init__ ->
    reasoning_config.initialize_token_ids (config/vllm.py:1592) ->
    ReasoningParserManager.get_reasoning_parser("atem") -- in a process that
    never imported the plugin:

        KeyError: Reasoning parser 'atem' not found. Available parsers: ...

    and the engine core dies before the model is even loaded. So: retry a miss
    once, after importing the plugin named by MUSE_ATEM_PLUGIN. Only the
    reasoning parser is reachable from __post_init__; the tool parser is looked
    up in the API server, which already has it.
    """
    plugin = os.environ.get("MUSE_ATEM_PLUGIN", "")
    if not plugin:
        return
    manager = getattr(mod, "ReasoningParserManager", None)
    if manager is None or getattr(manager, "_muse_lazy_plugin", False):
        return
    original = manager.__dict__["get_reasoning_parser"].__func__

    def get_reasoning_parser(cls, name):
        try:
            return original(cls, name)
        except KeyError:
            if not os.path.isfile(plugin):
                raise
            _log(f"[muse-dflash] reasoning parser {name!r} missing here; loading {plugin}")
            cls.import_reasoning_parser(plugin)
            return original(cls, name)

    manager.get_reasoning_parser = classmethod(get_reasoning_parser)
    manager._muse_lazy_plugin = True


# ---------------------------------------------------------------------------
# import hook
# ---------------------------------------------------------------------------
# ---------------------------------------------------------------------------
# SHIM 7 -- upstream's config classes, when NATIVE=1
# ---------------------------------------------------------------------------
def _patch_native_configs(mod):
    """Mirror the _CONFIG_REGISTRY edits from vllm-project/vllm#51655.

    Everything else the PR adds is vendored and resolved by
    patches/muse-native/muse_native.py, but this one is an edit to an EXISTING
    core file (transformers_utils/config.py:104-107), so it has to be replayed
    rather than copied. Runs right after that module finishes importing, which
    is well before any model config is parsed.
    """
    if os.environ.get("MUSE_NATIVE", "0") != "1":
        return
    if getattr(mod, "_muse_native_configs", False):
        return
    try:
        import muse_native
    except Exception as exc:
        _log(f"[muse-native] NOT active: cannot import muse_native ({exc!r})")
        return
    try:
        registered = muse_native.register_configs()
    except Exception as exc:
        _log(f"[muse-native] NOT active: config registration failed ({exc!r})")
        return
    mod._muse_native_configs = True
    _log(f"[muse-native] upstream {muse_native.upstream_ref()}")
    _log(f"[muse-native] registered config types: {', '.join(registered)}")


# ---------------------------------------------------------------------------
# SHIM 8 -- EAGLE3 aux-layer lookup for a native multimodal model (NATIVE=1)
# ---------------------------------------------------------------------------
def _patch_eagle3_lookup(mod):
    """Backport one core fix the native model depends on.

    PR #51655 is branched off a newer main than v0.27.1, and its native model
    relies on a change to SupportsEagle3.set_aux_hidden_state_layers that 0.27.1
    does not have. 0.27.1 insists the language model have a further `.model`:

        parent_ref = self.get_language_model()
        assert hasattr(parent_ref, "model"), \
            "Model instance must have 'model' attribute to set number of layers"
        parent_ref.model._set_aux_hidden_state_layers(layers)

    MuseGlimmerForCausalLM.get_language_model() returns the decoder itself,
    which IS the EagleModelMixin and has no further `.model` -- so with
    NATIVE=1 SPEC=1 that assertion fires and the engine dies before loading.
    Newer main unwraps only when there is something to unwrap. Same logic here,
    verbatim from the PR's interfaces.py.

    Only touches the aux-hidden-state path, so it is inert unless speculative
    decoding is on. Delete when ./build pins a vLLM that has it.
    """
    if os.environ.get("MUSE_NATIVE", "0") != "1":
        return
    cls = getattr(mod, "SupportsEagle3", None)
    mixin = getattr(mod, "EagleModelMixin", None)
    if cls is None or mixin is None or getattr(cls, "_muse_eagle3_lookup", False):
        return

    def set_aux_hidden_state_layers(self, layers):
        parent_ref = self
        if hasattr(self, "get_language_model"):
            parent_ref = self.get_language_model()
        elif hasattr(self, "language_model"):
            parent_ref = self.language_model
        holder = getattr(parent_ref, "model", parent_ref)
        assert isinstance(holder, mixin), (
            "Model instance must inherit from EagleModelMixin to set auxiliary layers"
        )
        holder._set_aux_hidden_state_layers(layers)

    cls.set_aux_hidden_state_layers = set_aux_hidden_state_layers
    cls._muse_eagle3_lookup = True
    _log("[muse-native] backported the EAGLE3 aux-layer lookup from newer main")


# ---------------------------------------------------------------------------
# SHIM 9 -- let an FP8-quantized lm_head reach the kernel (MUSE_FP8_LMHEAD=0 to disable)
# ---------------------------------------------------------------------------
# vLLM supports a quantized lm_head: get_quant_method has an explicit
# `isinstance(layer, ParallelLMHead)` branch, and it binds
# CompressedTensorsW8A16Fp8 correctly for our fp8lmhead checkpoint. Loading then
# dies one step later:
#
#     humming_utils.py:465 in prepare_humming_layer
#       shape_n_stacks = layer.output_partition_sizes
#     AttributeError: 'ParallelLMHead' object has no attribute
#                     'output_partition_sizes'
#
# ParallelLMHead subclasses VocabParallelEmbedding, not LinearBase, so it never
# gets the attribute the Linear path takes for granted. This is an upstream
# oversight rather than a design decision -- the comment two lines ABOVE the
# failure special-cases this exact class:
#
#     # Use hasattr rather than getattr's default arg, which is evaluated
#     # eagerly and would raise on layers lacking input_size (e.g. ParallelLMHead)
#
# They guarded input_size_per_partition for ParallelLMHead and then used
# output_partition_sizes unguarded on the very next line.
#
# CompressedTensorsW8A16Fp8.create_weights already RECEIVES output_partition_sizes
# and stores it as `logical_widths`; it just never sets the name the humming
# kernel reads. So the fix is to record it under both names. Only fills the
# attribute when it is missing, which makes it a no-op for every LinearBase layer.
def _patch_w8a16_fp8_lmhead(mod):
    if os.environ.get("MUSE_FP8_LMHEAD", "1") != "1":
        return
    cls = getattr(mod, "CompressedTensorsW8A16Fp8", None)
    if cls is None or getattr(cls, "_muse_opsizes_installed", False):
        return
    real_create_weights = cls.create_weights

    def create_weights(self, layer, input_size_per_partition,
                       output_partition_sizes, *args, **kwargs):
        real_create_weights(self, layer, input_size_per_partition,
                            output_partition_sizes, *args, **kwargs)
        # Mirror what LinearBase.__init__ would have set, deriving each the same
        # way it does rather than guessing:
        #   linear.py:347-349  output_partition_sizes = [output_size]
        #   linear.py:267      has_bias = bias   (ParallelLMHead registers
        #                      bias=None when bias=False, so test for None)
        # Each is filled ONLY when absent, so this is a no-op on every real
        # LinearBase layer and cannot mask an upstream fix.
        if not hasattr(layer, "output_partition_sizes"):
            layer.output_partition_sizes = list(output_partition_sizes)
        if not hasattr(layer, "has_bias"):
            layer.has_bias = getattr(layer, "bias", None) is not None
        if not hasattr(layer, "layer_name"):
            layer.layer_name = getattr(layer, "prefix", "") or ""
        return None

    cls.create_weights = create_weights
    cls._muse_opsizes_installed = True
    _log("[muse-fp8-lmhead] output_partition_sizes shim installed")


_PATCHES = {
    MODELING: _patch_modeling,
    SPECULATIVE: _patch_speculative,
    REASONING: _patch_parser_lookup,
    TRANSFORMERS_BACKEND: _patch_transformers_backend,
    TU_CONFIG: _patch_native_configs,
    INTERFACES: _patch_eagle3_lookup,
    W8A16FP8: _patch_w8a16_fp8_lmhead,
}


class _PostImportPatcher(importlib.abc.MetaPathFinder):
    """Let the normal machinery import a target, then patch it."""

    def find_spec(self, fullname, path=None, target=None):
        patch = _PATCHES.get(fullname)
        if patch is None:
            return None
        sys.meta_path.remove(self)          # avoid recursing into ourselves
        try:
            spec = importlib.util.find_spec(fullname)
        finally:
            sys.meta_path.insert(0, self)
        if spec is None or spec.loader is None:
            return None
        real_exec_module = spec.loader.exec_module

        def exec_module(module):
            real_exec_module(module)
            patch(module)

        spec.loader.exec_module = exec_module
        return spec


_pending = False
for _target, _patch in _PATCHES.items():
    if _target in sys.modules:
        _patch(sys.modules[_target])
    else:
        _pending = True
if _pending:
    sys.meta_path.insert(0, _PostImportPatcher())
