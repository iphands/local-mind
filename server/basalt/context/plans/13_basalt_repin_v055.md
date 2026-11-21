# Plan 13 — Repin Basalt to v0.5.5 and rebuild the image

> **Status**: pending
> **Created**: 2026-11-21
> **Depends on**: Plan 10 T1 (the pin `ad274aa` this starts from); Plan 12 T1 (`patches/0001`, which must fit the new pin); Plan 01 (the image recipe, `./build`, the gate)
> **Goal**: `versions.env` pins Basalt `ff0b978` (v0.5.5), the image `iphands/basalt-blackwell:cu1343-sm120-ff0b978` is built from it with the buffered-io patch, the full gate passes against that image, and each launcher boots its pack on it with a real reply.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Move the Basalt pin from `ad274aa` (v0.5.0) to the v0.5.5 tag commit, rebuild with the
carried patch, and prove both launchers still boot.

**Deliverables**:
1. `versions.env` at `ff0b978d2ee9ff195d113b6feb80961b2d2f99ef`; image `cu1343-sm120-ff0b978` built by
   the owner, labelled with the patch; gate 1–6 (with 5b) pass on it; `README.md` names the new pin
   and tag.
2. Owner boots, `./run` and `./run-hy4`, each with a real `/v1/chat/completions` reply, recorded.
3. Changed facts harvested into `context/`.

**Estimated effort**: Small (2 h of agent work; the owner's build and two boots).

---

## Scope

**In**: `versions.env`, `README.md`, plan bookkeeping, `context/` harvest; `Dockerfile` and
`Dockerfile.dockerignore` **only if** the build finds something the recipe must follow (the pre-read
found nothing); the gate snippets in `RULES.md` only if `serve.config` or `serve.engine.vision` moved
a name they call (the pre-read found all five names in place).
**Out**: the open rows of Plans 08, 10, 11 and 12 — in particular Plan 12 T3's gate step 6b and its
record of the patched build; re-targeting Plan 09's audit; a launcher for Inkling 2 Mini, the model
this release adds (a new launcher profile is a plan); narrowing `BASALT_MODELS`; `--api-key-file`
in the launchers; the CUDA base pins; `./publish` runs; `patches/0001` itself — it fits unchanged,
and editing even its prose header would change the hash the image label carries. If the patch stops
fitting or a fix needs to cross this line, **stop** and record the blocker in the tracker.

---

## Context

The owner asked (2026-11-21): "0.5.5 just landed repin to this version so I can build and publish".
A pin change is a plan (`RULES.md` § *When Does a Change Need a Plan?*): upstream keeps no flag
shims, so the plan says what the release notes changed and re-boots. Precedent and method:
`context/plans/10_basalt_repin_v050.md`. New since that repin: the image carries a local patch
(Plan 12), so a repin must also show the patch still fits.

### Pre-Identified Bug/Issue

Not a bug: upstream released past the pin (2026-11-21; the filter keeps `main` and the 0.5 tags).

```bash
$ git ls-remote --tags --heads https://github.com/jesdga95/basalt.git 2>&1 | grep -E 'v0\.5|refs/heads/main'
97e2e7110578aeb7f8708dab176b910589b35047	refs/heads/main
fa7b40ab76032a23f57c363cd56683e4ce881354	refs/tags/v0.5.0
ad274aa8dc243f0fdcc1ad382f2e31c4591b4552	refs/tags/v0.5.0^{}
3cfb5ef4800b17d20a0813bcbcea95a5622ab230	refs/tags/v0.5.5
ff0b978d2ee9ff195d113b6feb80961b2d2f99ef	refs/tags/v0.5.5^{}
```

Today's pin and checkout:

```bash
$ grep '^BASALT_SHA' versions.env; GIT_NO_LAZY_FETCH=1 git -C vendor/basalt rev-parse HEAD
BASALT_SHA=ad274aa8dc243f0fdcc1ad382f2e31c4591b4552
ad274aa8dc243f0fdcc1ad382f2e31c4591b4552
```

### Why repin by tag commit, build-then-commit

The same two choices as Plans 08 and 10, for the same reasons.

- **Tag commit over `main`'s tip.** The tag has release notes with a Checked section; `main` is
  already past it (`97e2e71`). Rejected: `main` on the day.
- **One task for edit + build + gate (T1).** Gate steps 5, 5b and 6 fail with no image for
  `BASALT_SHA`, and the gate reads the working tree, so no commit can pass between the pin edit and
  the owner's build. Rejected: committing the pin first — that commit would be made on a red gate.

### Key Facts

Read on 2026-11-21 from the objects of `v0.5.5` in `vendor/basalt`, fetched by the agent with
`git -C vendor/basalt fetch origin` (the fetch `./build` itself makes when the pin is not local; the
checkout stayed at `ad274aa`, clean). These are the bytes, not a paraphrase. `OLD` is `ad274aa`,
`NEW` is `ff0b978`.

| Fact | Value | How confirmed |
|---|---|---|
| New pin | `ff0b978d2ee9ff195d113b6feb80961b2d2f99ef` = `refs/tags/v0.5.5^{}` (annotated tag `3cfb5ef4`), "Merge Inkling 2 Mini support (Basalt 0.5.5)", committed 2026-11-21 | `git ls-remote`, above; `git log -1 --format='%H %cs %s' NEW` |
| Distance | 177 commits ahead, 0 behind, merge base `ad274aa` — a strict descendant | `git rev-list --count OLD..NEW`, the reverse, `git merge-base OLD NEW` |
| Headline | a fifth model, Thinking Machines Lab's Inkling 2 Mini (`inkling2`, a 19.7 GB pack) | `git show NEW:docs/changelog/0.5.5.md` |
| Flags and formats | "No flag or config key is removed or renamed. The `.basalt` format stays version 2 and the config version stays 2." | same file, § *Upgrading*; gate steps 5 and 5b on the new image decide |
| Image recipe inputs | `Dockerfile`, `.dockerignore`, `requirements.txt`, `tools/model_file.py`, `LICENSE`, `THIRD_PARTY_LICENSES`: the same blob at both pins | `git rev-parse OLD:<f>` = `git rev-parse NEW:<f>` for each |
| The patch's three files | `include/basalt/platform/file.hpp`, `src/platform/direct_file.cpp`, `src/platform/file.cpp`: the same blob at both pins, so `patches/0001` fits byte for byte | same comparison; `./build`'s `git apply --check` at T1 step 2 is the check that counts |
| The patch's fallbacks | every reader that answers the patch's `-1` still has its cached path: `src/core/loader.cpp` changed (170 lines) and still opens a `buffered` descriptor beside the direct one (`:398-405` at `NEW`) and reads through it when the direct one is absent (`read_staged` :439, `read_direct` :459); `src/vision/mmproj.cpp` changed and still falls through when `open_unbuffered` answers below 0 (`:107-110`); `src/core/model_file.cpp`, `conversation_store.cpp`, `file_reads.cpp`, `expert_source.cpp` and `src/ngram/ple_reader.cpp` are the same blobs | `git show NEW:<file> \| grep -n`, `git rev-parse` pairs. A sweep of every `open_unbuffered` and `DirectFile` user in the checked-out tree, new model included, is T1 step 2 |
| The patch's route into the engine | the server still hands the engine its whole environment (`env = dict(os.environ)`, `serve/engine/vision.py:533` at `NEW`, was `:525`) | `git show NEW:serve/engine/vision.py \| grep -n os.environ` |
| Gate hooks | `serve/config.py` at `NEW` defines `engine_flags_of` :32, `server_options` :42, `parse_settings` :261, `build_parser` :535; `serve/engine/vision.py` defines `model_plan` :48 | `git show NEW:<file> \| grep -n '^def '` |
| `serve/config.py` changes | a new server option `--api-key-file`; a config-file engine entry may be a list only for a flag that takes several values (`ENGINE_VALUE_FORMS`); a `slots_text` helper for the start log | `git diff OLD NEW -- serve/config.py`. The launchers pass words on the command line and no list |
| Build inputs that moved | `CMakeLists.txt` (`project` 0.5.5, the `inkling2` targets, two new shared kernels, no new `find_package`, `cmake_minimum_required` still 3.24), `cmake/models.cmake` (`BASALT_MODELS` defaults to the word `all`, which now means five models), five new `data/` files (`data/profiles/inkling2-24x128.prof`, four `data/bench/inkling2/*.ids`) | `git diff OLD NEW -- CMakeLists.txt cmake/models.cmake`; `git diff --no-renames --name-status OLD NEW -- data` |
| Size of the change | 277 files; `src/models`: 39 new under `inkling2`, 22 touched under `qwen4exp`, 16 under `hy4flash`, 5 under `deepseek41`; also `src/core` 3, `src/kernels` 8, `src/platform` 2 (`cpu.cpp`, `memory.cpp`), `src/prefill` 2, `serve/` 6 plus `serve/engine` 4 | `git diff --no-renames --name-only OLD NEW`, counted by directory |
| Flash-Next's KV with slots | `--kv int8` is the format that runs with batch slots; `fp16`, `q4_0` and `k8v4` are refused above one slot. `./run` passes `--kv int8 --parallel 3` | `git show NEW:tests/models/qwen4exp/cli/kv_batch_test.cpp` (the rule now sits in a header with a test) |
| Flash-Next's routing flags | `--route-tail-skip` and `--route-prior` moved to a parser shared by every MoE model (`model::parse_route_flag`); no launcher passes either | `git diff OLD NEW -- src/models/qwen4exp/flags.cpp`; `grep -n route run run-hy4` prints nothing |
| Output | upstream: Flash-Next greedy output identical to `main` with its tier pinned, after a short and a 16,671-token prompt; Hy4-flash identical with a pinned, frozen tier — **against `main` at `a36524024`, not against v0.5.0** | changelog § *Checked* — upstream's claim, not re-measured here |
| The tag | annotated, tagger Jesus Gulfo, no signature block; both `v0.5.0` and `v0.5.5` tag objects are in the clone after this fetch | `git -C vendor/basalt cat-file -p v0.5.5`; `git -C vendor/basalt tag -l 'v0.5*'` |
| Build cost | five models now; v0.5.0's four took 307.9 s at `JOBS=16`, 427 ninja targets, image 3.49 GB | Plan 10 tracker, Evidence row 4; the new figure is recorded in T1 |
| Gate baseline, old pin | passes 1–6 with 5b on `iphands/basalt-blackwell:cu1343-sm120` (`4de9f15bb9d2`) — tracker Evidence row 1 | run 2026-11-21 before the plan commit |
| Agent shell can run gate 3–6 | `docker` is in `id -nG` | `id -nG`, 2026-11-21 |
| Rollback image | `cu1343-sm120-ad274aa` (`4de9f15bb9d2`, label `ai.basalt.patches=0001-buffered-io-switch.patch:a3553ec6bb8c`) stays on disk; `./build` tags only `…-ff0b978` and moves nothing else | `docker images iphands/basalt-blackwell`, `docker image inspect`, 2026-11-21; `build` |
| A server is up | container `basalt`, 14 hours, on `iphands/basalt-blackwell:cu1343-sm120` (`4de9f15bb9d2`); a build does not touch it | `docker ps`, `docker inspect -f '{{.Image}}' basalt`, 2026-11-21 |

### Rejected Claims

- **"The recipe needs a change because upstream added a model."** Not on this evidence: the default
  builds every model and the runtime tree is the one upstream's own unchanged `Dockerfile` ships. Do
  NOT narrow `BASALT_MODELS` here — it is a build-flag change (a plan of its own).
- **"The patch needs re-fitting at every repin."** Only when one of its three files moves. None did;
  do NOT regenerate or re-header the patch — its hash is what the image label and Plan 12 name.
- **"`vendor/basalt` can be probed with plain git."** The clone is `blob:none`; an unguarded probe
  lazy-fetches. `GIT_NO_LAZY_FETCH=1` on every probe that is not a deliberate read of a named file.

---

## Step-by-Step Tasks

### T1: Repin to `ff0b978`, preflight, owner build, gate on the new image

**File**: `versions.env`, `README.md`; `Dockerfile`, `Dockerfile.dockerignore` and the `RULES.md` gate snippets only if the build or the gate finds a reason

**What to do**, in this order — nothing is committed until step 5:
1. Agent: edit `versions.env` (below). The file stays dirty until step 5.
2. Agent: `PREFLIGHT_ONLY=1 ./build` — checks `vendor/basalt` out at the new SHA, runs
   `git apply --check` on `patches/0001`, resolves both CUDA digests, lints. Wanted:
   `== patch fits: 0001-buffered-io-switch.patch`, last line `preflight ok`. Then, in the checked-out
   tree, `grep -rn 'open_unbuffered\|DirectFile' src include` against the same grep at `ad274aa`: a new
   caller with no cached path when `open_unbuffered` answers -1 is a stop. Then gate steps 1–4 on
   the dirty tree (step 4 prints `image: stub:none` until the image exists). Hand off with
   `dirty: versions.env (mine, in progress)`.
3. **Owner**: `./build` → `iphands/basalt-blackwell:cu1343-sm120-ff0b978`. Log:
   `captures/build_<date>_ff0b978.log`.
4. Agent: the image id and labels (`ai.basalt.ref`, `ai.basalt.patches`); the build log's
   `== applying 0001-buffered-io-switch.patch` line and the compile step's time and target count;
   gate 1–6 with 5b, steps 5, 5b and 6 against the new image; step 5's negative control.
5. Agent: `README.md` (Status, Pins, tag names, the sentence saying the image on disk predates the
   patch), tracker row, commit.

**Before**:
```bash
# Basalt: v0.5.0, the tag's commit (owner decision 2026-11-20, Plan 10). Was d3b75742 (v0.4.1,
# Plan 08); before that dc047a77 (main on 2026-11-04, the v0.2.1 engine).
BASALT_REPO=https://github.com/jesdga95/basalt.git
BASALT_SHA=ad274aa8dc243f0fdcc1ad382f2e31c4591b4552
```

**After**:
```bash
# Basalt: v0.5.5, the tag's commit (owner decision 2026-11-21, Plan 13). Was ad274aa8 (v0.5.0,
# Plan 10); before that d3b75742 (v0.4.1, Plan 08) and dc047a77 (main on 2026-11-04, the v0.2.1
# engine).
BASALT_REPO=https://github.com/jesdga95/basalt.git
BASALT_SHA=ff0b978d2ee9ff195d113b6feb80961b2d2f99ef
```

**Verify**:
```bash
PREFLIGHT_ONLY=1 ./build 2>&1 | grep -E 'patch fits|preflight ok'                # → both lines
img=$(bash -c 'source lib/common.sh; load_pins; image_for_pin'); echo "$img"     # → an iphands/basalt-blackwell tag
docker image inspect -f '{{index .Config.Labels "ai.basalt.ref"}}' "$img"         # → ff0b978d2ee9ff195d113b6feb80961b2d2f99ef
docker image inspect -f '{{index .Config.Labels "ai.basalt.patches"}}' "$img"     # → 0001-buffered-io-switch.patch:a3553ec6bb8c
EXTRA_ARGS=--no-such-flag <gate step 5>                                         # → FAIL(flags) --no-such-flag
# + the full project gate 1–6 with 5b
```

**Expected observation**:
- **Confirms if**: the build log shows `== applying 0001-buffered-io-switch.patch` with no fuzz or
  reject and the compile finishes; step 5 prints `flags ok (…)`, step 5b three `pack flags ok` lines
  and step 6 `imports ok (N serve modules)`, all from the image labelled with the new SHA.
- **Refutes if**: the patch does not apply; the build stops on a missing tool or path; step 5 or 5b
  names a flag a launcher passes; step 6 names a module that needs a file our runtime stage does not
  copy (`serve/engine/console.py` is new).
- **Within noise if**: the flag and module counts differ from the v0.5.0 image's (157 engine flags
  known, 42 serve modules), or the build is slower and the image larger with a fifth model — only
  `FAIL` lines matter. Record the figures.

If the build or gate fails in a way this plan's scope cannot fix: put the old `BASALT_SHA` line and
its comment back by hand (nothing was committed, so no `git restore`), re-run `PREFLIGHT_ONLY=1
./build` so `vendor/basalt` returns to `ad274aa`, and record the blocker. The old image stays on disk.

**Commit**: `[basalt][P13][T1][pins] repin Basalt to ff0b978 (v0.5.5) and rebuild`

### T2: Owner boots — `./run` and `./run-hy4` — with a real reply each

**File**: tracker Evidence only (boot start blocks pasted by the owner)

**What to do**: stop whatever serves `:8700`. **Owner**: `./run`, then from another shell one
`/v1/chat/completions` request to `http://cosmo.lan:8700/v1` as `cosmo-6000`; Ctrl+C; `./run-hy4`
(after `./preload` if the pack is on NFS), the same request. Paste each launcher's `==` lines, the
engine's start block and the reply. Both boots run under `BUFFERED_IO=1`, the launchers' default.

**Verify**: the pasted output is read, not inferred (`RULES.md` § *Project gate* → Look at it).

**Expected observation**:
- **Confirms if**: `== image:` names an image labelled `ff0b978…` and `== pack:` the pack asked for;
  the engine log says `session is up (engine 0.5.5)`; the engine accepts each launcher's flags; a
  reply comes back from each.
- **Refutes if**: the engine refuses a flag value, caps the context below 362144, or stalls on a
  prompt — then the setting is a new owner decision, not a silent edit.
- **Within noise if**: load time, the `elastic KV:` start figures and the VRAM/RAM split differ from
  v0.5.0's. Record them; do not explain them away.

**Commit**: `[basalt][P13][T2][boot] record the owner boots on v0.5.5`

### T3: Harvest and close

**File**: `context/distilled.md`, `context/high_level.md`, `context/pitfalls.md`, this plan + tracker, `SERIES.md`

**What to do**: dated superseding notes in `distilled.md` for every fact T1/T2 changed (the built
image, build time and size, flag and module counts, boot numbers, five models built and two served),
each tagged with how it is known; `high_level.md`: the pin's rationale; `pitfalls.md`: anything that
took more than one attempt. Re-read each file after writing. Carry the tracker's Follow-ups to where
they land. Then `git mv` this plan and its tracker to `completed/` and mark SERIES done.

**Verify**: every finding the tracker mentions is found by `grep` in the file it names; the full gate.

**Commit**: `[basalt][P13][close] Basalt repin to v0.5.5: <outcome>`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `versions.env` | `BASALT_SHA` and its comment | P0 |
| `README.md` | Status, Pins, tag names, the patched-image sentence | P1 |
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
2. **Risk: between the pin edit and the build, `./run` and `./run-hy4` refuse** ("no … image is
   labelled `ai.basalt.ref`=…; run `./build` first"). *Mitigation*: the server that is up keeps
   running; a restart meanwhile is
   `IMAGE_OVERRIDE=iphands/basalt-blackwell:cu1343-sm120-ad274aa ./run`.
3. **Risk: Flash-Next or Hy4-flash behaves differently at boot or under long prompts** (22 and 16
   files of their model folders changed, and shared `src/core`, `src/prefill`, `src/kernels`).
   *Mitigation*: T2's two boots; the v0.5.0 image stays on disk as the rollback.
4. **Risk: the image is published before it has booted.** The owner asked for the repin in order to
   build and publish, and gate 1–6 is model-free. *Mitigation*: the immutable tag names the commit;
   the moving tags are the owner's to move; T2 records the first boot whenever it happens.
5. **Risk: four plans are open underneath this one** (08, 10, 11, 12), three of them waiting on
   boots of an image a plain `./run` no longer selects once the pin moves. *Mitigation*: their files
   are not edited here; the tracker's Follow-ups name each decision.
6. **Risk: the built pin is unaudited, and Plan 09's row names `d3b7574`.** *Mitigation*: Follow-ups
   row; nothing in `context/audit/` claims to cover v0.4.1, v0.5.0 or v0.5.5.
7. **Question (deferred — goes to the audit plan, as Plan 10 Open Question 6 did): does the pin's tree
   equal GitHub's tree object for `v0.5.5`?** The tag object is in the clone and carries no signature
   (Key Facts), so there is no signature to check. *How we'll settle it*: the audit's first tasks.

---

## Verification Checklist

- [ ] T1: `image_for_pin` prints a tag whose `ai.basalt.ref` label is `ff0b978d2ee9ff195d113b6feb80961b2d2f99ef` and whose `ai.basalt.patches` label names `0001-buffered-io-switch.patch:a3553ec6bb8c`; gate steps 5, 5b and 6 pass on it; step 5's negative control fires
- [ ] T2: both boot logs name the `ff0b978` image and the pack; a real reply from each launcher is quoted in the tracker
- [ ] T3: each harvested fact is in the file the tracker names, re-read after writing
- [ ] All: the project gate (Rule A) passes on the final commit
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
