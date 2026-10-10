# Repin the vLLM main snapshot

A runbook for Claude. To run it, say: **"follow ./repin.md"** (newest `main`) or
**"follow ./repin.md to <sha>"**. It moves `VLLM_MAIN_SHA` in `container/versions.env`, re-checks
everything the image and `./qwen3.8-flash-next/run` depend on, makes **one commit**, and reports.
It never builds and never pushes. The owner runs the build. (This is the hands-on, local
counterpart of `qwen3.8-flash-next/upstream-watch-prompt.md`, which is a remote read-only watcher.)

Templates for the result: `git show 6a9bac4` (flashinfer moved) and `git show ddc82f5` (nothing
build-side moved, but the Qwen4Exp config did).

## Rules

- Start from a clean tree on `dev`. Commit only this repin. Do not push. Do not run
  `./container/build` without `PREFLIGHT_ONLY=1`. Do not touch the running `vllm` container.
  Every docker call below is a throwaway `--rm` with no GPU.
- Scratch files go in the session scratchpad, not the repo.
- **Stop and ask** before editing if any of these happen: `torch==` / `torchaudio==` /
  `torchvision==` change in `requirements/cuda.txt` (torch source rebuild plus a re-pin of the
  release set too), a launcher workaround breaks with no obvious fix, or the dry-run resolve
  moves torch / torchaudio / flashinfer when it shouldn't.
- When a check passes, say what it covered. When something can't be checked here (anything at
  runtime on the GPU), say "not verified".

## 0. Setup

```bash
V=/mnt/noir/scratch/ai/vllm/build/src/vllm          # ./container/build's vLLM checkout
source container/versions.env; OLD=$VLLM_MAIN_SHA
git -C $V fetch --tags --force origin
NEW=$(git -C $V rev-parse ${TARGET:-origin/main})    # or the sha the user gave
git -C $V merge-base --is-ancestor $OLD $NEW && git -C $V rev-list --count $OLD..$NEW
git -C $V log -1 --format='%h %cs %s' $NEW
OLD_IMG=$(docker images iphands/vllm-blackwell --filter label=ai.vllmcustom.vllm.ref=$OLD \
            --format '{{.Repository}}:{{.Tag}}' | grep -m1 -- '-main-vllm')
```

If `$NEW == $OLD`, report "already pinned" and stop.

## 1. Build-side diff

```bash
git -C $V diff --stat $OLD $NEW -- requirements/ setup.py pyproject.toml use_existing_torch.py CMakeLists.txt cmake/
git -C $V diff $OLD $NEW -- requirements/cuda.txt requirements/common.txt requirements/build/ CMakeLists.txt cmake/
git -C $V log --format='%h %cs %s' $OLD..$NEW -- requirements/cuda.txt requirements/common.txt requirements/build/ CMakeLists.txt cmake/
```

Attribute each pin change to its commit/PR. Then:

- **flashinfer-python / flashinfer-cubin** moved: set `VLLM_MAIN_FLASHINFER_REF=v<that version>`.
  Skim the flashinfer commits between the two refs for anything sm120 or AOT-build related
  (`git -C <build>/src/flashinfer log --oneline vOLD..vNEW` after fetching). flashinfer-build will
  recompile (~35 min).
- **cmake/external_projects/vllm_flash_attn.cmake** changed: `GIT_PROGRESS TRUE` must still appear
  exactly once in the git branch of the FetchContent declaration (the Dockerfile `sed` hooks it). If
  `GIT_TAG` moved, fetch the new flash-attention `CMakeLists.txt` and confirm both anchors in
  `container/patches/vllm-flash-attn-arch.py` appear exactly once.
- Any other external project or new kernel: confirm what it compiles for `CUDA_ARCHS=12.0`
  (empty / 12.0 / 12.0f). Flag anything that compiles for an arch other than sm_120.
- Changes under `docker/` upstream are usually irrelevant (we have our own Dockerfile). Skim for
  new system deps (the libdw case in `container/Dockerfile`).

## 2. Runtime resolve (dry-run in the current image)

```bash
R=<scratchpad>/req-$NEW; mkdir -p $R && git -C $V archive $NEW requirements | tar -x -C $R
docker run --rm --entrypoint bash -v $R/requirements:/req:ro "$OLD_IMG" \
  -c 'cd /req && uv pip install --dry-run -r cuda.txt 2>&1 | tail -40'
```

The in-place dry-run only shows what *must* change. A package whose range just got looser (e.g. a
raised ceiling) stays at the image's version, but the real build re-resolves the vllm-openai layer
fresh and takes the newest allowed version. So re-run with `--upgrade-package <name>` for every
package whose requirement line changed in step 1 (the bdd31c3 case: "no changes" in place, but
transformers 5.17.0 -> 5.18.0 in the build).

List every `-`/`+` pair. torch, torchaudio and flashinfer must stay put unless step 1 moved
flashinfer. For each moved package, say whether it is on the Flash-Next path (e.g. nvidia-cutlass-dsl
is: Qwen4Exp's hc_down_silu / hyperconnection kernels are CuTe DSL). If a version goes *down*, find
the requirement that caused it (`git log -S` on the pin line) and say why.

## 3. Flash-Next code path

```bash
git -C $V log --format='%h %cs %s' $OLD..$NEW -- vllm/models/qwen4_exp vllm/config/engram.py \
  vllm/model_executor/models/registry.py vllm/envs.py vllm/platforms/interface.py
```

Re-verify every assumption in the `qwen3.8-flash-next/run` header at `$NEW` (`git -C $V grep ... $NEW -- <path>`):

| Assumption | Where |
|---|---|
| `cpu_offload` in EngramConfig | `vllm/config/engram.py` |
| `class Qwen4ExpPLEPinnedHostEmbedding`, chosen when cpu_offload | `vllm/models/qwen4_exp/common/ngram_embedding.py`, `nvidia/ngram_embedding.py` |
| `isinstance(quant_config, CompressedTensorsConfig)` branch (PLE overlay stays skipped; its absence = regression) | `common/ngram_embedding.py` |
| `getattr(config, "ple_embedding_dtype", ...)` is what picks the PLE method | `nvidia/ngram_embedding.py` |
| `Qwen4ExpForConditionalGeneration`, `Qwen4ExpMTP` registered | `vllm/model_executor/models/registry.py` |
| `VLLM_GDN_DECODE_KERNEL` env | `vllm/envs.py` |
| `"qwen3"` reasoning parser, `"qwen3_coder"` tool parser | `vllm/reasoning/__init__.py`, `vllm/tool_parsers/__init__.py` |
| `_update_nested` / `_apply_dict_overrides` still merge nested `--hf-overrides` | `vllm/config/model.py` |
| QSA ring assert still not folded into the block-size LCM (if it is now, the `BLOCK_SIZE=auto` workaround can go) | `common/qsa_cache.py`, `vllm/platforms/interface.py` `_align_hybrid_block_size` |
| PLE table still one `torch.empty(..., pin_memory=True)` (if chunked / registered, `PINNED_ROUND_MB` can go) | `common/ngram_embedding.py` `allocate_embedding_weight` |

If `requirements/common.txt`'s `transformers` line changed, or anything touched the Qwen4Exp
config / layer types, load **both** checkpoints' configs with the transformers version the dry-run
picked (vLLM uses transformers' own `Qwen4ExpConfig` since vllm#57387). The checkpoints still label
their QSA layers `full_attention`, and the model code only accepts `qwen_sparse_attention`.

```bash
cat > <scratchpad>/cfgcheck.py <<'PY'
import collections, transformers
from transformers import AutoConfig
c = AutoConfig.from_pretrained("/model"); t = c.get_text_config()
print("transformers", transformers.__version__, type(t).__module__, type(t).__name__)
print("layer_types", collections.Counter(t.layer_types))   # want qwen_sparse_attention, not full_attention
for k in ["ple_embedding_dtype", "indexer_compress_ratio", "indexer_n_heads", "mtp_num_hidden_layers"]:
    print(f"  {k} = {getattr(t, k, '<MISSING>')!r}")
setattr(c.text_config, "ple_embedding_dtype", "float8_e4m3fn")   # what the launcher's --hf-overrides does
print("override lands:", c.get_text_config().ple_embedding_dtype)
PY
for m in nvidia/Qwen3.8-Flash-Next-NVFP4 primitive-ai/Qwen3.8-Flash-Next-mixed-NVFP4-FP8; do
  docker run --rm --entrypoint bash -v <scratchpad>/cfgcheck.py:/c.py:ro \
    -v "$(readlink -f models/vllm)/$m":/model:ro "$OLD_IMG" \
    -c "uv pip install -q transformers==<resolved version> 2>/dev/null; python /c.py 2>&1 | grep -vi warn"
done
```

## 4. Overlays (hook by name, no-op on drift)

Re-grep every name in the hook tables of `container/patches/boot-timing/UPSTREAM` and
`container/patches/draft-load-filter/UPSTREAM` at `$NEW`, and say which hooked files changed
(`git -C $V diff --stat $OLD $NEW -- <those files>`). For draft-load-filter, `_remap_mtp_weight_name`
and the `"mtp.": None` mapper entry must be untouched.

```bash
python3 container/patches/boot-timing/test_boot_timing.py
python3 container/patches/draft-load-filter/test_draft_load_filter.py
```

## 5. Version prefix

`DEFAULT_VLLM_NEXT_VERSION` must be the release *after* the newest release branch whose cut point
is behind `$NEW`:

```bash
B=$(git -C $V branch -r | grep -oE 'origin/releases/v[0-9.]+' | sort -V | tail -1)
CUT=$(git -C $V merge-base $B origin/main); git -C $V merge-base --is-ancestor $CUT $NEW && echo "past $B cut ($CUT)"
```

If the snapshot is past a cut newer than the one in the `versions.env` comment, bump to the next
minor and update that comment (pattern: the 0.31.0 -> 0.32.0 move in `6a9bac4`).

## 6. Edit (same style as the template commits)

- `container/versions.env`: `VLLM_MAIN_SHA`, the `# main @ <date> (was <old7>, <date>): torch X / flashinfer Y.`
  line, `VLLM_MAIN_FLASHINFER_REF` if it moved, `DEFAULT_VLLM_NEXT_VERSION` if step 5 says so.
- `container/Dockerfile` header: the `pinned at <sha7>, <date>; was ...` line, plus a
  `<old7> -> <new7> (<date>, N commits): ...` paragraph above "Otherwise nothing in this file is
  snapshot-specific". It covers which layers rebuild, the runtime resolve and CMake.
- `container/patches/{boot-timing,draft-load-filter}/UPSTREAM`: a "re-grepped at <sha7>" note.
- `qwen3.8-flash-next/run` header: `Last verified against main @ <sha7>`, append the old sha to the
  re-checked list, and update any workaround note that step 3 changed. Edit code only if an
  assumption broke.
- `qwen3.8-flash-next/upstream-watch-prompt.md` / `README.md`: only if a fact they state changed.

## 7. Verify

```bash
PREFLIGHT_ONLY=1 VLLM_REF=main ./container/build      # syncs src trees to $NEW, checks pins + cubin + torchvision wheel
bash -n qwen3.8-flash-next/run && shellcheck -x qwen3.8-flash-next/run
```

Launcher args, with the final `docker run` stubbed out (the new image isn't built yet, so point
`IMAGE_TAG` at the old one):

```bash
mkdir -p <scratchpad>/stub; REAL=$(command -v docker); cat > <scratchpad>/stub/docker <<EOF
#!/bin/bash
for a in "\$@"; do [[ \$a == serve ]] && { echo "STUB: launch suppressed"; exit 0; }; done
exec $REAL "\$@"
EOF
chmod +x <scratchpad>/stub/docker
PATH=<scratchpad>/stub:$PATH IMAGE_TAG=${OLD_IMG#*:} ./qwen3.8-flash-next/run 2>&1 | grep -E '^## |^docker '
```

Expect: PLE overlay not mounted (vllm#59431), PLE dtype override `float8_e4m3fn`, no `--block-size`
at MTP 3, and `--kv-cache-dtype fp8 --speculative-config ...3 --moe-backend humming
--gpu-memory-utilization 0.96` on `/models/nvidia/Qwen3.8-Flash-Next-NVFP4`.

## 8. Commit

One commit, `container: pin main snapshot at <sha7>`. Body as in the templates: what moved and
why (with PR numbers), what the dry-run resolved, the launcher and overlay findings, a `Checked:`
line, `Not verified:` (runtime on the GPU), and `Not built yet: VLLM_REF=main ./container/build.`
No co-author trailer. Don't push.

## 9. Report (short)

- `<old7> -> <new7>` (date, N commits), commit hash.
- A table of what moved (package / was / now / on the Flash-Next path?), and which build stages
  will rebuild.
- Anything that needed a decision or a code change, and anything not verified.
- Next steps for the owner:
  1. `VLLM_REF=main ./container/build && ./container/push`
  2. `./qwen3.8-flash-next/run` (a bare launch is the daily driver).

  Until the new image exists, the launcher refuses, because it looks the image up by
  `VLLM_MAIN_SHA`. To run the previous image meanwhile, use `IMAGE_TAG=<old tag> ./qwen3.8-flash-next/run`.
