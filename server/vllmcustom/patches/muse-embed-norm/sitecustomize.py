"""Restore Muse Glimmer's embedding RMSNorm under vLLM's Transformers backend.

vLLM has no native MuseGlimmerForConditionalGeneration (checked on 0.25.2 and again
on 0.27.1.dev0, where this patch still fires), so it falls back to
the generic Transformers backend. That backend unconditionally swaps the model's
input embedding for a plain VocabParallelEmbedding:

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

Activated by putting this directory on PYTHONPATH; CPython imports `sitecustomize`
automatically at startup. Applies to the API server and every spawned worker.

Delete this patch once vLLM ships a native Muse Glimmer implementation.
"""

import importlib.abc
import importlib.util
import sys

TARGET = "transformers.models.muse_glimmer.modeling_muse_glimmer"


def _patch(mod):
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
            print(
                f"[muse-embed-norm] re-attached embed_norm to {cls.__name__}",
                file=sys.stderr,
            )
        self.embed_tokens = value

    text_model_cls.set_input_embeddings = set_input_embeddings
    text_model_cls._muse_embed_norm_patched = True


class _PostImportPatcher(importlib.abc.MetaPathFinder):
    """Let the normal machinery import TARGET, then patch it."""

    def find_spec(self, fullname, path=None, target=None):
        if fullname != TARGET:
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
            _patch(module)

        spec.loader.exec_module = exec_module
        return spec


if TARGET in sys.modules:
    _patch(sys.modules[TARGET])
else:
    sys.meta_path.insert(0, _PostImportPatcher())
