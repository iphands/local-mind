# Plan 08 — Repin Basalt to v0.4.1 and rebuild the image

> **Status**: in-progress
> **Created**: 2026-11-16
> **Depends on**: Plan 01 (the image recipe, `./build`, `./run`, gate steps 3–6)
> **Goal**: `versions.env` pins Basalt `d3b7574` (v0.4.1), the image `iphands/basalt-blackwell:cu1343-sm120-d3b7574` is built from it, the full gate passes against that image, and `./run` boots the Flash-Next pack on it with a real reply in both modes.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Move the Basalt pin from `dc047a7` (v0.2.1 + 3 commits) to the v0.4.1 tag commit,
rebuild, and prove the launcher still boots.

**Deliverables**:
1. `./run` honours a pack argument again, so gate step 4 is green (it is red at `6275af2`).
2. `versions.env` at `d3b75742967c7f9fd892c6894a2b9ac6e36e0cf3`; `Dockerfile` adapted to the new
   tree; image `cu1343-sm120-d3b7574` built by the owner; gate 1–6 pass on it.
3. Owner boots, default and `--single`, each with a real `/v1/chat/completions` reply, recorded.
4. Plans 02 and 03 (the audit of `dc047a7`) abandoned with the reason; a re-audit of the new pin
   registered as a pending plan (owner decision 2026-11-16).
5. Changed facts harvested into `context/`.

**Estimated effort**: Small–Medium (half a day of agent work; the owner's build and two boots).

---

## Scope

**In**: `versions.env`, `Dockerfile`, `Dockerfile.dockerignore`, `run` (the pack default only),
`README.md`, the gate step 5/6 snippets in `RULES.md` if `serve.config` moved, plan bookkeeping,
`context/` harvest.
**Out**: `./publish` runs (owner-only, whenever wanted), tuning any default (Plan 05), video/FFmpeg
in the image, serving the Qwen3.8-27B that Basalt 0.4 can now load, the CUDA base pins. If a fix
needs to cross this line, **stop** and record the blocker in the tracker.

---

## Context

The owner asked (2026-11-16) how to repin and rebuild on the latest Basalt. A pin change is a plan
(`RULES.md` § *When Does a Change Need a Plan?*): upstream keeps no flag shims, so the plan says what
the release notes changed and re-boots. Read first: `context/high_level.md` § *The image — two
stages, upstream's source-tree layout at `/opt/basalt`* and § *Attesting the Basalt pin `dc047a77`*.

Owner decisions, this session: pin the **v0.4.1 tag commit**, not `main`'s tip (9 commits further:
two grammar/sampling fixes, a serve progress feature, an optional Ada-peer build flag, community
rows — none with release checks behind them yet); **abandon Plans 02 and 03** and re-audit the new
pin later.

### Pre-Identified Bug/Issue

Upstream has moved past the pin (2026-11-16, pull refs filtered out, older tags elided):

```bash
$ git ls-remote https://github.com/jesdga95/basalt.git | grep -v refs/pull/
98105f8f147c68dc1e3dae23d0957b14073b2837	HEAD
98105f8f147c68dc1e3dae23d0957b14073b2837	refs/heads/main
4ae7af98d1bedf5a36de06a58bcfbdf12592950e	refs/tags/v0.2.1
1b3f69641c989f2c1b6b6faf1525a65574fa1446	refs/tags/v0.2.1^{}
33bd410c1bcc5bfa738efae57cc01a6911415e57	refs/tags/v0.4.1
d3b75742967c7f9fd892c6894a2b9ac6e36e0cf3	refs/tags/v0.4.1^{}
```

And gate step 4 is red at `6275af2`, before anything in this plan: `./run` line 54 assigned
`PACK=/mnt/noir/…` after the argument was read, so a pack argument was ignored.

```bash
$ DRY_RUN=1 ./run /tmp/x.ninfer >/dev/null 2>&1; echo "ninfer rc=$? (gate expects non-zero)"
ninfer rc=0 (gate expects non-zero)
```

### Why repin by tag commit, build-then-commit

- **Tag commit over `main`'s tip.** The tag has release notes with an Upgrading list and a Checked
  section; the 9 later commits have neither. Rejected: `main` on the day, which is what Plan 01
  pinned when no release was newer than the fixes it wanted.
- **One task for edit + build + gate (T2).** Gate steps 5–6 fail with no image for `BASALT_SHA`, and
  the gate reads the working tree, so no commit can pass between the pin edit and the owner's build.
  Rejected: committing the pin first and the build record second — the first commit would be made on
  a red gate.

### Key Facts

`[DOC]` rows are upstream's release notes, read with `GET /repos/jesdga95/basalt/releases` on
2026-11-16; they are claims until T2/T3 confirm them against the built image.

| Fact | Value | How confirmed |
|---|---|---|
| New pin | `d3b75742967c7f9fd892c6894a2b9ac6e36e0cf3` = `refs/tags/v0.4.1^{}`, released 2026-11-16 | `git ls-remote`, above |
| Distance | `dc047a7` → `main` is 707 commits; v0.4.1 → `main` is 9 | `GET …/compare/dc047a77…main` and `…/compare/v0.4.1...main`, 2026-11-16 |
| `./build` handles a new SHA | fetches origin when the SHA is not local, checks out detached, refuses a dirty tree, tags `cu<digits>-sm120-<sha7>` | `build:54-58`, `build:71` |
| Flags removed since 0.2.1 | `--peer-prefill-rows`, engine `--vision-mmproj`, `--no-answer-room` — `./run` passes none | `[DOC]` 0.3.1, 0.3.3 Upgrading; `run` `serve_args`; confirm with gate step 5 in T2 |
| `--kv int8` | still valid for Flash-Next (`fp16`, `int8`, `q4_0`, `k8v4`) | `[DOC]` 0.3.1 Upgrading 6; confirm at boot (T3) |
| Install root | `BASALT_SOURCE_DIR` is gone; the binary finds `serve/`, `data/`, `.venv` beside its own `bin/` | `[DOC]` 0.3.2; confirm in T2 against the tree and in T3 at boot |
| Python deps | `requirements.txt` adds `pypdf` 7.0.1 | `[DOC]` 0.3.3; confirm in T2 from the file |
| Video | needs FFmpeg 6–8 libraries at run time; without them the start prints `video  off` | `[DOC]` 0.3.3; confirm at boot (T3) |
| Build inputs | `CMakeLists.txt` +777/−407; `-DBASALT_MODELS`, default every model; upstream ships its own `Dockerfile` and `.dockerignore` | compare API file list, 2026-11-16; `[DOC]` 0.4.0; read in T2 |
| Server | multi-model router, config v2; a config without `models` is refused | `[DOC]` 0.3.0 — we pass flags, no config file; `serve <pack> --alias` unconfirmed — confirm in T3 |
| Memory | `--ram-reserve-mib` (default 2048) replaces the RAM floors; checkpoints take page-locked RAM from start; the pack is read in place, nothing unpacked into `~/.cache/basalt` | `[DOC]` 0.3.1, 0.4.0, 0.4.1; observe in T3 |
| Output | Flash-Next greedy output identical at 0.3.0, 0.4.0 and 0.4.1 against the release before | `[DOC]` Checked sections — upstream's claim, not re-measured here |
| Dense 27B | Basalt 0.4.0 loads Qwen3.8-27B (QUASAR NVFP4 + DFlash 2) as a `.basalt` pack | `[DOC]` 0.4.0 — fires `high_level.md`'s "Revisit if"; out of scope, follow-up |
| Gate baseline, old pin | 1–6 pass with T1's fix: `4 b`/`5 b`, `env ok`, shellcheck ok ×2, `preflight ok`, `dry-run ok`, `flags ok (3 engine, 8 server: 142 engine flags known)`, `imports ok (32 serve modules)` | run 2026-11-16, tracker Evidence |
| Agent shell can run gate 3–6 | `docker` group, daemon 29.8.1 | `id -nG`, `docker version`, 2026-11-16 |

### Rejected Claims

- **"`MAX_CONTEXT=362144` is a typo for 262144."** Do NOT re-fix: it is the owner's default
  (`91c4071`, tracked by gate step 4). T1 corrects the header and README, which still said 262144.
- **"Our `Dockerfile.dockerignore` stops applying once upstream ships a `.dockerignore`."** BuildKit
  prefers `<Dockerfile>.dockerignore` beside the Dockerfile named by `-f`; only its comment goes stale.

---

## Step-by-Step Tasks

### T1: Let a pack argument override the NFS default again

**File**: `run`, `README.md`

**What to do**: fold the unconditional `PACK=` assignment into the default, keeping the owner's NFS
path; bring the header, the missing-pack hint and README in line with the defaults the code has
(`362144`, the NFS pack).

**Before**:
```bash
PACK=${pack_arg:-${PACK:-$HERE/models.local/Qwen3.8-FN-NVFP4-DQ6_K.basalt}}
PACK=/mnt/noir/scratch/ai/llm/models/vllm/jesdga/Qwen3.8-FN-NVFP4-DQ6_K.basalt
```

**After**:
```bash
PACK=${pack_arg:-${PACK:-/mnt/noir/scratch/ai/llm/models/vllm/jesdga/Qwen3.8-FN-NVFP4-DQ6_K.basalt}}
```

**Verify**:
```bash
DRY_RUN=1 ./run /tmp/x.ninfer >/dev/null 2>&1; echo "rc=$?"    # → rc=1
PACK=README.md DRY_RUN=1 ./run 2>&1 | tail -1                    # → … is not a .basalt pack …
# + the full project gate 1–6 on the OLD pin — the green baseline before the pin moves
```
Expected: both refusals, and gate step 4 prints `dry-run ok` with no `FAIL` line.

**Commit**: `[basalt][P8][T1][run] let a pack argument override the NFS default again`

### T2: Repin to `d3b7574`, adapt the image recipe, owner build, gate on the new image

**File**: `versions.env`, `Dockerfile`, `Dockerfile.dockerignore`, `README.md`; `context/plans/RULES.md` gate step 5/6 snippets only if `serve.config` moved

**What to do**, in this order — nothing is committed until step 6:
1. Agent: edit `versions.env` (below). Hand off with `dirty: versions.env (mine, in progress)`.
2. **Owner**: `PREFLIGHT_ONLY=1 ./build` — fetches origin, checks `vendor/basalt` out at the new
   SHA, resolves both CUDA digests, lints. Last line `preflight ok`.
3. Agent, from the checked-out tree, no network: `git -C vendor/basalt rev-parse HEAD` is the pin and
   `v0.4.1^{commit}` resolves to it; read upstream's `Dockerfile`, `.dockerignore`, `docs/building.md`,
   `docs/platforms.md` (container image), `requirements.txt`, `CMakeLists.txt` options and
   `serve/config.py` (`build_parser`, `server_options`, `engine_flags_of`). Adapt `Dockerfile` (the
   `BASALT_SOURCE_DIR` header comment; any directory, file or apt package the new server needs;
   cmake/g++ minimums) and the `Dockerfile.dockerignore` comment. If `serve.config`'s names moved,
   update gate steps 5/6 in `RULES.md` in this commit (Rule A.5) and re-run step 5's negative control.
   Keep upstream's default `BASALT_MODELS`.
4. **Owner**: `./build` → `iphands/basalt-blackwell:cu1343-sm120-d3b7574`; paste the image id and
   labels lines. Log: `captures/build_<date>_d3b7574.log`.
5. Agent: gate 1–6. Steps 5–6 must name the new image.
6. Agent: `README.md` (Pins, Status, tag names), tracker row, commit.

**Before**:
```bash
# Basalt: main on 2026-11-04 (owner decision, Plan 01) — v0.2.1 (1b3f6964) plus three serve/ + docs
# commits; the engine is identical to v0.2.1.
BASALT_REPO=https://github.com/jesdga95/basalt.git
BASALT_SHA=dc047a77beeddf1e91234d983a91d6bf929e9619
```

**After**:
```bash
# Basalt: v0.4.1, the tag's commit (owner decision 2026-11-16, Plan 08). Was dc047a77 (main on
# 2026-11-04, the v0.2.1 engine).
BASALT_REPO=https://github.com/jesdga95/basalt.git
BASALT_SHA=d3b75742967c7f9fd892c6894a2b9ac6e36e0cf3
```

**Verify**:
```bash
PREFLIGHT_ONLY=1 ./build 2>&1 | tail -1                                   # → preflight ok
img=$(bash -c 'source lib/common.sh; load_pins; image_for_pin'); echo "$img"   # → …:cu1343-sm120-d3b7574
docker image inspect -f '{{index .Config.Labels "ai.basalt.ref"}}' "$img"       # → the full new SHA
EXTRA_ARGS=--no-such-flag <gate step 5>                                   # → FAIL(flags) --no-such-flag
# + the full project gate 1–6
```

**Expected observation**:
- **Confirms if**: the build completes; step 5 prints `flags ok (…)` and step 6 `imports ok (N serve
  modules)` from the `d3b7574` image, N likely above 32 (more server modules upstream).
- **Refutes if**: step 5 names a flag `./run` passes (a rename the release notes did not list), step 6
  names a module that needs a file our runtime stage does not copy, or the build stops on a missing
  tool or path.
- **Within noise if**: the flag and module counts differ from the old image's — only `FAIL` lines matter.

If the build or gate fails in a way this plan's scope cannot fix: put the old `BASALT_SHA` line back
by hand (nothing was committed, so no `git restore`), ask the owner to re-run `./build` so
`vendor/basalt` returns to `dc047a7`, and record the blocker. The old image stays on disk.

**Commit**: `[basalt][P8][T2][pins] repin Basalt to d3b7574 (v0.4.1) and rebuild`

### T3: Owner boots — default and `--single` — with a real reply each

**File**: tracker Evidence only (boot start blocks pasted by the owner)

**What to do**: stop whatever serves `:8700`. **Owner**: `./run`, then from another shell one
`/v1/chat/completions` request to `http://cosmo.lan:8700/v1` as `cosmo-6000`; Ctrl+C; `./run --single`,
the same request. Paste `./run`'s `== image/pack/mode` lines, the engine's start block and the reply.

**Verify**: the pasted output is read, not inferred (`RULES.md` § *Project gate* → Look at it).

**Expected observation**:
- **Confirms if**: `== image:` names `cu1343-sm120-d3b7574` and `== pack:` the NFS pack; the engine
  accepts `3 slots × 362144` and `--kv int8 --ple-io ram --vision dynamic`; a reply comes back in each
  mode; the start prints `video  off` (no FFmpeg in the image).
- **Refutes if**: the engine refuses a flag value, refuses `serve <pack> --alias` without a v2 config,
  or caps Flash-Next's context below 362144 — then `MAX_CONTEXT` is a new owner decision, not a silent edit.
- **Within noise if**: load time and the VRAM/RAM split differ from Plan 01's (40 s / 34 s, 63.29 GiB):
  the pack is on NFS now and 0.3.1–0.4.1 changed the RAM plan. Record them; do not explain them away.

**Commit**: `[basalt][P8][T3][boot] record the owner boots on v0.4.1`

### T4: Retire the `dc047a7` audit plans, register the re-audit

**File**: `context/plans/SERIES.md`, `context/plans/abandoned/` (Plans 02, 03 + trackers), `context/audit/index.md`, `context/AGENTS.md`

**What to do**: `git mv` the four files to `context/plans/abandoned/`; add both plans to SERIES
§ *Abandoned / Superseded* with the reason (the pin moved 707 commits to `d3b7574` and most of `src/`
moved into per-model folders; the manifest, census and findings stand for `dc047a7` only;
`audit/posture.md` on our own launcher still applies). Register the re-audit as a pending SERIES row
under the next free number with a Backlog Rationale line (reuse `context/audit/scripts/` and the
canary method), and repoint Plan 04's dependency to it. Add a dated banner to `audit/index.md` — the
tree it describes is no longer the built pin, and command 1's freeze-check now reports drift by
design — and fix links to the moved plan files.

**Verify**:
```bash
ls context/plans/abandoned/                                        # → 4 files
grep -rn 'plans/0[23]_basalt' context AGENTS.md README.md | grep -v 'plans/abandoned/'   # → no live link to the old paths
# + the full project gate 1–6
```

**Commit**: `[basalt][P8][T4][series] abandon the dc047a7 audit plans, register the re-audit`

### T5: Harvest and close

**File**: `context/distilled.md`, `context/high_level.md`, `context/pitfalls.md`, this plan + tracker, `SERIES.md`

**What to do**: dated superseding notes in `distilled.md` for every fact T2/T3 changed (install-root
resolution, what `~/.cache/basalt` holds, the RAM reserve, video, the boot numbers), each tagged with
how it is known; `high_level.md`: the pin's rationale and the fired 27B "Revisit if" as a follow-up;
`pitfalls.md`: anything that took more than one attempt. Re-read each file after writing. Then
`git mv` this plan and its tracker to `completed/` and mark SERIES done.

**Verify**: every finding the tracker mentions is found by `grep` in the file it names; the full gate.

**Commit**: `[basalt][P8][close] Basalt repin to v0.4.1: <outcome>`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `versions.env` | `BASALT_SHA` and its comment | P0 |
| `run` | pack default instead of an unconditional override; header defaults | P0 |
| `Dockerfile` | header comment; whatever the v0.4.1 tree needs to build and to serve | P0 |
| `README.md` | Pins, Status, tag names, defaults | P1 |
| `Dockerfile.dockerignore` | comment (upstream now has its own `.dockerignore`) | P2 |
| `context/plans/RULES.md` | gate step 5/6 snippets, only if `serve.config` moved | P1 |
| `context/plans/SERIES.md`, `abandoned/02_*`, `abandoned/03_*` | bookkeeping | P1 |
| `context/distilled.md`, `context/high_level.md`, `context/audit/index.md` | harvest | P1 |

---

## Open Questions / Risks

1. **Risk: the working tree is dirty across the owner's build (T2 steps 1–5), in a worktree with two
   writers (Project Rule P1).** *Mitigation*: only `versions.env`, `Dockerfile`,
   `Dockerfile.dockerignore` and `README.md` are dirty, every handoff carries the `dirty:` line, and
   no `context/` or plan file is left uncommitted while the owner works.
2. **Risk: `serve.config`'s helper names moved, so gate steps 5–6 fail for a reason that is the
   gate's, not the image's.** *Mitigation*: T2 step 3 reads `serve/config.py` before the build;
   the snippets change in the same commit and step 5's negative control is re-run.
3. **Risk: the runtime stage misses something the new server reads** (it missed `tools/` once —
   `context/pitfalls.md`). *Mitigation*: upstream's own `Dockerfile` is read for what it ships; gate
   step 6 imports every `serve.*` module; T3 boots.
4. **Risk: building every model lengthens the build.** Upstream reports 290–309 s for a clean Release
   build at 0.3.2 on its machine; ours is unmeasured. *Mitigation*: keep upstream's tested default;
   record the build time; narrowing `BASALT_MODELS` is a build-flag change and a later plan's.
5. **Question: does Flash-Next still take a 362144-token context at v0.4.1?** *How we'll settle it*:
   T3's boot; a refusal goes to the owner as a decision.
6. **Risk: the built pin is unaudited.** Plan 02's reading covered `dc047a7`. *Mitigation*: T4 says so
   in `audit/index.md` and registers the re-audit; `audit/posture.md`'s action list (the open LAN port
   with no API key) is about our launcher and is unchanged by the bump.
7. **Question: is the v0.4.1 tag signed, and does the pin's tree equal GitHub's tree object?**
   (`high_level.md` § *Attesting the Basalt pin `dc047a77`* → Revisit if.) *How we'll settle it*: T2
   step 3 runs `git tag -v v0.4.1` and records the answer; the tree comparison belongs to the re-audit.

---

## Verification Checklist

- [ ] T1: `DRY_RUN=1 ./run /tmp/x.ninfer` exits 1; gate 1–6 pass on the old pin
- [ ] T2: `image_for_pin` prints `…:cu1343-sm120-d3b7574`, its `ai.basalt.ref` label is the full new SHA, gate steps 5–6 pass on it, step 5's negative control fires
- [ ] T3: both boot logs name the `d3b7574` image and the pack; a real reply in each mode is quoted in the tracker
- [ ] T4: Plans 02 and 03 are in `abandoned/` with their reason in SERIES; the re-audit has a row; no link points at the old paths
- [ ] T5: each harvested fact is in the file the tracker names, re-read after writing
- [ ] All: the project gate (Rule A) passes on the final commit
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
