# DiffusionGemma (`diffusiongemma-26B-A4B-it`) — serving notes & rebuild instructions

Status as of **2026-08-10**: **unblocked.** The original blocker — the local image being built from
vLLM v0.23.0, which had no DiffusionGemma architecture — was resolved by bumping `build` defaults to
**v0.27.0**, whose registry contains `DiffusionGemmaForBlockDiffusion`. Rebuild with `./build`, then
serve with `./run-diffusiongemma`.

This doc is now the standalone runbook for the model itself: what it is, how to verify a good load,
and the reference facts behind every non-obvious flag.

---

## 1. What this model is (and why it's different)

`/mnt/noir/scratch/ai/llm/models/vllm/google/diffusiongemma-26B-A4B-it`

- **Architecture:** `DiffusionGemmaForBlockDiffusion` (`config.json` → `"model_type": "diffusion_gemma"`).
  This is a **block-diffusion LLM (dLLM)**, *not* an autoregressive transformer. It generates by
  iteratively **denoising a fixed-length 256-token canvas** (`canvas_length: 256`) over up to 48
  denoising steps per block, instead of emitting one token left-to-right.
- **MoE:** 128 experts, top-8 (`num_experts: 128`, `top_k_experts: 8`). "A4B" ≈ 4B active params,
  ~26B total. bf16 checkpoint (`dtype: bfloat16`) — **not** NVFP4-quantized.
- **Multimodal:** Gemma4 processor with vision + audio + video towers. In vLLM, **image is
  supported; audio is not**.
- **Decoding config (`generation_config.json`):** `max_denoising_steps: 48`, `t_max: 0.8`,
  `t_min: 0.4`, `EntropyBoundSamplerConfig` with `entropy_bound: 0.1`, `confidence_threshold: 0.005`,
  `stability_threshold: 1`, and `max_new_tokens: 256` (this last one is a gotcha — see §4).
- Built on the Gemma4 backbone; `transformers_version: 5.8.0.dev0` (bleeding edge → needs
  `--trust-remote-code`).

Why it matters: dLLMs do not fit the standard autoregressive serving path. They need bidirectional
attention during denoise, iterative refinement, block-based generation, and a custom per-step sampler.
vLLM implements this via **model-runner-v2's `ModelState` abstraction** (`DiffusionGemmaModelState`),
reusing the speculative-decoding data path with minimal scheduler changes.

---

## 2. How it was unblocked (historical)

The original failure was a build/version problem, not a flag problem:

```
TransformersMultiModalMoEForCausalLM has no vLLM implementation, falling back to Transformers implementation.
Using Transformers modeling backend.
EngineCore failed to start. ... self.driver_worker.load_model()
```

`DiffusionGemmaForBlockDiffusion` was unregistered in v0.23.0, so vLLM fell back to a generic
Transformers MoE causal-LM path (autoregressive — the wrong decode anyway) and then died in
`load_model()`. Native support landed on `main` after the v0.23.0 release (blog 2026-06-10; merge
commits `043dc27` / `18e7d0b` / `297dd43`, "[Model] Add DiffusionGemma Support").

Rather than build from a moving `main` tip, we waited for a tag. `./build` now defaults to
**`VLLM_REF=v0.27.0`**, which registers the architecture. That bump also dragged torch
2.11.0 → 2.13.0 and flashinfer 0.6.13 → 0.6.16.post3 (a vLLM bump pins all three as a matched set —
always read the target tag's `requirements/cuda.txt` for `torch==` and `flashinfer-python==` before
moving `TORCH_REF`/`FLASHINFER_REF`). Because torch moved, this was a full multi-hour rebuild, not
the cheap cache-hit rebuild that an unchanged torch/flashinfer pin would have given.

**Tagging caveat (still true):** `./build` tags the result `iphands/vllm-blackwell:cu1303-sm120`
**and** `:latest`, so it moves `:latest` on this machine. To keep a known-good `:latest`, build under
a distinct `TAG=` and point the run scripts at it via `IMAGE_TAG=` (supported by `run` and
`run-diffusiongemma`) — noting `build` force-tags `:latest` regardless, so you would re-tag it back
afterward.

---

## 3. Run + verify

```bash
# Detached, then watch the load:
DETACH=1 ./run-diffusiongemma
docker logs -f vllm-diffusiongemma
```

`run-diffusiongemma` defaults: GPU 0 only, port **8701** (so it can coexist with `./run` on 8700),
container name `vllm-diffusiongemma`, attention backend `FLASH_ATTN`, canvas 256.

**Success criteria in the logs:**
- **Must be ABSENT:** "has no vLLM implementation, falling back to Transformers implementation" and
  "Using Transformers modeling backend".
- **Must be PRESENT:** a native load of `DiffusionGemmaForBlockDiffusion` (look for diffusion /
  denoise / `ModelState` lines), then "Application startup complete" on port 8799 (mapped to host
  8701).

**Smoke test:**
```bash
curl -s localhost:8701/v1/chat/completions -H 'content-type: application/json' \
  -d '{"model":"diffusiongemma","messages":[{"role":"user","content":"Say hi in one short sentence."}],"max_tokens":64}' | jq .
```

**Also confirm the autoregressive path still works** on the new image before trusting it — start one
of the normal models via `./run <model>` (it shares `:latest`) and do a quick completion. v0.27.0 is
a superset of v0.23.0, so MTP/NVFP4/qwen flows should be intact, but verify — note v0.27.0 removed the
`VLLM_NVFP4_GEMM_BACKEND` env, so `flashinfer-b12x` now routes through `--linear-backend` instead.

---

## 4. The `run-diffusiongemma` flags — what changed from `run` and why

`run-diffusiongemma` is a reworked copy of `run`, *not* a clone — most of `run`'s autoregressive knobs
are invalid for a dLLM.

**Dropped (unsupported or wrong for this model):**
- **MTP speculative decoding** (`--speculative-config`) — unsupported for dLLMs.
- **`--enable-prefix-caching`** — unsupported for dLLMs. (`--enable-chunked-prefill`,
  `--async-scheduling`, `--kv-cache-dtype fp8` also dropped: not part of the documented diffusion
  serving path.)
- **NVFP4 GEMM backend** (`VLLM_NVFP4_GEMM_BACKEND` / `NVFP4_BACKEND`) — this checkpoint is **bf16**,
  not NVFP4-quantized.
- **qwen3 reasoning/tool parsers** and the qwen `enable_thinking` chat-template kwargs → replaced with
  the **`gemma4`** parsers (`--reasoning-parser gemma4`, `--tool-call-parser gemma4`; `gemma4` is in
  this build's accepted `--tool-call-parser` list).

**Added / changed (diffusion- and Gemma-specific):**
- **`--attention-backend FLASH_ATTN`** (default; overridable to `TRITON_ATTN`). Per the vLLM blog,
  only **FLASH_ATTN (FlashAttention 4)** and **TRITON_ATTN** support the dynamic-per-sequence-causality
  denoise path. **`flashinfer` is NOT supported** for this model (it *is* the default in `run`).
- **`--generation-config vllm`** — see the gotcha at the end of this section.
- **`--hf-overrides '{"canvas_length": N}'`** — there is **no `--diffusion-config` CLI flag** (an
  earlier attempt with `--diffusion-config` failed: "unrecognized arguments"). `canvas_length` is read
  from the model's `config.json` (already 256); overriding it goes through `--hf-overrides`, which
  forwards keys onto the HF config. Knob: `CANVAS_LEN` (default 256, i.e. a no-op unless changed).
- **Image multimodal:** `--mm-processor-kwargs '{"max_soft_tokens": 1120}'` and
  `--limit-mm-per-prompt '{"image": 7}'` (recipe values). **Audio is unsupported in vLLM.** To drop
  the vision tower entirely (text-only, saves VRAM): `LANGUAGE_MODEL_ONLY=1 ./run-diffusiongemma`.
- **`--gpu-memory-utilization 0.85`** and **`--max-num-seqs 4`** — lower than `run`, because diffusion
  state buffers (per-request canvas + self-conditioning probabilities) are large.
- Port default **8701**, container name **`vllm-diffusiongemma`**, served name **`diffusiongemma`**.

**The `--generation-config vllm` gotcha (important):**
`--generation-config vllm` tells vLLM to load **no** checkpoint generation config (use vLLM defaults).
The recipe recommends this specifically because the checkpoint's `generation_config.json` pins
`max_new_tokens: 256`, which would otherwise impose a **server-wide 256-token output cap** on every
request. BUT: `vllm` also discards the checkpoint's diffusion sampler settings
(`EntropyBoundSamplerConfig`, `max_denoising_steps`, `t_max/t_min`). If output quality looks wrong
after it runs, switch to **preserve-and-lift** instead:

```
--generation-config auto
--override-generation-config '{"max_new_tokens": <big N>}'
```

`auto` loads the checkpoint config (keeping the diffusion sampler), and the override lifts only the
token cap. (A comment to this effect is already in `run-diffusiongemma`.)

---

## 5. Bind-mount / "model path" FAQ (not a bug)

`run-diffusiongemma` splits the path you pass into parent-folder + model-name and bind-mounts the
parent into the container, exactly like `run`:

```
-v /mnt/noir/scratch/ai/llm/models/vllm/google:/models:ro   # host dir, mounted read-only
serve /models/diffusiongemma-26B-A4B-it                      # same model, container-side path
```

The container can't read host paths directly — it only sees what's mounted — so the host path is
translated to its in-container location `/models/<name>`. Nothing is missing; this matches `run`'s
convention.

---

## 6. Contingencies

- **Attention backend unavailable:** if `FLASH_ATTN` (FA4) isn't present in the build, use
  `ATTN_BACKEND=TRITON_ATTN ./run-diffusiongemma`.
- **Build breakage at a tag:** if the vLLM source won't compile against the pinned torch /
  flashinfer, use the versions named in that tag's `requirements/cuda.txt` (pass matching
  `TORCH_REF` / `FLASHINFER_REF`; see §2).
- **Want `:latest` to stay on the previous image:** see the tagging caveat in §2.
- **Multimodal processor rejects `max_soft_tokens`:** fall back to text-only with
  `LANGUAGE_MODEL_ONLY=1`.

---

## 7. Reference / sources

- vLLM blog — "DiffusionGemma: The First Diffusion LLM (dLLM) Natively Supported in vLLM":
  https://vllm-project.github.io/2026/06/10/diffusion-gemma.html
- vLLM recipe — `Google/diffusiongemma-26B-A4B-it`:
  https://recipes.vllm.ai/Google/diffusiongemma-26B-A4B-it
- vLLM releases: https://github.com/vllm-project/vllm/releases
- DiffusionGemma merge commits (CI): `043dc27`, `18e7d0b`, `297dd43` ("[Model] Add DiffusionGemma Support")
- vLLM v0.27.0 dependency pins (`requirements/cuda.txt`): `torch==2.13.0`, `torchvision==0.28.0`,
  `torchaudio==2.11.0`, `flashinfer-python==0.6.16.post3` (cubin from `https://flashinfer.ai/whl/`,
  no longer on PyPI after 0.6.13)

**Local files involved:** `build` (vLLM/torch/flashinfer refs), `Dockerfile` (build stages),
`run-diffusiongemma` (serve flags), `run` (the autoregressive original this was adapted from).
