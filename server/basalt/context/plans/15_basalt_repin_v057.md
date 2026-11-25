# Plan 15 — Repin Basalt to v0.5.7 and rebuild the image

> **Status**: in-progress
> **Created**: 2026-11-25
> **Depends on**: Plan 13 T1 (the pin `ff0b978` this starts from); Plan 12 T1 (`patches/0001`, which must fit the new pin); Plan 14 T1–T3 (`./run-ink` and its gate checks); Plan 01 (the image recipe, `./build`, the gate)
> **Goal**: `versions.env` pins Basalt `400b411` (v0.5.7), the image `iphands/basalt-blackwell:cu1343-sm120-400b411` is built from it with the buffered-io patch, the full gate passes against that image, and each launcher the owner serves from boots its pack on it with requests answered.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Move the Basalt pin from `ff0b978` (v0.5.5) to the v0.5.7 tag commit, two releases on,
rebuild with the carried patch, and prove the three launchers still boot.

**Deliverables**:
1. `versions.env` at `400b411c58031d064812ea21a259b2381acdaa3e`; image `cu1343-sm120-400b411` built
   by the owner, labelled with the patch; gate 1–6 (with 5b) pass on it; `README.md` names the new
   pin and tag.
2. Owner boots — `./run-ink`, `./run`, `./run-hy4` — each with requests answered, recorded.
3. Changed facts harvested into `context/`.

**Estimated effort**: Small (2 h of agent work; the owner's build and boots).

---

## Scope

**In**: `versions.env`, `README.md`, plan bookkeeping, `context/` harvest; `Dockerfile` and
`Dockerfile.dockerignore` **only if** the build finds something the recipe must follow (the pre-read
found nothing); the gate snippets in `RULES.md` only if `serve.config` or `serve.engine.vision` moved
a name they call (the pre-read found all five names in place; `model_plan` gained two optional
arguments the gate does not pass).
**Out**: the open rows of Plans 08, 10, 11, 12, 13 and 14; widening `./run-ink`'s `KV` knob to
`int8`, which the engine takes from v0.5.6 on (a launcher's accepted values and the gate assertion
that goes with them); the new default Inkling pack `Inkling-2-Mini-NVFP4-MX8.basalt` (a download,
the owner's) and `./run-ink`'s default pack; a second card or any `gpu-mode`; `--api-key-file`;
narrowing `BASALT_MODELS`; re-targeting Plan 09's audit; the CUDA base pins; `./publish` runs;
`patches/0001` itself — it fits unchanged, and editing even its prose header would change the hash
the image label carries. If the patch stops fitting or a fix needs to cross this line, **stop** and
record the blocker in the tracker.

---

## Context

The owner asked (2026-11-25): "0.5.5 has been working well lets repin to 0.5.7". A pin change is a
plan (`RULES.md` § *When Does a Change Need a Plan?*): upstream keeps no flag shims, and v0.5.7 is
a release its own notes call breaking, so the plan says what the two sets of release notes changed
and re-boots. Precedent and method: `context/plans/13_basalt_repin_v055.md`.

### Pre-Identified Bug/Issue

Not a bug: upstream released past the pin twice, both on 2026-11-23 (the filter keeps `main` and
the 0.5 tags).

```bash
$ git ls-remote --tags --heads https://github.com/jesdga95/basalt.git 2>&1 | grep -E 'v0\.5|refs/heads/main'
f71a58751456b91cf86747f3498893d3ab8f1b13	refs/heads/main
fa7b40ab76032a23f57c363cd56683e4ce881354	refs/tags/v0.5.0
ad274aa8dc243f0fdcc1ad382f2e31c4591b4552	refs/tags/v0.5.0^{}
3cfb5ef4800b17d20a0813bcbcea95a5622ab230	refs/tags/v0.5.5
ff0b978d2ee9ff195d113b6feb80961b2d2f99ef	refs/tags/v0.5.5^{}
1a7446fc863ccbfff298ad21548f7c1731ce6b3c	refs/tags/v0.5.6
1e205ae87cc8e3c04e96db40fd505a75f44685ec	refs/tags/v0.5.6^{}
cea3989c11e7314acc8d0c1c4393c0a7b47dabe6	refs/tags/v0.5.7
400b411c58031d064812ea21a259b2381acdaa3e	refs/tags/v0.5.7^{}
```

Today's pin and checkout:

```bash
$ grep '^BASALT_SHA' versions.env; GIT_NO_LAZY_FETCH=1 git -C vendor/basalt rev-parse HEAD
BASALT_SHA=ff0b978d2ee9ff195d113b6feb80961b2d2f99ef
ff0b978d2ee9ff195d113b6feb80961b2d2f99ef
```

### Why repin by tag commit, build-then-commit

The same two choices as Plans 08, 10 and 13, for the same reasons.

- **Tag commit over `main`'s tip.** The tag has release notes with a checks section; `main` is
  already past it (`f71a587`). Rejected: `main` on the day.
- **Straight to v0.5.7, not through v0.5.6.** The owner named 0.5.7; v0.5.6 (`1e205ae`) is an
  ancestor of it, so its changes are in the build either way and its notes are read here. Rejected:
  an intermediate image nobody asked for.
- **One task for edit + build + gate (T1).** Gate steps 5, 5b and 6 fail with no image for
  `BASALT_SHA`, and the gate reads the working tree, so no commit can pass between the pin edit and
  the owner's build. Rejected: committing the pin first — that commit would be made on a red gate.

### Key Facts

Read on 2026-11-25 from the objects of `v0.5.7` in `vendor/basalt`, fetched by the agent with
`git -C vendor/basalt fetch origin` (the fetch `./build` itself makes when the pin is not local; the
checkout stayed at `ff0b978`, clean). These are the bytes, not a paraphrase. `OLD` is `ff0b978`,
`NEW` is `400b411`.

| Fact | Value | How confirmed |
|---|---|---|
| New pin | `400b411c58031d064812ea21a259b2381acdaa3e` = `refs/tags/v0.5.7^{}` (annotated tag `cea3989c`), "0.5.7: changelog and index row (gpu, gpu-mode and gpu-share replace every model's multi-GPU logic; …); the version is 0.5.7", committed 2026-11-23 | `git ls-remote`, above; `git log -1 --format='%H %cs %s' NEW` |
| Distance | 251 commits ahead, 0 behind, merge base `ff0b978` — a strict descendant. v0.5.6 (`1e205ae`) is 134 of them, v0.5.7 the other 117 | `git rev-list --count OLD..NEW`, the reverse, `git merge-base OLD NEW`; the same counts through `1e205ae` |
| v0.5.7's break | "A breaking release for every config that names more than one card": `gpu: auto`, a `gpu` list without a `gpu-mode`, `--diag-split-at` and `--no-peer-prefill` are errors. "`"gpu": "0"` — unchanged, and silent: naming a card is a choice" | `git show NEW:docs/changelog/0.5.7.md`, § *Upgrading* |
| What our launchers pass | `--gpu 0` in all three (`run:133`, `run-hy4:141`, `run-ink:151`), on a container that sees one card (`--gpus device=0`); none passes `--gpu-mode`, `--gpu-share`, `--diag-split-at`, `--no-peer-prefill`, `--batch-groups` or `--exp` | `grep -nE 'gpu\|split-at\|peer-prefill\|batch-groups\|concur5\|--exp' run run-hy4 run-ink preload lib/common.sh` |
| v0.5.6's removals | `--exp concur5_digest=1` and Flash-Next's `--batch-groups` pipeline. "No flag or config key is renamed. The `.basalt` format stays version 2 and the config version stays 2. Packs built for 0.5.5 … load as they are." | `git show NEW:docs/changelog/0.5.6.md`, § *Upgrading*; gate steps 5 and 5b on the new image decide |
| Image recipe inputs | `Dockerfile`, `.dockerignore`, `requirements.txt`, `tools/model_file.py`, `LICENSE`, `THIRD_PARTY_LICENSES`, `cmake/models.cmake`, `docs/building.md`: the same blob at both pins; nothing under `data/` changed | `git rev-parse OLD:<f>` = `git rev-parse NEW:<f>` for each; `git diff --no-renames --name-only OLD NEW -- data` is empty |
| The patch's three files | `include/basalt/platform/file.hpp`, `src/platform/direct_file.cpp`, `src/platform/file.cpp`: the same blob at both pins, so `patches/0001` fits byte for byte. No file under `src/platform` changed at all | same comparison; `git diff --no-renames --name-only OLD NEW -- src/platform` is empty; `./build`'s `git apply --check` at T1 step 2 is the check that counts |
| The patch's fallbacks | every reader that answers the patch's `-1` is the same blob at both pins: `src/core/loader.cpp`, `src/vision/mmproj.cpp`, `src/core/model_file.cpp`, `conversation_store.cpp`, `file_reads.cpp`, `expert_source.cpp`, `src/ngram/ple_reader.cpp` | `git rev-parse` pairs. A sweep of every `open_unbuffered` and `DirectFile` user in the checked-out tree — the new `src/model/` and `src/models/inkling2/cpu_experts.cpp` included — is T1 step 2 |
| The patch's route into the engine | the server still hands the engine its whole environment (`env = dict(os.environ)`, `serve/engine/vision.py:561` at `NEW`, was `:533`) | `git show NEW:serve/engine/vision.py \| grep -n os.environ` |
| Gate hooks | `serve/config.py` at `NEW` defines `engine_flags_of` :32, `server_options` :42, `parse_settings` :265, `build_parser` :555; `serve/engine/vision.py` defines `model_plan` :48, now `model_plan(engine_bin, path, value, words, mode=None, share=None)` — the gate's four-argument call still fits | `git show NEW:<file> \| grep -n '^def '`; `git diff OLD NEW -- serve/engine/vision.py`; `RULES.md:587` |
| `serve/config.py` changes | new server options `--gpu-mode`, `--gpu-share`, `--totals-file`, `--totals-reset`, `--cost-in`, `--cost-out`, `--cost-cache-write`, `--cost-cache-hit`; `gpu` and `gpu-share` may be lists in a config file. With no config file and no `--totals-file` the new persistent totals are kept nowhere ("No config and no --totals-file: nowhere to keep them", `serve/totals.py:22`), so nothing new is written in the container | `git diff OLD NEW -- serve/config.py`; `git grep` in `NEW:serve/totals.py`. The launchers pass no config file |
| Build inputs that moved | `CMakeLists.txt`: `project` 0.5.7, two new static libraries (`prefill_mxfp8`, `prefill_q8`, both `CUDA_ARCHITECTURES 120a-real` like their neighbours), new sources under `src/model/` and `src/models/`, new tests; no new `find_package` (`CUDAToolkit`, `Threads`, `Python3` as before), `cmake_minimum_required` still 3.24 | `git diff OLD NEW -- CMakeLists.txt`; `git show NEW:CMakeLists.txt \| grep -n` |
| New server modules | `serve/prometheus.py`, `serve/totals.py` (gate step 6 imports every module in the image) | `git diff --no-renames --name-status OLD NEW -- serve` |
| Size of the change | 516 files: `docs` 187, `src/models` 111 (`qwen4exp` 61, `inkling2` 30, `deepseek41` 13, `hy4flash` 6, `qwen35` 1), `include` 53, `tests` 33, `src/program` 25, `serve` 34 (18 of them tests), `tools` 18, `src/kernels` 15, `src/model` 6, `src/dense` 6, `src/core` 5, `src/prefill` 5, `src/vision` 4 | `git diff --no-renames --name-only OLD NEW`, counted by directory |
| Flash-Next: slots | "Both batch their slots through the shared driver. Flash-Next's own rounds are gone and its speed matches them at 1, 2, 4 and 8 slots". `./run` passes `--parallel 3 --slot-full-context`. The rule that `--kv int8` is the format that runs with batch slots is unchanged (`tests/models/qwen4exp/cli/kv_batch_test.cpp`, the same blob) | changelog 0.5.6 § *Qwen3.8-Flash-Next and Qwen3.8-27B* — upstream's claim; `git rev-parse` pair. T2's `./run` boot is the check |
| Hy4-flash: output changes by design | the Engram gate's epsilon fix (#556): "mean KLD 0.782 to 0.209, top-1 76.77% to 86.14%; the stray answers in JSON and code went from 3 to 4 of 48 to 0 of 48". Also "elastic KV growth no longer refuses spuriously" (#549). Known issue, both releases: an illegal memory access on prompts of about 600K tokens and more — above `./run-hy4`'s 362144 | changelogs 0.5.6 and 0.5.7 — upstream's claims, not re-measured here |
| Inkling 2 Mini: speed | prompts "21 to 25% faster" (#555); "drafted conversation decodes 1 to 5% slower than 0.5.5 (windows of 1.5 tokens against 1.9, 60% of drafts kept against 66%)"; v0.5.7: one card "within noise" of 0.5.6 | changelog 0.5.6 § *Highlights*, § *Checked*; 0.5.7 § *Checks of this release* — upstream's claims. Our v0.5.5 session to compare with: Plan 14 tracker, Evidence row 9 |
| Inkling 2 Mini: `--kv` | the engine now takes `int8` beside `fp16` and `fp8` (#552; `src/models/inkling2/adapter.cpp:63` at `NEW`). `./run-ink` passes `fp16` and refuses `KV=int8` itself; gate step 4 asserts that refusal, which is the launcher's and stays true | `git show NEW:src/models/inkling2/adapter.cpp \| grep -n int8`; `run-ink` |
| Inkling 2 Mini: packs | upstream's default download is now `Inkling-2-Mini-NVFP4-MX8.basalt` (19.6 GB, "needs 0.5.6 or later"); "`Inkling-2-Mini-NVFP4-DQ8.basalt` (19.7 GB), the 0.5.5 pack … Both load." Ours is `Inkling-2-Mini-NVFP4-q8.basalt`, 19,734,300,551 bytes, downloaded 2026-11-21 under v0.5.5 — **that it is the file upstream now calls DQ8 is an inference from the size and the date, not checked** | `git show NEW:docs/models/inkling2.md` § *Pack*; Plan 14 tracker Evidence row 7. T2's `./run-ink` boot decides whether it loads |
| Server behaviour that changed | "the default model answers a request with no `model` field" (#337); `/metrics` speaks Prometheus text when asked; "a top-level `enable_thinking: false` turns thinking off"; "a full batch gives each slot one row next round" | changelog 0.5.7 § *Other changes* — upstream's claims |
| The tag | annotated, tagger Jesus Gulfo, no signature block; `v0.5.0`, `v0.5.5`, `v0.5.6` and `v0.5.7` tag objects are in the clone after this fetch | `git -C vendor/basalt cat-file -p v0.5.7`; `git -C vendor/basalt tag -l 'v0.5*'` |
| Build cost | v0.5.5's five models took 327.9 s at `JOBS=16`, 449 ninja targets, image 3.5 GB | Plan 13 tracker, Evidence row 7; the new figure is recorded in T1 |
| Gate baseline, old pin | passes 1–6 with 5b on `iphands/basalt-blackwell:cu1343-sm120` (`a3441f94bdf1`) — tracker Evidence row 1 | run 2026-11-25 before the plan commit |
| Agent shell can run gate 3–6 | `docker` is in `id -nG` | `id -nG`, 2026-11-25 |
| Rollback image | `cu1343-sm120-ff0b978` (`a3441f94bdf1`, labels `ai.basalt.ref=ff0b978…`, `ai.basalt.patches=0001-buffered-io-switch.patch:a3553ec6bb8c`) stays on disk; `./build` tags only `…-400b411`. The moving tags `cu1343-sm120` and `latest` list `a3441f94bdf1` today and move when the owner's `./publish` moves them | `docker images iphands/basalt-blackwell`, `docker image inspect`, 2026-11-25; `build`, `publish` |
| A server is up | container `basalt`, three days, serving Inkling 2 Mini on `a3441f94bdf1`; a build does not touch it | `docker ps`, `docker inspect`, 2026-11-25 |

### Rejected Claims

- **"v0.5.7 is breaking, so the launchers must change."** The break is for configs that name more
  than one card or say `gpu: auto`. The launchers name card 0 and the container is given only that
  card. Do NOT add `--gpu-mode`: with one card it is an error ("`gpu-mode` is required with two or
  more cards and an error with one").
- **"The recipe needs a change because upstream added libraries."** Not on this evidence: the new
  libraries are static targets inside the same build, and upstream's own `Dockerfile` — whose runtime
  tree ours mirrors — is the same blob at both pins.
- **"The patch needs re-fitting at every repin."** Only when one of its three files moves. None did;
  do NOT regenerate or re-header the patch — its hash is what the image label and Plan 12 name.
- **"`./run-ink` must accept `KV=int8` now."** The engine accepting a value does not make the
  launcher wrong for refusing it; widening a knob is a launcher change with its own gate assertion.
  Follow-ups row.
- **"`vendor/basalt` can be probed with plain git."** The clone is `blob:none`; an unguarded probe
  lazy-fetches. `GIT_NO_LAZY_FETCH=1` on every probe that is not a deliberate read of a named file.

---

## Step-by-Step Tasks

### T1: Repin to `400b411`, preflight, owner build, gate on the new image

**File**: `versions.env`, `README.md`; `Dockerfile`, `Dockerfile.dockerignore` and the `RULES.md` gate snippets only if the build or the gate finds a reason

**What to do**, in this order — nothing is committed until step 5:
1. Agent: edit `versions.env` (below). The file stays dirty until step 5.
2. Agent: `PREFLIGHT_ONLY=1 ./build` — checks `vendor/basalt` out at the new SHA, runs
   `git apply --check` on `patches/0001`, resolves both CUDA digests, lints. Wanted:
   `== patch fits: 0001-buffered-io-switch.patch`, last line `preflight ok`. Then, in the checked-out
   tree, `grep -rn 'open_unbuffered\|DirectFile' src include` against the same grep at `ff0b978`: a new
   caller with no cached path when `open_unbuffered` answers -1 is a stop. Then gate steps 1–4 on
   the dirty tree (step 4 prints `image: stub:none` until the image exists). Hand off with
   `dirty: versions.env (mine, in progress)`.
3. **Owner**: `./build` → `iphands/basalt-blackwell:cu1343-sm120-400b411`. Log:
   `captures/build_<date>_400b411.log`.
4. Agent: the image id and labels (`ai.basalt.ref`, `ai.basalt.patches`); the build log's
   `== applying 0001-buffered-io-switch.patch` line and the compile step's time and target count;
   gate 1–6 with 5b, steps 5, 5b and 6 against the new image; step 5's negative control.
5. Agent: `README.md` (Status, Pins, tag names, the rollback line), tracker row, commit.

**Before**:
```bash
# Basalt: v0.5.5, the tag's commit (owner decision 2026-11-21, Plan 13). Was ad274aa8 (v0.5.0,
# Plan 10); before that d3b75742 (v0.4.1, Plan 08) and dc047a77 (main on 2026-11-04, the v0.2.1
# engine).
BASALT_REPO=https://github.com/jesdga95/basalt.git
BASALT_SHA=ff0b978d2ee9ff195d113b6feb80961b2d2f99ef
```

**After**:
```bash
# Basalt: v0.5.7, the tag's commit (owner decision 2026-11-25, Plan 15). Was ff0b978d (v0.5.5,
# Plan 13); before that ad274aa8 (v0.5.0, Plan 10), d3b75742 (v0.4.1, Plan 08) and dc047a77 (main
# on 2026-11-04, the v0.2.1 engine).
BASALT_REPO=https://github.com/jesdga95/basalt.git
BASALT_SHA=400b411c58031d064812ea21a259b2381acdaa3e
```

**Verify**:
```bash
PREFLIGHT_ONLY=1 ./build 2>&1 | grep -E 'patch fits|preflight ok'                # → both lines
img=$(bash -c 'source lib/common.sh; load_pins; image_for_pin'); echo "$img"     # → an iphands/basalt-blackwell tag
docker image inspect -f '{{index .Config.Labels "ai.basalt.ref"}}' "$img"         # → 400b411c58031d064812ea21a259b2381acdaa3e
docker image inspect -f '{{index .Config.Labels "ai.basalt.patches"}}' "$img"     # → 0001-buffered-io-switch.patch:a3553ec6bb8c
EXTRA_ARGS=--no-such-flag <gate step 5>                                         # → FAIL(flags) --no-such-flag
# + the full project gate 1–6 with 5b
```

**Expected observation**:
- **Confirms if**: the build log shows `== applying 0001-buffered-io-switch.patch` with no fuzz or
  reject and the compile finishes; step 5 prints `flags ok (…)`, step 5b four `pack flags ok` lines
  and step 6 `imports ok (N serve modules)`, all from the image labelled with the new SHA.
- **Refutes if**: the patch does not apply; the build stops on a missing tool or path; step 5 or 5b
  names a flag a launcher passes (`--gpu 0` under the new parser is the one to watch); step 6 names
  a module that needs a file our runtime stage does not copy (`serve/prometheus.py` and
  `serve/totals.py` are new).
- **Within noise if**: the flag and module counts differ from the v0.5.5 image's (159 engine flags
  known, 44 serve modules), or the build is slower and the image larger — only `FAIL` lines matter.
  Record the figures.

If the build or gate fails in a way this plan's scope cannot fix: put the old `BASALT_SHA` line and
its comment back by hand (nothing was committed, so no `git restore`), re-run `PREFLIGHT_ONLY=1
./build` so `vendor/basalt` returns to `ff0b978`, and record the blocker. The old image stays on disk.

**Commit**: `[basalt][P15][T1][pins] repin Basalt to 400b411 (v0.5.7) and rebuild`

### T2: Owner boots — `./run-ink`, `./run`, `./run-hy4` — with requests answered

**File**: tracker Evidence (read by the agent from the running container, as Plans 13 and 14 did)

**What to do**: **Owner**: `docker stop basalt`, then the launcher wanted — `./run-ink` first, as the
one in daily use — and requests from the owner's own client as `cosmo-6000`. When the owner says a
launcher is up, the agent reads `docker inspect basalt`, `docker logs basalt` and the engine log
under `storage/cache/basalt/` and records the image id, the words, the start block and the request
lines. The agent sends no request of its own to a one-slot server. `./run` and `./run-hy4` (after
`./preload` if the pack is on NFS) the same way, whenever the owner next serves from them. All
boots run under `BUFFERED_IO=1`, the launchers' default.

**Verify**: the logs are read, not inferred (`RULES.md` § *Project gate* → Look at it).

**Expected observation**:
- **Confirms if**: the container's image id is the one labelled `400b411…` and its words name the
  pack asked for; the engine accepts each launcher's flags; requests finish with `end=stop`. For
  `./run`: `slots 3 × 362144` accepted under the new batch driver. For `./run-ink`: the 0.5.5 pack
  loads, every expert on the card.
- **Refutes if**: the engine refuses a flag value (`--gpu 0` included), refuses the 0.5.5 Inkling
  pack, caps the context below 362144, or stalls on a prompt — then the setting is a new owner
  decision, not a silent edit.
- **Within noise if**: load time, rates and the VRAM/RAM split differ from v0.5.5's (Plan 13
  tracker Evidence row 11 for Flash-Next, Plan 14 tracker Evidence row 9 for Inkling). Upstream
  expects Inkling prompts faster and its drafted decode 1 to 5% slower. Record them; one session of
  the owner's traffic is not a bench, so do not call a difference a regression or a gain.

A launcher the owner has not served from when the plan closes is recorded `skipped` in its row,
with that reason, by the owner's decision.

**Commit**: `[basalt][P15][T2][boot] record the owner boots on v0.5.7`

### T3: Harvest and close

**File**: `context/distilled.md`, `context/high_level.md`, `context/pitfalls.md`, this plan + tracker, `SERIES.md`

**What to do**: dated superseding notes in `distilled.md` for every fact T1/T2 changed (the built
image, build time and size, flag and module counts, boot numbers, the `gpu` settings), each tagged
with how it is known; `high_level.md`: the pin's rationale; `pitfalls.md`: anything that took more
than one attempt. Re-read each file after writing. Carry the tracker's Follow-ups to where they
land. Then `git mv` this plan and its tracker to `completed/` and mark SERIES done.

**Verify**: every finding the tracker mentions is found by `grep` in the file it names; the full gate.

**Commit**: `[basalt][P15][close] Basalt repin to v0.5.7: <outcome>`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `versions.env` | `BASALT_SHA` and its comment | P0 |
| `README.md` | Status, Pins, tag names, the rollback line | P1 |
| `Dockerfile`, `Dockerfile.dockerignore` | only if the build finds a reason; none expected | P1 |
| `context/plans/RULES.md` | gate snippets, only if a hook name moved; none did in the pre-read | P1 |
| `context/plans/SERIES.md` | bookkeeping | P1 |
| `context/distilled.md`, `context/high_level.md` | harvest | P1 |

---

## Open Questions / Risks

1. **Risk: the working tree is dirty across the owner's build (T1 steps 1–4), in a worktree with two
   writers (Project Rule P1).** *Mitigation*: only `versions.env` is dirty (and `README.md` at step
   5), every handoff carries the `dirty:` line, and no `context/` or plan file is left uncommitted
   while the owner works.
2. **Risk: between the pin edit and the build, every launcher refuses** ("no … image is labelled
   `ai.basalt.ref`=…; run `./build` first"). *Mitigation*: the server that is up keeps running; a
   restart meanwhile is `IMAGE_OVERRIDE=iphands/basalt-blackwell:cu1343-sm120-ff0b978 ./run-ink`.
3. **Risk: a model behaves differently at boot or under long prompts.** Two releases at once: 61
   files of Flash-Next's folder changed and its slot batching was replaced; Hy4-flash's output
   changes by design; every model's card handling moved into shared code. *Mitigation*: T2's boots;
   the v0.5.5 image stays on disk as the rollback, three days of the owner's use behind it.
4. **Risk: the 0.5.5 Inkling pack under a newer engine.** Upstream says packs built for 0.5.5 load
   as they are; ours carries a name upstream's guide no longer uses. *Mitigation*: gate step 5b runs
   the new engine's parser on the pack at T1; T2's `./run-ink` boot loads it. The MX8 pack is the
   owner's download if wanted (Follow-ups).
5. **Risk: the image is published before it has booted.** The owner repins in order to build and
   publish, and gate 1–6 is model-free. *Mitigation*: the immutable tag names the commit; the moving
   tags are the owner's to move; T2 records the first boot whenever it happens.
6. **Risk: six plans are open underneath this one** (08, 10, 11, 12, 13, 14), four of them waiting
   on boots of an image a plain launcher no longer selects once the pin moves. *Mitigation*: their
   files are not edited here; the tracker's Follow-ups name each decision.
7. **Risk: the built pin is unaudited, and Plan 09's row names `d3b7574`.** *Mitigation*: Follow-ups
   row; nothing in `context/audit/` claims to cover any pin since `dc047a7`.
8. **Question (deferred — goes to the audit plan, as Plan 13 Open Question 7 did): does the pin's
   tree equal GitHub's tree object for `v0.5.7`?** The tag object is in the clone and carries no
   signature (Key Facts). *How we'll settle it*: the audit's first tasks.

---

## Verification Checklist

- [ ] T1: `image_for_pin` prints a tag whose `ai.basalt.ref` label is `400b411c58031d064812ea21a259b2381acdaa3e` and whose `ai.basalt.patches` label names `0001-buffered-io-switch.patch:a3553ec6bb8c`; gate steps 5, 5b and 6 pass on it; step 5's negative control fires
- [ ] T2: each boot's container names the `400b411` image and the pack; finished requests from each launcher are recorded in the tracker, or the launcher's row says `skipped` and why
- [ ] T3: each harvested fact is in the file the tracker names, re-read after writing
- [ ] All: the project gate (Rule A) passes on the final commit
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
