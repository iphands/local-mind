# Looped transformers: can vLLM loop any model?

Researched 2026-10-07. The trigger was a r/LocalLLaMA thread,
["Microsoft confirms OpenAI has been using Looped Transformers in the GPT-6 series"](https://www.reddit.com/r/LocalLLaMA/comments/1wz00vv/microsoft_confirms_openai_has_been_using_looped/).

Question: can we teach the vLLM runner to loop through part of the layer stack 2 or 4 times, for
any model? Or does the model have to be built and trained for it?

## TL;DR

- **Running the loop in vLLM is easy.** vLLM already serves two models that were trained to
  loop (IQuest LoopCoder and HRM). Each pass needs its own KV/state slot, and the weights can be
  shared.
- **Getting better output is the hard part.** Looping pays off reliably only when the model was
  trained to loop, either from scratch or by converting an existing model with continued
  pretraining. On an ordinary model the best case is a small gain from repeating one carefully
  chosen middle block about once. 4 passes of an arbitrary block will most likely make it worse.
- **It is not free at inference time.** Looping saves weight memory, not bandwidth. Each pass
  reads the layer weights again and needs its own KV/state, so decode time and KV memory grow
  roughly like they would with extra layers.
- **Don't try it on the daily driver.** Qwen3.8-Flash-Next (Qwen4Exp) is about the worst
  candidate. Qwen3.6-27B is the right test model.

## What a looped transformer is

A normal transformer runs each token through N distinct blocks once. A looped (also called
"recurrent-depth" or "universal") transformer runs the hidden state through the same block, or
group of blocks, several times. The weights are shared across passes. Common variants:

- **Whole-stack loop:** run blocks 1..N, then run 1..N again. Nanbeige4.2-3B loops 22 blocks
  ×2 for 44 block applications. IQuest LoopCoder also loops the whole stack.
- **Prelude / recurrent core / coda:** a few unique layers in, a looped middle block, a few
  unique layers out (Huginn, McLeish et al., Shapiro).
- **Adaptive depth:** a learned halting score or router decides how many passes each token gets
  (Universal Transformers' ACT, Ouro's early exit, Mixture-of-Recursions routing).
- **Hierarchical:** nested fast and slow loops (HRM, which runs inner L cycles inside outer H
  cycles).

Costs and benefits:

- **Training compute:** a looped model costs about the same compute per token as an unrolled
  model of equal depth. At equal compute it needs fewer parameters, and SMELT reports 6.8–18%
  less training compute to reach the same validation loss.
- **Reasoning:** reported gains are mostly on serial, iterative tasks (math, code, multi-step
  reasoning) rather than on knowledge recall, because fewer parameters means less room to store
  knowledge.

## The "Microsoft confirms" claim

A public Microsoft page says **GPT-6.1 Sol uses the same base weights as GPT-6 Sol with "two
inference passes instead of three"**, tuned for a cheaper serving profile. It surfaced on X
(@Algorithon, 787k+ views) and r/LocalLLaMA on 2026-10-06.

- Microsoft **never uses the word "looped"**. "Microsoft confirms" is a reading of that one
  line. The reading is plausible, but it is not an acknowledgment.
- It does fit earlier reporting by The Information about OpenAI's GPT-6 "Astra" using looped
  transformers.
- If it is true, it supports the main point of this note: you can change the pass count on the
  same weights (3 → 2) only because the model was trained for a variable loop count. Huginn
  randomizes the recurrence count during training, and Ouro learns when to exit early, for the
  same reason.

## Can vLLM loop any model? The mechanics

vLLM's model code is plain PyTorch, so the loop itself is just a `for` around a range of layers.
The real constraint is **per-pass cache state**.

### KV and state slots are tied to layer names

- `vllm/model_executor/layers/attention/attention.py:444`: every `Attention` module registers
  itself in the forward context under its `prefix`, and a repeated prefix raises
  `ValueError("Duplicate layer name: ...")`. The KV cache manager gives each registered layer
  its own cache slot.
- Pass 2 through layer *i* produces **different** K/V than pass 1 did, because its input hidden
  state is different. If you called the same `Attention` module twice, pass 2 would overwrite
  pass 1's cache.
- GDN and Mamba-style linear-attention layers have the same issue with their per-layer
  recurrent state (conv state plus SSM/delta state). `mamba/short_conv.py:87` has the same
  duplicate-name guard.
- **Fix:** for each extra pass, create a separate cache-owning module with a unique prefix that
  shares the original's weights.

### vLLM already does this for models trained to loop

Paths below are relative to the vLLM checkout at `/mnt/noir/scratch/ai/vllm/build/src/vllm`,
pinned at `b6d8e8a` (2026-10-06).

- **`vllm/model_executor/models/iquest_loopcoder.py`** (IQuest LoopCoder, `loop_num`, default 2)
  - `:133-176`: one `Attention` per loop pass. The prefix is rewritten from `layers.{i}` to
    `layers.{loop_idx * total_layers + i}`, so each pass gets its own KV slot.
    `qkv_proj`/`o_proj`/rotary are shared.
  - `:178-207`: pass 1 is ordinary attention. Passes 2+ compute a "global" attention that reads
    pass 1's KV (`global_attn(q, None, None)`) and a "local" attention over a 64-token sliding
    window. A learned `LoopGateProjection` (`:279`) mixes the two. This saves KV memory: later
    passes only keep a 64-token window.
  - `:452-462`: the model forward is literally `for loop_idx in range(loop_num): for layer in
    layers: ...`.
- **`vllm/model_executor/models/hrm_text.py`** (HRM text, hierarchical H/L cycles)
  - `:185-224`: `attn_per_step` is a `ModuleDict` with one attention instance per recurrence step
    the stack actually runs. Each has its own KV slot, and the projections are shared.
  - `:354-370`: nested `H_cycles` × `L_cycles` forward.

These are the patterns to copy if we ever build a generic loop patch.

### Other mechanical concerns for a generic patch

- **torch.compile and CUDA graphs:** a fixed loop count is fine. It just unrolls. An adaptive
  per-token loop count is much harder.
- **Hybrid KV cache manager:** extra attention or GDN layers change the per-group layer counts
  and page sizes. That should work, but it needs checking at boot.
- **Quantized weights:** shared weights must point at the *post-processed* tensors, after
  `process_weights_after_loading`. A shallow copy of the cache-owning submodule that keeps the
  same `Parameter` objects avoids allocating new weights.
- **Spec decode (MTP / EAGLE):** the drafter was trained on the un-looped model's final hidden
  state, so expect draft acceptance to drop.

## Will it help an ordinary model? Usually not without training

A model that was not trained to loop has never seen its own output fed back in. Layer *i*
expects the hidden-state distribution produced by layer *i-1*, not by layer *i* (or *j > i*).

### Without training: RYS layer duplication

David Ng's "RYS" (Repeat Your Self) experiments are the best evidence for looping a model with
no training:

- **Qwen2-72B:** duplicating a block of **7 middle layers**, with no weight changes and no
  training, made the #1 model on the HuggingFace Open LLM Leaderboard. As of 2026 the top 4
  models there are still descendants of it.
- **RYS II on Qwen3.5-27B** (64 layers, GDN hybrid, the same layout as our Qwen3.6-27B).
  Pareto-optimal duplications:

  | Duplicated layers | Compute overhead | Math gain | EQ gain |
  |---|---|---|---|
  | (33,34) | +1.56% | +0.018 | +0.095 |
  | (31,34) | +4.69% | +0.021 | +0.097 |
  | (30,35) | +7.81% | +0.028 | +0.098 |
  | (26,34) | +12.5% | +0.028 | +0.101 |

  - Contiguous mid-stack blocks worked best, and more complex multi-block compositions were not
    worth it.
  - Repeating layer 10 three times gave +0.077 on math at 3.1% overhead, but the EQ results were
    inconsistent.
  - The author found **no evidence that two back-to-back copies of a block beat one copy**, and
    each additional block buys less than the last while overhead grows linearly.
- Finding these spots takes a heatmap sweep over (start, end) ranges. Most placements are
  neutral or harmful, and early or late layers tend to break the model.
- Deployment so far:
  - Full weight copies (S/M/L/XL variants on HF).
  - Pointer-based duplication in ExLlamaV3 is pending. It adds no VRAM for weights; only the
    extra passes add KV.

**Bottom line for untrained looping:** 2 passes of a well-chosen mid-stack block is plausible
and may give a few points on math or reasoning. 2 or 4 passes of an arbitrary block will
probably degrade quality.

### With training: where the real gains are

- **Trained looped from scratch:**
  - Huginn (3.5B, randomized recurrence count, so test-time depth is adjustable).
  - Ouro (ByteDance LoopLM).
  - Nanbeige4.2-3B (22 blocks ×2).
  - IQuest LoopCoder and HRM, both already served by vLLM.
  - Mixture-of-Recursions (learned per-token depth).
- **Converted from a pretrained model with continued pretraining:**
  - McLeish et al. (2511.07384) convert pretrained non-recurrent LMs into depth-recurrent ones
    with a **curriculum of recurrences**, slowly increasing effective depth during training. On
    math, the converted model beats simply post-training the original at the same compute.
  - Shapiro (2608.11233) splits Qwen2.5-0.5B-Instruct into a Prelude, a weight-tied Recurrent
    Block and a Coda. One loop is an identity-preserving path, and a trainable bridge re-injects
    the Prelude representation on later loops.
  - Relaxed Recursive Transformers (Google DeepMind) tie the layers and add per-loop LoRA, then
    uptrain.
- Nanbeige found **training from scratch beat converting a pretrained model**. They also tried
  sharing the KV cache across passes: it halved the KV size but made the model worse.
- The loop count is usually fixed by training. A model can only vary it at inference (the
  GPT-6.1 Sol "2 instead of 3" case) if it was trained for that.

## The "no extra memory bandwidth" claim is mostly wrong for local decode

The Reddit comment says looping gives more compute "in a form that does not require additional
memory bandwidth". That is only partly right:

- **What looping saves:** parameter count, meaning VRAM *capacity* for weights. A 27B model
  looped 2× gets the depth of a much larger model while holding only 27B of weights.
- **Bandwidth:** decode is memory-bound, and every pass re-reads that layer's weights from VRAM
  unless they fit in L2. One Qwen3.6-27B layer is about 0.4 GB (24 GB NVFP4 / 64 layers), and
  the RTX PRO 6000 has 128 MB of L2. They don't fit, so pass 2 costs about the same time as a
  distinct layer would.
- **KV and state:** each pass needs its own KV/state (sharing it hurt quality in Nanbeige's
  tests), so KV memory and context capacity also grow with every repeated layer.
- **Net effect for local serving:** decode tok/s drops and KV capacity shrinks roughly in
  proportion to the number of repeated layer applications. The capacity win only matters if
  weights are what's stopping you from fitting a deeper model.

## Our models

### Qwen3.8-Flash-Next (Qwen4Exp, daily driver): don't

`nvidia/Qwen3.8-Flash-Next-NVFP4` is 124 GB. It has 48 layers, a 3:1 ratio of GDN linear
attention to QSA sparse attention, 512 experts with top-10 routing, hidden size 2560, and one MTP
layer. It is about the worst candidate:

- **Hyper-connections:** multi-stream residuals with a *delayed* combine that carries a
  `(block_output, injection)` tuple across layer boundaries
  (`vllm/models/qwen4_exp/nvidia/model.py:502-556`). A loop has to materialize or re-thread that
  state correctly.
- **PLE (per-layer n-gram embeddings):** keyed by absolute layer id
  (`ple_layer_ids`, `model.py:194-207`) and prefetched one layer ahead from pinned host RAM
  (`model.py:496-515`). The PLE layer is itself a `MambaBase` with state.
- **Cache state:** QSA sparse attention has its own indexer cache, and the GDN layers hold
  recurrent state. Both need per-pass slots.
- **MTP:** trained on the un-looped final hidden state, so expect lower acceptance at
  `SPEC_TOKENS=3`.
- **Memory:** the card already runs at `GPU_MEM_UTIL=0.96`, so there is no headroom for extra
  KV/state.

### Qwen3.6-27B: the right test bed if we ever try

`Qwen3_5ForConditionalGeneration`: 64 layers, 3:1 GDN hybrid, hidden 5120, dense FFN. Local
copies are BF16, FP8 (29 GB) and NVFP4 (24 GB).

It has the **same layout as the Qwen3.5-27B measured in RYS II**, so there are published
reference points: (33,34), (31,34), (30,35) and (26,34).

## If we ever do the experiment (not started)

### Recommended: vLLM layer-repeat runtime patch

- An env-gated monkeypatch such as `LAYER_REPEAT=30:35x2`, mounted at runtime the same way as
  `container/patches/draft-load-filter` (a `.pth` hook on site-packages), so **no rebuild** is
  needed.
- For each repeated pass, create cache-owning shadow modules (`Attention`, and the GDN layer for
  linear-attention layers) with unique prefixes beyond the real layer count, as LoopCoder does.
  They share the original's post-load weights, and the forward loops over the chosen range.
- This lets us sweep many ranges without rewriting the checkpoint.

### Alternative: offline duplicated checkpoint

- A script writes a model copy with layers `[a,b)` duplicated. It renames the tensors, updates
  `num_hidden_layers` and `layer_types`, and remaps any per-layer entries in the quant-config
  ignore list.
- No vLLM code is needed, but every configuration costs a ~24 GB rewrite plus VRAM for the
  copied layers.

### Verification for either approach

- Baseline vs. repeated on the same evals (a gsm8k/math subset, plus a perplexity sanity check).
- Decode tok/s and KV capacity from the boot log.
- It needs the GPU, so the owner swaps the daily driver out. Don't touch the running container.

## Sources

- r/LocalLLaMA thread: <https://www.reddit.com/r/LocalLLaMA/comments/1wz00vv/microsoft_confirms_openai_has_been_using_looped/>
- @Algorithon on X ("Microsoft confirms"): <https://x.com/Algorithon/status/2107287650881208694>
- Write-up of the Microsoft page line (GPT-6.1 Sol, two passes): <https://pasqualepillitteri.it/en/news/21350/microsoft-looped-transformers-gpt-6-1-sol-two-passes>
- Sebastian Raschka, "GPT-6 Astra, Looped Transformers, and Hidden Reasoning": <https://magazine.sebastianraschka.com/p/gpt-6-astra-looped-transformers-and>
- RYS II (layer duplication on Qwen3.5-27B): <https://dnhkng.github.io/posts/rys-ii/>
- Universal Transformers: <https://arxiv.org/abs/1807.03819>
- Huginn, "Scaling up Test-Time Compute with Latent Reasoning: A Recurrent Depth Approach": <https://arxiv.org/abs/2502.05171>
- Mixture-of-Recursions: <https://arxiv.org/abs/2507.10524>
- Ouro, "Scaling Latent Reasoning via Looped Language Models": <https://arxiv.org/abs/2510.25741>
- McLeish et al., "Teaching Pretrained Language Models to Think Deeper with Retrofitted Recurrence": <https://arxiv.org/abs/2511.07384>
- Nanbeige4.2-3B: <https://arxiv.org/abs/2607.22083>
- Shapiro, "Retrofitting Recurrent Depth into a Pretrained Language Model": <https://arxiv.org/abs/2608.11233>
- SMELT (6.8–18% training-compute savings): <https://arxiv.org/abs/2609.01343>
- Hyperloop Transformers: <https://arxiv.org/abs/2604.21254>
