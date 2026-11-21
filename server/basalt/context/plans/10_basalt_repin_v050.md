# Plan 10 — Repin Basalt to v0.5.0 and rebuild the image

> **Status**: pending
> **Created**: 2026-11-20
> **Depends on**: Plan 01 (the image recipe, `./build`, `./run`, gate steps 3–6); starts from Plan 08 T2's pin (`d3b7574`)
> **Goal**: `versions.env` pins Basalt `ad274aa` (v0.5.0), the image `iphands/basalt-blackwell:cu1343-sm120-ad274aa` is built from it, the full gate passes against that image, and `./run` boots the Flash-Next pack on it with a real reply in both modes.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Move the Basalt pin from `d3b7574` (v0.4.1) to the v0.5.0 tag commit, rebuild, and prove
the launcher still boots.

**Deliverables**:
1. `versions.env` at `ad274aa8dc243f0fdcc1ad382f2e31c4591b4552`; image `cu1343-sm120-ad274aa` built
   by the owner; gate 1–6 pass on it; `README.md` names the new pin and tag.
2. Owner boots, default and `--single`, each with a real `/v1/chat/completions` reply, recorded.
3. Changed facts harvested into `context/`.

**Estimated effort**: Small (2 h of agent work; the owner's build and two boots).

---

## Scope

**In**: `versions.env`, `README.md`, plan bookkeeping, `context/` harvest; `Dockerfile` and
`Dockerfile.dockerignore` **only if** T1's read of the checked-out tree finds something the recipe
must follow; the gate step 5/6 snippets in `RULES.md` only if `serve.config` moved.
**Out** (owner decision 2026-11-20, "deal with those other things later"): Plan 08's open rows (T3
boots of the v0.4.1 image, T5 harvest and close); re-targeting Plan 09's audit from `d3b7574` to the
new pin, and the lines in `context/AGENTS.md` and `context/audit/index.md` that call `d3b7574` the
built pin. Also out: `./publish` runs, tuning any default (Plan 05), narrowing `BASALT_MODELS`,
serving Hy4-flash or the 27B, the CUDA base pins. If a fix needs to cross this line, **stop** and
record the blocker in the tracker.

---

## Context

The owner asked (2026-11-20) to repin to 0.5.0. A pin change is a plan (`RULES.md` § *When Does a
Change Need a Plan?*): upstream keeps no flag shims, so the plan says what the release notes changed
and re-boots. Precedent and method: `context/plans/08_basalt_repin_v041.md`.

### Pre-Identified Bug/Issue

Not a bug: upstream released past the pin (2026-11-20; the filter keeps `main` and the 0.4/0.5 tags).

```bash
$ git ls-remote --tags --heads https://github.com/jesdga95/basalt.git 2>&1 | grep -E 'v0\.(4|5)|refs/heads/main' | tail -20
8ba264c0b7a3b944edf613c78672e19deaa28606	refs/heads/main
01f620d92f34fe80a2d05081683f4f5e38cc06d2	refs/tags/v0.4.0
99dc78cb51ac43555c3feed9a1ec14f056d05035	refs/tags/v0.4.0^{}
33bd410c1bcc5bfa738efae57cc01a6911415e57	refs/tags/v0.4.1
d3b75742967c7f9fd892c6894a2b9ac6e36e0cf3	refs/tags/v0.4.1^{}
fa7b40ab76032a23f57c363cd56683e4ce881354	refs/tags/v0.5.0
ad274aa8dc243f0fdcc1ad382f2e31c4591b4552	refs/tags/v0.5.0^{}
```

Today's pin and checkout:

```bash
$ grep '^BASALT_SHA' versions.env; GIT_NO_LAZY_FETCH=1 git -C vendor/basalt rev-parse HEAD
BASALT_SHA=d3b75742967c7f9fd892c6894a2b9ac6e36e0cf3
d3b75742967c7f9fd892c6894a2b9ac6e36e0cf3
```

### Why repin by tag commit, build-then-commit

Same two choices as Plan 08, for the same reasons.

- **Tag commit over `main`'s tip.** The tag has release notes with a Checked section; `main` is
  already past it (`8ba264c0`). Rejected: `main` on the day.
- **One task for edit + build + gate (T1).** Gate steps 5–6 fail with no image for `BASALT_SHA`, and
  the gate reads the working tree, so no commit can pass between the pin edit and the owner's build.
  Rejected: committing the pin first — that commit would be made on a red gate.

### Key Facts

`[DOC]` rows are upstream's files at the `v0.5.0` tag, read on 2026-11-20 from
`raw.githubusercontent.com` **through a fetch tool that returns paraphrase and short quotes, not the
bytes**. They are claims until T1 step 3 reads the checked-out tree and T2 boots.

| Fact | Value | How confirmed |
|---|---|---|
| New pin | `ad274aa8dc243f0fdcc1ad382f2e31c4591b4552` = `refs/tags/v0.5.0^{}` (annotated tag `fa7b40ab`), released 2026-11-19 | `git ls-remote`, above; date `[DOC]` `docs/changelog/0.5.0.md` |
| Distance | `d3b7574` → `ad274aa` is 206 commits, `behind_by` 0, merge base `d3b7574` — a strict descendant | `GET /repos/jesdga95/basalt/compare/v0.4.1...v0.5.0`, top-level fields, 2026-11-20; re-count locally in T1 step 3 |
| Headline | a fourth model, Tencent Hy4-flash (`hy4flash`); `-DBASALT_MODELS` accepts it and the default still builds every model | `[DOC]` changelog |
| Flags and formats | "No flag or config key is removed or renamed"; `.basalt` format and config both stay at version 2 | `[DOC]` changelog; confirm with gate step 5 in T1 |
| Flags `./run` passes | `--host --port --alias --allowed-host --api-key --parallel` (1..8) `--slot-full-context --vision --gpu` are server options in `serve/config.py`; `--max-context` and `--kv` (`fp16`, `int8`, `q4_0`, `k8v4` for Flash-Next) are documented engine flags; **`--ple-io` was in none of the three files read** | `[DOC]` `serve/config.py`, `docs/models/qwen38-flash-next.md`, `docs/running.md`; `--ple-io` unconfirmed — gate step 5 in T1 decides |
| Gate hooks | `serve.config` still defines `build_parser`, `server_options`, `engine_flags_of` | `[DOC]` `serve/config.py`; confirm in T1 step 3 |
| Image recipe inputs | upstream's `Dockerfile`, `.dockerignore`, `requirements.txt` at the tag are 3512 / 110 / 343 characters — the byte sizes of the same files at `d3b7574` — and their summaries match (same apt packages, same `bin/ serve/ tools/ data/ .venv` tree, same pins incl. `pypdf` 7.0.1) | sizes as the fetch tool reported them vs `wc -c vendor/basalt/{Dockerfile,.dockerignore,requirements.txt}`, 2026-11-20. Equal size is not equal bytes: `git diff d3b7574 ad274aa -- <the three>` in T1 step 3 |
| Flash-Next code moved | the compare lists changes under `src/models/qwen4exp` (about 34 files) and shared `src/program`, `src/vision`, `src/platform`; none seen under `src/core` or `src/kernels` | compare API `files`, read only in part through the same tool — **counts unverified**; `git diff --stat` in T1 step 3 |
| Shared-engine behaviour | elastic attention caches grow into the expert tier's room and a prompt that cannot grow is **refused** (`Status::Refused`; the engine keeps serving); partial reclaim; bounded lend is off for every model but Hy4-flash | `[DOC]` changelog; observe in T2. Elastic KV is already live at v0.4.1 on our settings (`storage/cache/basalt/aefd5d98dc650ed4/engine.log`, `elastic KV:` lines through 2026-11-19) |
| Output | Flash-Next greedy output on pinned tiers "identical to `main`" at `63cb68ad4` — **not compared with v0.4.1** | `[DOC]` changelog Checked section — upstream's claim, not re-measured here |
| Build cost | every model is built, now four; v0.4.1's three took 319.6 s at `JOBS=16`, image 3.49 GB | Plan 08 tracker Evidence row 4; new figure recorded in T1 |
| Gate baseline, old pin | see tracker Evidence row 1 | run 2026-11-20 before the plan commit |
| Agent shell can run gate 3–6 | `docker` is in `id -nG` | `id -nG`, 2026-11-20 |
| Rollback image | `cu1343-sm120-d3b7574` (`41619e9056f8`) and `cu1343-sm120-dc047a7` (`63d25a81f328`) are on disk | `docker images iphands/basalt-blackwell`, 2026-11-20 |

### Rejected Claims

- **"The recipe needs a change because upstream added a model."** Not on this evidence: the default
  `BASALT_MODELS` builds every model and the runtime tree is unchanged. Do NOT narrow `BASALT_MODELS`
  here — it is a build-flag change (a plan of its own) and out of scope.
- **"`vendor/basalt` can be probed for the new commit with a plain `git cat-file`."** The clone is
  `blob:none`; an unguarded probe lazy-fetches (Plan 08 tracker Notes). `GIT_NO_LAZY_FETCH=1` on every
  probe; the fetch is the owner's preflight.

---

## Step-by-Step Tasks

### T1: Repin to `ad274aa`, owner build, read the tree, gate on the new image

**File**: `versions.env`, `README.md`; `Dockerfile`, `Dockerfile.dockerignore` and the `RULES.md` gate step 5/6 snippets only if step 3 finds a reason

**What to do**, in this order — nothing is committed until step 6:
1. Agent: edit `versions.env` (below). Hand off with `dirty: versions.env (mine, in progress)`.
2. **Owner**: `PREFLIGHT_ONLY=1 ./build` — fetches origin, checks `vendor/basalt` out at the new
   SHA, resolves both CUDA digests, lints. Last line `preflight ok`.
3. Agent, from the checked-out tree, `GIT_NO_LAZY_FETCH=1` on every git command:
   `git -C vendor/basalt rev-parse HEAD` is the pin; `rev-list --count d3b7574..ad274aa` (expect 206);
   `git diff --stat d3b7574 ad274aa -- Dockerfile .dockerignore requirements.txt CMakeLists.txt cmake docs/building.md serve/config.py data`
   and read whatever it lists; confirm `build_parser`, `server_options`, `engine_flags_of` in
   `serve/config.py`. Adapt `Dockerfile` / `Dockerfile.dockerignore` only if a directory, file, apt
   package or tool minimum changed. If `serve.config`'s names moved, update gate steps 5/6 in
   `RULES.md` in this commit (Rule A.5) and re-run step 5's negative control. Keep upstream's default
   `BASALT_MODELS`. *(A blob the diff needs may not be local in a `blob:none` clone; if the guarded
   command refuses, read the file in the checkout instead and say so in the tracker.)*
4. **Owner**: `./build` → `iphands/basalt-blackwell:cu1343-sm120-ad274aa`; paste the image id and
   labels lines. Log: `captures/build_<date>_ad274aa.log`.
5. Agent: gate 1–6. Steps 5–6 must run against the new image.
6. Agent: `README.md` (Status, Pins, tag names), tracker row, commit.

**Before**:
```bash
# Basalt: v0.4.1, the tag's commit (owner decision 2026-11-16, Plan 08). Was dc047a77 (main on
# 2026-11-04, the v0.2.1 engine).
BASALT_REPO=https://github.com/jesdga95/basalt.git
BASALT_SHA=d3b75742967c7f9fd892c6894a2b9ac6e36e0cf3
```

**After**:
```bash
# Basalt: v0.5.0, the tag's commit (owner decision 2026-11-20, Plan 10). Was d3b75742 (v0.4.1,
# Plan 08); before that dc047a77 (main on 2026-11-04, the v0.2.1 engine).
BASALT_REPO=https://github.com/jesdga95/basalt.git
BASALT_SHA=ad274aa8dc243f0fdcc1ad382f2e31c4591b4552
```

**Verify**:
```bash
PREFLIGHT_ONLY=1 ./build 2>&1 | tail -1                                         # → preflight ok
img=$(bash -c 'source lib/common.sh; load_pins; image_for_pin'); echo "$img"     # → an iphands/basalt-blackwell tag
docker image inspect -f '{{index .Config.Labels "ai.basalt.ref"}}' "$img"         # → ad274aa8dc243f0fdcc1ad382f2e31c4591b4552
EXTRA_ARGS=--no-such-flag <gate step 5>                                         # → FAIL(flags) --no-such-flag
# + the full project gate 1–6
```

**Expected observation**:
- **Confirms if**: the build completes; step 5 prints `flags ok (…)` and step 6 `imports ok (N serve
  modules)` from the image labelled with the new SHA.
- **Refutes if**: step 5 names a flag `./run` passes (`--ple-io` is the unconfirmed one), step 6 names
  a module that needs a file our runtime stage does not copy, or the build stops on a missing tool or
  path.
- **Within noise if**: the flag and module counts differ from the v0.4.1 image's (153 engine flags
  known, 40 serve modules), or the build is slower and the image larger with a fourth model — only
  `FAIL` lines matter. Record the figures.

If the build or gate fails in a way this plan's scope cannot fix: put the old `BASALT_SHA` line back
by hand (nothing was committed, so no `git restore`), ask the owner to re-run `PREFLIGHT_ONLY=1
./build` so `vendor/basalt` returns to `d3b7574`, and record the blocker. The old image stays on disk.

**Commit**: `[basalt][P10][T1][pins] repin Basalt to ad274aa (v0.5.0) and rebuild`

### T2: Owner boots — default and `--single` — with a real reply each

**File**: tracker Evidence only (boot start blocks pasted by the owner)

**What to do**: stop whatever serves `:8700`. **Owner**: `./run`, then from another shell one
`/v1/chat/completions` request to `http://cosmo.lan:8700/v1` as `cosmo-6000`; Ctrl+C; `./run --single`,
the same request. Paste `./run`'s `== image/pack/mode` lines, the engine's start block and the reply.

**Verify**: the pasted output is read, not inferred (`RULES.md` § *Project gate* → Look at it).

**Expected observation**:
- **Confirms if**: `== image:` names an image labelled `ad274aa…` and `== pack:` the NFS pack; the
  start block says engine 0.5.0; the engine accepts `3 slots × 362144` and `--kv int8 --ple-io ram
  --vision dynamic`; a reply comes back in each mode.
- **Refutes if**: the engine refuses a flag value, caps Flash-Next's context below 362144, or answers
  a normal prompt with the new refusal — then the setting is a new owner decision, not a silent edit.
- **Within noise if**: load time, the `elastic KV:` start figures and the VRAM/RAM split differ from
  v0.4.1's. Record them; do not explain them away.

**Commit**: `[basalt][P10][T2][boot] record the owner boots on v0.5.0`

### T3: Harvest and close

**File**: `context/distilled.md`, `context/high_level.md`, `context/pitfalls.md`, this plan + tracker, `SERIES.md`

**What to do**: dated superseding notes in `distilled.md` for every fact T1/T2 changed (the built
image, build time and size, flag and module counts, boot numbers, the refusal behaviour if seen),
each tagged with how it is known; `high_level.md`: the pin's rationale; `pitfalls.md`: anything that
took more than one attempt. Re-read each file after writing. Carry the tracker's Follow-ups to where
they land. Then `git mv` this plan and its tracker to `completed/` and mark SERIES done.

**Verify**: every finding the tracker mentions is found by `grep` in the file it names; the full gate.

**Commit**: `[basalt][P10][close] Basalt repin to v0.5.0: <outcome>`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `versions.env` | `BASALT_SHA` and its comment | P0 |
| `README.md` | Status, Pins, tag names | P1 |
| `Dockerfile`, `Dockerfile.dockerignore` | only if T1 step 3 finds a reason; none expected | P1 |
| `context/plans/RULES.md` | gate step 5/6 snippets, only if `serve.config` moved | P1 |
| `context/plans/SERIES.md` | bookkeeping | P1 |
| `context/distilled.md`, `context/high_level.md` | harvest | P1 |

---

## Open Questions / Risks

1. **Risk: the working tree is dirty across the owner's build (T1 steps 1–5), in a worktree with two
   writers (Project Rule P1).** *Mitigation*: only `versions.env` (and `README.md` at step 6) is dirty,
   every handoff carries the `dirty:` line, and no `context/` or plan file is left uncommitted while
   the owner works.
2. **Risk: the pre-read of v0.5.0 was paraphrase, so a renamed flag or a changed build input could
   have been missed.** *Mitigation*: T1 step 3 diffs the checked-out tree; gate steps 5–6 run on the
   built image; T2 boots.
3. **Risk: Flash-Next behaves differently at boot or under long prompts** (its model folder changed,
   and a prompt whose cache cannot grow is now refused). *Mitigation*: T2's two boots; a refusal or a
   context cap goes to the owner as a decision. The v0.4.1 image stays on disk as the rollback
   (`IMAGE_OVERRIDE=iphands/basalt-blackwell:cu1343-sm120-d3b7574 ./run` boots it whatever the pin
   says: `run` takes `IMAGE_OVERRIDE` before `image_for_pin`).
4. **Risk: Plan 08 is left open underneath this plan** (owner decision 2026-11-20). Its T3 asks for
   boots of the `d3b7574` image, which a plain `./run` no longer selects once the pin moves.
   *Mitigation*: Plan 08's files are not edited here; the tracker's Follow-ups name the decision.
5. **Risk: the built pin is unaudited, and Plan 09's row names the pin before it.** *Mitigation*:
   Follow-ups row; nothing in `context/audit/` claims to cover either v0.4.1 or v0.5.0.
6. **Question (deferred — goes to the audit plan, as Plan 08 Open Question 7 did): is the `v0.5.0` tag
   signed, and does the pin's tree equal GitHub's tree object?** *How we'll settle it*: T1 step 3
   records whether the owner's fetch brought the tag object (`git tag -l v0.5.0`); the check itself is
   the audit's.

---

## Verification Checklist

- [ ] T1: `image_for_pin` prints a tag whose `ai.basalt.ref` label is `ad274aa8dc243f0fdcc1ad382f2e31c4591b4552`; gate steps 5–6 pass on it; step 5's negative control fires
- [ ] T2: both boot logs name the `ad274aa` image and the pack; a real reply in each mode is quoted in the tracker
- [ ] T3: each harvested fact is in the file the tracker names, re-read after writing
- [ ] All: the project gate (Rule A) passes on the final commit
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
