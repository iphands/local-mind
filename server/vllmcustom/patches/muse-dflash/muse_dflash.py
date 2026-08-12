# SPDX-License-Identifier: Apache-2.0
"""DFlash drafter for Muse Glimmer, registered out-of-tree.

Muse-Glimmer-30B-assistant is a DFlash block-diffusion drafter (5 layers,
block_size 16, mask_token_id 201818) that reads the target's hidden states at
layers {1, 13, 25, 37, 49} of 52. vLLM 0.27.1 supports the *method* -- see
vllm/config/speculative.py and vllm/model_executor/models/qwen3_dflash.py --
but has no registered architecture for this checkpoint:

    vllm/transformers_utils/configs/eagle.py:63-72
        architectures = ["MuseGlimmerAssistantModel"]
                     -> ["DFlashMuseGlimmerAssistantModel"]

and that name is not in _SPECULATIVE_DECODING_MODELS (registry.py:607-667), so
resolution hard-fails. A DFlash* name never falls through to the Transformers
backend either, so there is no escape hatch -- the class has to exist.

Happily, almost nothing here is new. The checkpoint's decoder layers are
Qwen3-shaped *verbatim*:

    layers.N.self_attn.{q,k,v,o}_proj / q_norm / k_norm
    layers.N.mlp.{gate,up,down}_proj
    layers.N.{input,post_attention}_layernorm
    norm

so vLLM's DFlashQwen3Model already builds exactly the right module tree, and its
WeightsMapper already does the qkv/gate_up stacking. Only two tensors are named
differently, both in the "encoder" (the block that projects concatenated target
hidden states down into the drafter's width):

    encoder.fc.weight              [6656, 33280]  ->  fc.weight
    encoder.output_norm_enc.weight [6656]         ->  hidden_norm.weight

`hidden_norm` is the right destination, not a cosmetic rename:
qwen3_dflash.py:505-534 (_project_context_kv) applies `_hidden_norm_weight` to
the fc output before the fused context-KV GEMM -- i.e. vLLM's `hidden_norm`
*is* the encoder's output norm.

The fc width also checks out without any override: _get_dflash_fc_input_size
(qwen3_dflash.py:75-83) computes hidden_size * len(aux_layers) = 6656 * 5 =
33280, exactly the shipped tensor's shape. If that ever disagrees you get a
loud ValueError from combine_hidden_states rather than silent garbage.

So this file is a rename plus two guards. Everything structural --
precompute_and_store_context_kv, combine_hidden_states, the block-diffusion
mask-token plumbing, per-layer causality -- is inherited untouched.

The checkpoint ships NO embed_tokens and NO lm_head (58 tensors, none of
either), so both are borrowed from the target by the generic proposer
(vllm/v1/spec_decode/llm_base_proposer.py:1423-1585). See MUSE_DRAFT_EMBED_NORM
below for the one wrinkle that creates.

Registered by run-muse via
    --model-class-overrides '{"DFlashMuseGlimmerAssistantModel": "muse_dflash:MuseGlimmerDFlashForCausalLM"}'
with this directory on PYTHONPATH. The draft config is reshaped into the layout
vLLM's DFlash code expects by patches/muse-embed-norm/sitecustomize.py -- see
_muse_draft_config_override() there.

Delete this patch once vLLM ships a native Muse Glimmer DFlash drafter.
"""

import os
import sys
import types
from collections.abc import Iterable

import torch
import torch.nn.functional as F

from vllm.config import VllmConfig
from vllm.model_executor.models.qwen3_dflash import DFlashQwen3ForCausalLM

# encoder.* -> the names DFlashQwen3Model builds. Applied as prefix renames on
# the raw checkpoint stream, before vLLM's own mapper stacks qkv/gate_up.
_WEIGHT_RENAMES = {
    "encoder.fc.": "fc.",
    "encoder.output_norm_enc.": "hidden_norm.",
}

# The drafter has no embedding of its own, so it is handed the target's --
# which, on this model, is the module patches/muse-embed-norm class-swaps to
# re-apply Muse Glimmer's weight-less embedding RMSNorm. Sharing it verbatim
# would feed the drafter *normed* embeddings for its anchor and mask slots.
#
# That is WRONG, and the reference implementation says so twice. The embedding
# class carries the reason in a comment
# (transformers/models/muse_glimmer/modeling_muse_glimmer.py:436):
#     # Weight-less norm applied on top of the embeddings - cannot be merged to
#     # the embedding matrix, as Dflash implem needs to embed without the norm
# and the reference generator bypasses the module to get at the raw table
# (transformers/generation/candidate_generator.py, DFlashTokenCandidateGenerator):
#     # The assistant needs embedding without norm thus take the lookup table
#     # and call `F.embedding`
#     noise_embeds = F.embedding(noise_ids, self.main_model_input_embeddings.weight)
#
# So the default is OFF: the drafter reads raw embedding rows. The target is
# untouched either way. MUSE_DRAFT_EMBED_NORM=1 restores the normed lookup if
# you want to A/B it. Symptom of the wrong choice: the server boots clean and
# generates correctly (rejection sampling protects output quality) but the draft
# acceptance rate sits near zero and there is no speedup.
_EMBED_NORM = os.environ.get("MUSE_DRAFT_EMBED_NORM", "0") == "1"


def _embed_unnormed(model, input_ids: torch.Tensor) -> torch.Tensor:
    """Raw embedding lookup, skipping the target's weight-less embedding RMSNorm.

    This is the reference implementation's own move, verbatim:
        noise_embeds = F.embedding(noise_ids, self.main_model_input_embeddings.weight)
    Reading .weight sidesteps the class swap patches/muse-embed-norm installs on
    the shared module, without disturbing the target, and stays a single static
    op -- which matters because on the V2 runner this executes inside the
    drafter's @support_torch_compile'd forward.

    Correct only at TP=1, where VocabParallelEmbedding holds the whole table;
    MuseGlimmerDFlashForCausalLM.__init__ enforces that before installing it.
    """
    return F.embedding(input_ids, model.embed_tokens.weight)


def _rename(
    weights: Iterable[tuple[str, torch.Tensor]],
) -> Iterable[tuple[str, torch.Tensor]]:
    for name, weight in weights:
        for old, new in _WEIGHT_RENAMES.items():
            if name.startswith(old):
                name = new + name[len(old) :]
                break
        yield name, weight


class MuseGlimmerDFlashForCausalLM(DFlashQwen3ForCausalLM):
    """Muse Glimmer's DFlash drafter. Qwen3-shaped; only the encoder is renamed."""

    def __init__(self, *, vllm_config: VllmConfig, prefix: str = ""):
        super().__init__(vllm_config=vllm_config, prefix=prefix)

        # All 5 draft layers are sliding_attention, and DFlash writes the
        # verifier context K/V at ABSOLUTE cache slots
        # (v1/spec_decode/utils.py:537-546 -> qwen3_dflash.py:605-619). If those
        # layers keep a SlidingWindowSpec, SlidingWindowManager nulls every block
        # older than the window, so any request past 2048 tokens writes into the
        # shared null block: silent garbage, not a crash. It also splits the KV
        # cache into a hybrid ~12-group layout that can fail page-size
        # unification outright.
        #
        # Clearing the window on the Attention module (and only there) flips the
        # lazily-read KV spec to FullAttentionSpec while leaving the window baked
        # into attn.impl, so SWA survives as a compute-time limit -- which is
        # exactly how the reference implementation treats it. Copied from
        # laguna_dflash.py:117-121, the other all-sliding DFlash drafter.
        # DFlashQwen3Model does not do this because its own checkpoints are mixed
        # sliding/full and take a different path.
        #
        # Done here rather than in a DFlashQwen3Model subclass on purpose: the
        # only timing requirement is "before get_kv_cache_spec", which the runner
        # calls long after load_model, and subclassing a @support_torch_compile
        # model means either re-decorating or duplicating its __init__.
        for layer in self.model.layers:
            if getattr(layer.self_attn, "sliding_window", None) is not None:
                layer.self_attn.attn.sliding_window = None

        # Un-normed embeddings, on BOTH runners. The V1 proposer calls the
        # drafter's embed_input_ids itself (llm_base_proposer.py:716, 971) and so
        # picks up the override below; the V2 speculator instead hands input_ids
        # straight to the model (gpu/spec_decode/dflash/speculator.py:236-239)
        # and the lookup happens inside DFlashQwen3Model.forward, where an
        # override on this class is never consulted. So rebind it on the inner
        # model too. It reads self.embed_tokens at call time, which is what lets
        # it see the target's table after the proposer shares it in.
        if not _EMBED_NORM:
            tp = vllm_config.parallel_config.tensor_parallel_size
            if tp != 1:
                raise NotImplementedError(
                    f"MUSE_DRAFT_EMBED_NORM=0 reads the embedding table directly and "
                    f"is only correct at tensor_parallel_size=1 (got {tp}). Run with "
                    "MUSE_DRAFT_EMBED_NORM=1, or teach _embed_unnormed to shard."
                )
            self.model.embed_input_ids = types.MethodType(_embed_unnormed, self.model)

        # The drafter config carries no vocab_size of its own (the checkpoint has
        # no embedding table); sitecustomize injects the target's. Assert rather
        # than trust it -- a mismatch here means the drafter shares an lm_head of
        # the wrong width, which produces plausible-looking garbage drafts and a
        # collapsed acceptance rate instead of an error.
        target_vocab_size = vllm_config.model_config.get_vocab_size()
        if self.config.draft_vocab_size != target_vocab_size:
            raise ValueError(
                "Muse Glimmer DFlash drafter shares the target's lm_head, so its "
                f"vocab must match: draft={self.config.draft_vocab_size} "
                f"target={target_vocab_size}. Check the vocab_size injected by "
                "patches/muse-embed-norm/sitecustomize.py against the target's "
                "text_config.vocab_size."
            )

        # Draft logits are compared against target logits by rejection sampling,
        # so the drafter must apply the same output_multiplier the target does
        # (run-muse feeds it to the target as text_config.logit_scale). An
        # unscaled drafter is 5.1x too sharp and acceptance collapses. Precedent:
        # Gemma4DSparkForCausalLM does the same with final_logit_softcapping.
        if getattr(self.config, "logit_scale", 1.0) == 1.0:
            print(
                "[muse-dflash] WARNING: draft logit_scale is 1.0; expected the "
                "target's output_multiplier (0.19611613513818404). Acceptance "
                "rate will be poor.",
                file=sys.stderr,
            )

        fc = getattr(self.model, "fc", None)
        kv_windows = sum(
            1 for layer in self.model.layers if layer.self_attn.attn.sliding_window is not None
        )
        print(
            f"[muse-dflash] drafter ready: {self.config.num_hidden_layers} layers, "
            f"fc in={fc.input_size if fc is not None else 'n/a'}, "
            f"vocab={target_vocab_size}, "
            f"logit_scale={getattr(self.config, 'logit_scale', 1.0)}, "
            f"embed_norm={'on' if _EMBED_NORM else 'off'}, "
            f"kv_sliding_layers={kv_windows} (want 0)",
            file=sys.stderr,
        )

    def embed_input_ids(
        self,
        input_ids: torch.Tensor,
        multimodal_embeddings=None,
        is_multimodal: torch.Tensor | None = None,
    ) -> torch.Tensor:
        # The V1 proposer embeds through here (llm_base_proposer.py:716, 971);
        # the V2 speculator does not -- see __init__.
        if _EMBED_NORM:
            return super().embed_input_ids(input_ids)
        return _embed_unnormed(self.model, input_ids)

    def load_weights(self, weights: Iterable[tuple[str, torch.Tensor]]):
        return super().load_weights(_rename(weights))
