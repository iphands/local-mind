# Repin the vLLM main snapshot

A runbook for Claude. To run it, say: **"follow ./repin.md"** (newest `main`) or
**"follow ./repin.md to <sha>"**. It moves `VLLM_MAIN_SHA` in `container/versions.env`, re-checks
everything the image and `./qwen3.8-flash-next/run` depend on, dispositions every customization
(4b), makes **one commit**, and reports.
It never builds and never pushes — neither `git push` nor `./container/push`; both are the owner's.
The owner runs the build. (This is the hands-on, local
counterpart of `qwen3.8-flash-next/upstream-watch-prompt.md`, which is a remote read-only watcher.)

**Scope: the main snapshot.** The other launchers are bound to images this runbook never moves —
`muse/patches/` (and `qwen`/`laguna`) to the release build, `run-legacy` to the upstream PR-branch
image. Reviewing their shims belongs to a release bump; no runbook for that yet, and this is not
it. That boundary is a default, not a guard: `muse/run` mounts `muse/patches/` unconditionally and
`IMAGE_TAG` overrides the image with no compatibility check, so `IMAGE_TAG=<main tag> ./muse/run`
runs those shims over a snapshot and only the boot finds out.

Templates for the result: `git show 6a9bac4` (flashinfer moved) and `git show ddc82f5` (nothing
build-side moved, but the Qwen4Exp config did).

## Rules

- Start from a clean tree on `dev`. Commit only this repin. **Never `git push`** — the owner does
  (RULES.md Rule B1 #7); report the hash, never a push claim. Do not run
  `./container/build` without `PREFLIGHT_ONLY=1`. Do not touch the running `vllm` container.
  Every docker call below is a throwaway `--rm` with no GPU.
- Scratch files go in the session scratchpad, not the repo.
- **Stop and ask** before editing if any of these happen: `torch==` / `torchaudio==` /
  `torchvision==` change in `requirements/cuda.txt` — still a stop, but since the `4d9a8d6` repin the
  usual answer is no longer "re-pin the release set too": main's torch now has its own slots
  (`VLLM_MAIN_TORCH_REF` / `VLLM_MAIN_TORCHVISION_VERSION`), so the question is *take the tip with its
  own torch, or hold the snapshot back*, and that is the owner's call, not yours. `torchaudio==`
  moving is a harder stop: it is a second source-built ABI partner. Also stop if a launcher
  workaround breaks with no obvious fix, if the dry-run resolve
  moves torch / torchaudio / flashinfer when it shouldn't, or if a **4b** row cannot be answered
  with evidence — an unanswered row is a disposition you do not have, and step 6 runs on dispositions.
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
| `isinstance(quant_config, CompressedTensorsConfig)` branch (regression check only — the overlay that needed it was deleted at `4d9a8d6`; its absence = upstream regressed, and the launcher no longer covers it) | `common/ngram_embedding.py` |
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
their QSA layers `full_attention` in the raw `config.json`; transformers relabels them, so what
`AutoConfig` hands back says `indexed_attention`, and vLLM's `_QSA_LAYER_TYPES` accepts both spellings
(`("qwen_sparse_attention", "indexed_attention")` at `4d9a8d6` as at `698e79f`). So a cfgcheck that
prints `full_attention` **after** `AutoConfig` is the regression, and so is one that prints
`indexed_attention` while `_QSA_LAYER_TYPES` no longer lists it.

```bash
cat > <scratchpad>/cfgcheck.py <<'PY'
import collections, transformers
from transformers import AutoConfig
c = AutoConfig.from_pretrained("/model"); t = c.get_text_config()
print("transformers", transformers.__version__, type(t).__module__, type(t).__name__)
print("layer_types", collections.Counter(t.layer_types))   # post-AutoConfig: want indexed_attention x12 (raw config.json says full_attention; see above)
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

Re-grep every name in the hook tables of `container/patches/boot-timing/UPSTREAM`,
`container/patches/draft-load-filter/UPSTREAM`, `container/patches/spec-live/UPSTREAM`,
`container/patches/sm120-gemm-plans/UPSTREAM` and `container/patches/csf-scale-decode/UPSTREAM`
(`run-fp4local` mounts it over this same image) at `$NEW`,
and say which hooked files changed (`git -C $V diff --stat $OLD $NEW -- <those files>`). For
draft-load-filter, `_remap_mtp_weight_name` and the `"mtp.": None` mapper entry must be untouched.
For spec-live (`SPEC_LIVE=1`, optional; not on the daily-driver path) check `Scheduler.dynamic_sd_lookup`
is still indexed by batch size at the `num_spec_tokens_to_schedule` computation in `scheduler.py`,
that `EngineCoreProc` still dispatches UTILITY requests with `getattr(self, name)` and the API side
still calls `engine_client.engine_core.call_utility_async`, and that `register_api_routers` still
lives in `vllm/entrypoints/launchers/api_server/routers.py`. For sm120-gemm-plans (`SM120_SKINNY=1`,
optional) check `QWEN4_EXP_GEMM_PLANS_BY_CAPABILITY` / `QWEN4_EXP_SM121_GEMM_PLANS` still exist in
`vllm/models/qwen4_exp/nvidia/low_latency_gemm.py`, and whether upstream now ships a `(12, 0)` table
itself (then drop the overlay).

```bash
python3 container/patches/boot-timing/test_boot_timing.py
python3 container/patches/draft-load-filter/test_draft_load_filter.py
python3 container/patches/spec-live/test_spec_live.py
python3 container/patches/sm120-gemm-plans/test_sm120_gemm_plans.py
python3 container/patches/csf-scale-decode/test_csf_scale_decode.py
```

For `container/patches/b12x` (a mounted package, no hooks): check `_B12X_SUBMODULES` in
`vllm/utils/b12x.py` and the `"b12x"` extra pin in `setup.py` at `$NEW`, and whether the image's
own dependencies now ship a top-level `b12x`. flashinfer-python ≥ 0.7.2 does, and then the mount
is shadowed, because its `.pth` appends to `sys.path`.

## 4b. Dispositions (every artifact, at $NEW)

A hook that still lands is not a disposition. One row per artifact, answered at `$NEW`, pasted into
the commit body. Allowed answers: **still needed**, **merged → remove in this commit**,
**retired-kept** (cannot fire anymore, stays on disk with a reason in the body). Two rows carry a
row-scoped state instead: **pending-build** (FA row only) and **checkpoint-driven** (dtype row only).
Evidence by kind — *merged* names the upstream commit or PR; *still needed* names the command you
ran and what it returned (empty log, no such symbol); no PR number exists for an absence, so never
cite one you did not see. No cell empty, no cell hedged: a row without evidence is **STOP** — halt
before step 6 and report the table as far as it goes.

| artifact | the question at `$NEW` | how to answer |
|---|---|---|
| `container/patches/vllm-flash-attn-arch.py` | does FA still force sm_80 PTX for FA2 / build FA3 on an empty arch list? | step 1's diff is the whole answer when the FA `GIT_TAG` is unmoved: unchanged tree ⇒ **still needed (FA <tag> unchanged)**. If it moved, read the fetched `CMakeLists.txt`; the patch asserts its own anchors at build time. Only `./container/archs` proves the emitted archs, so until a build has happened this row is **pending-build** |
| `container/patches/boot-timing` | out-of-tree reporting, can never merge; has upstream shipped a startup waterfall that would make it noise? | `git -C $V log --format='%h %s' $OLD..$NEW -- vllm/entrypoints vllm/renderers vllm/v1/engine vllm/v1/worker vllm/model_executor/warmup`, then `git -C $V grep -inE 'startup_timing\|boot_timing\|profile_report' $NEW`. A hit is a talk, not a merge — `vllm/config/profiler.py` is on-demand profiling, not a report |
| `container/patches/draft-load-filter` | did a loader-side MTP-head skip ship that covers `Qwen4ExpMTP`? | `git -C $V log --format='%h %s' $OLD..$NEW -- vllm/model_executor/model_loader/ vllm/models/qwen4_exp/`; `git -C $V grep -n is_unused_checkpoint_weight $NEW -- vllm/models/qwen4_exp/`. vllm#60152's shard-level hook is the candidate and it is model-declared: the row is **merged** only when `Qwen4ExpMTP` declares it |
| `container/patches/spec-live` (opt path) | did Dynamic SD gain a live-K API that subsumes the utility call? | `git -C $V log --format='%h %s' $OLD..$NEW -- vllm/v1/core/sched vllm/v1/engine/ vllm/config/speculative.py vllm/entrypoints/launchers/api_server` — a live-K knob would surface beside `num_speculative_tokens_per_batch_size` in `speculative.py` |
| `container/patches/sm120-gemm-plans` (opt path) | does upstream ship a `(12,0)` GEMM table? | step 4's check. Yes ⇒ **merged → remove in this commit** |
| `container/patches/csf-scale-decode` (`run-fp4local`) | did upstream learn to decode CSF-encoded NVFP4 scales? | `git -C $V grep -inE 'nvfp4_csf\|csf_exceptions\|byte-window4' $NEW -- vllm`; a hit ⇒ read it before calling it **merged** |
| `container/patches/b12x` (opt path) | does the image now ship `b12x` itself? | step 4's b12x check. Shadowed ⇒ **retired-kept** for this image (the mount still serves older tags); dropping it or making it win is a Plan 06-style decision, not a repin edit |
| ~~`qwen3.8-flash-next/patches/ple-ct-ignore`~~ | **DELETED at the `4d9a8d6` repin** — this row is history, not a task; the dir no longer exists, so do not go looking for it. It was retired upstream at `c32513b` (vllm#59431), an ancestor of every pin since, and a repin only moves forward, so it could never fire again | Keep the regression check, without the overlay: `git -C $V grep -n 'isinstance(quant_config, CompressedTensorsConfig)' $NEW -- vllm/models/qwen4_exp/common/ngram_embedding.py`. If that branch ever disappears, upstream regressed and there is no longer a local workaround — that is a stop-and-ask, not a re-derive |
| launcher: `--hf-overrides` dtype fix | retires with a corrected checkpoint, not with a sha | step 3's cfgcheck shows whether the checkpoint still mislabels ⇒ **checkpoint-driven** |
| launcher: `BLOCK_SIZE=auto`, `PINNED_ROUND_MB` | upstream's own fix retires them | the last two rows of step 3's table give the exact drop condition |
| `muse/patches/*`, `run-legacy`, the other launchers | out of scope — bound to images this repin never moves (see Scope) | — |

A **merged** row makes this commit bigger — see step 6's last bullet — and it is still one commit.

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
- `container/patches/{boot-timing,draft-load-filter,spec-live,sm120-gemm-plans,csf-scale-decode,b12x}/UPSTREAM`: a
  "re-grepped at <sha7>" note carrying the row's 4b answer.
- `qwen3.8-flash-next/run` header: `Last verified against main @ <sha7>`, append the old sha to the
  re-checked list, and update any workaround note that step 3 changed. Edit code only if an
  assumption broke.
- Any 4b row answered **merged → remove in this commit**: enumerate the artifact's touchpoints with
  `git grep -l <name>` and remove every one in this commit — a half-removal is worse than no
  removal. Worked example, the `ple-ct-ignore` removal at `4d9a8d6`: the dir, the `PATCH_PLE` knob
  with its header doc, its gate block and its `-v` mount line in `qwen3.8-flash-next/run`,
  `container/versions.env`, both `README.md` mentions,
  `qwen3.8-flash-next/upstream-watch-prompt.md`, `context/distilled.md`, the comparison line in the
  three sibling `container/patches/*/UPSTREAM`, and this file's own step 3 row and step 7 expect.
  All of those are clean now, so `git grep -l ple-ct-ignore` should return nothing but intentional
  history notes: this file, `context/distilled.md`, the step-1 count note in
  `context/plans/RULES.md`, and the comparison line in `container/patches/boot-timing/UPSTREAM`.
  A hit anywhere else is a leftover.
  `container/patches/*` overlays are mounted by the launcher, not the Dockerfile — the Dockerfile
  holds only the vllm-flash-attn-arch hook. This is the "an assumption broke" of the bullet above,
  broken upstream and evidenced by the table, not an exception to it.
- `qwen3.8-flash-next/upstream-watch-prompt.md` / `README.md`: only if a fact they state changed.

## 7. Verify

```bash
PREFLIGHT_ONLY=1 VLLM_REF=main ./container/build      # syncs src trees to $NEW, checks pins + cubin + torchvision wheel
bash -n qwen3.8-flash-next/run && shellcheck -x qwen3.8-flash-next/run
```

Launcher args, with the final launch stubbed out (the new image isn't built yet, so point
`IMAGE_TAG` at the old one). `DOCKER=docker` is still passed explicitly even though it is now the
default: it pins this step to the fake docker on `PATH` rather than to whatever the default becomes
next, and leaving `DOCKER` unset would let the launcher reach the real daemon. The stub therefore has
to answer `image inspect` itself rather than forward it — forwarding reaches the real socket and
answers with an image that is not the one under test.

```bash
mkdir -p <scratchpad>/stub; cat > <scratchpad>/stub/docker <<'EOF'
#!/bin/bash
[[ $1 == image && $2 == inspect ]] && exit 0          # satisfy require_image
for a in "$@"; do [[ $a == serve ]] && { echo "STUB: launch suppressed"; exit 0; }; done
exit 0
EOF
chmod +x <scratchpad>/stub/docker
DOCKER=docker PATH=<scratchpad>/stub:$PATH IMAGE_TAG=${OLD_IMG#*:} ./qwen3.8-flash-next/run 2>&1 \
  | grep -E '^## |^docker '
```

There is no `PATCH_PLE=0` here any more, and there must not be one added back: that knob and the PLE
gate behind it were deleted at `4d9a8d6`, so the stubbed run goes straight from `require_image` to the
launch line. Before that removal the gate hashed the image's own `ngram_embedding.py` through
`docker run`, the stub gave it no hash, and the launcher exited 1 ("gave no hash") before reaching the
launch — which is why this command used to need the knob. Nothing else in the launcher shells out to
docker before `serve`, so if the run ever dies before printing the `docker ` line again, the cause is
a new pre-launch container call, not this.

The anchor is `^docker ` exactly: Plan 02 removed the optional privilege prefix it used to tolerate.
`vllm_launch` echoes whatever `$DOCKER` expands to, so if the default ever changes this filter silently
matches nothing — a grep that passes having verified nothing. Keep `DOCKER=docker` explicit above, and
if the default moves, move this anchor with it.

Expect: no `ngram_embedding.py` `-v` on the `docker ` line (there is no overlay to mount any more, so
this is a check that the removal stayed removed, not that a skip fired), PLE dtype override
`float8_e4m3fn`, no `--block-size` (the QSA ring capacity divides every 16-aligned size, so
`BLOCK_SIZE=auto` drops it), and `--kv-cache-dtype fp8 --kv-cache-dtype-skip-layers 48
--speculative-config {"method":"mtp","num_speculative_tokens":3} --moe-backend humming
--gpu-memory-utilization 0.95` on `/models/nvidia/Qwen3.8-Flash-Next-NVFP4`, plus the
`sm120_gemm_plans.pth` `-v` mount (`SM120_SKINNY` defaults to 1). Those numbers are the launcher's own
defaults (`SPEC_TOKENS:-3`, `GPU_MEM_UTIL:-0.95`, `SM120_SKINNY:-1`), not constants of this
procedure. This line said "MTP 3" and "0.96" from 2026-09-13 until the `4d9a8d6` repin ran the stub
and caught it. It said MTP 4 with no skinny mount until 2026-10-18, when the defaults moved to K=3
and skinny on. So read them off `qwen3.8-flash-next/run` rather than trusting this file.

## 8. Commit

One commit, `container: pin main snapshot at <sha7>`. Body as in the templates: what moved and
why (with PR numbers), what the dry-run resolved, the launcher and overlay findings, the completed
4b disposition table, a `Checked:` line, `Not verified:` (runtime on the GPU, plus every
**pending-build** row), and `Not built yet: VLLM_REF=main ./container/build.`
No co-author trailer. Never `git push` — the owner does; report the hash and stop.

## 9. Report (short)

- `<old7> -> <new7>` (date, N commits), commit hash.
- Say the words **"I did not run `git push`"** and stop there. Make no claim about the remote in either
  direction — not "not pushed", not "already pushed", not "in sync" — you cannot see it from here
  (`context/pitfalls.md`). This says nothing about the owner's `./container/push` step below.
- A table of what moved (package / was / now / on the Flash-Next path?), and which build stages
  will rebuild.
- Anything that needed a decision or a code change, and anything not verified.
  - Next steps for the owner:
    1. `VLLM_REF=main ./container/build`, then `IMAGE_TAG=cu1321-sm120-main ./container/archs`
       (bare, archs inspects `:latest` — the *release* image, a check that passes having checked
       nothing), then `./container/push`. The `_vllm_fa2_C` line out of archs is the evidence the
       4b FA row could not get pre-build; paste it back and the next repin cites it.
    2. `./qwen3.8-flash-next/run` (a bare launch is the daily driver).

  Until the new image exists, the launcher refuses, because it looks the image up by
  `VLLM_MAIN_SHA`. To run the previous image meanwhile, use `IMAGE_TAG=<old tag> ./qwen3.8-flash-next/run`.
