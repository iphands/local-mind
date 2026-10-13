# Plan 01 — Run everything as uid/gid 1000:1000, host and container — Tracker

## Overview

- Status: 10% complete (1/10 tasks; T3b and T10 start `blocked`)
- Start date: 2026-10-13 (T1)
- Plan: `context/plans/01_uid_parity.md`
- Evidence: `context/plans/01_uid_parity.md` § Context (the 2026-10-13 probe transcripts), each
  task's Verify transcript pasted into this tracker's Notes, and — from T10 onward — the launcher's
  echoed `docker … serve …` line plus the boot report. **Test environment for every row**: image
  `iphands/vllm-blackwell:cu1321-sm120-main-vllm0.32.0.dev20261013-gdf6c7ad` (Wave 1 — the *pre*-T3
  image, which is deliberate: Wave 1 changes host behaviour only), `VLLM_REF=main` pinned at
  `df6c7ad03194a3051a197d57ca4a94e4a063014e`, host uid/gid `1000:1000`, `CACHE_DIR=/mnt/noir/scratch/ai/vllm/cache`.
  **Corrected at T1**: every probe written during planning ran against `…dev20261010-g187a0eb`, but
  `container/.last-build` now names `…dev20261013-gdf6c7ad` — the owner rebuilt between the plan
  commit and T1. A commit-diff premise refresh cannot see an out-of-repo rebuild, so every Key Fact
  the plan depends on was re-probed on the current image before any edit (memlock, GPU, `site-packages`,
  `/root/.cache`, `Config.User`, `/home/ubuntu/.cache`), and all of them held. Gate step 5's
  `last-build == VLLM_MAIN_SHA` now passes, which it did not before the rebuild.

## Resume Instructions

1. Read `AGENTS.md`, then `context/plans/RULES.md` **in full**, then `01_uid_parity.md`.
2. Environment: docker needs `sudo -n` (socket is `srw-rw---- root printadmin`; this account is in
   no docker group) — but **after T1/T5 the scripts carry their own privilege**, so run
   `./qwen/run` and `./container/build` **as yourself**. `DOCKER=` overrides for a grouped host.
   `CACHE_DIR` default `/mnt/noir/scratch/ai/vllm/cache`; `RUN_UID` default `1000`; no GPU work in
   Wave 1.
3. Ordering: **T1 before T2, and both before any rebuild** — T1 alone is the `uid-0` trap, because
   the launchers still reach bare `docker` and get started with sudo. **T4 before asking the owner
   to rebuild** (2 min, settles the `/root`-hardcoding and mount-hiding questions while the answer
   is still free). T3 then T5–T9 finish Wave 1. T3b and T10 wait on the rebuild.
4. Commit per task as `[P1][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**. T1 additionally must carry the README § Caching rewrite (Rule B.6).
5. Restore state at session end (Rule P2.4): no leftover `uidprobe` image, no `/tmp/p*` dirs, no
   stub `docker` on PATH, launcher env unset.

## Open Unknowns

1. **Is production `$CACHE_DIR` writable by the invoker?** ~~open~~ **ANSWERED 2026-10-13 at T1: yes.**
   `test -w /mnt/noir/scratch/ai/vllm/cache` → writable by uid 1000, so the launcher can create
   `uid-1000` itself. (The risk was stated as "`install -d` fails"; T1 replaced `install -d -m` with
   `mkdir -m` for an unrelated reason, but the precheck guards both.)
2. **Is `/opt/venv/lib/python3.12/site-packages` traversable by uid 1000?** ~~open~~ **ANSWERED
   2026-10-13 at T1: yes** — `test -r …/site-packages/vllm/__init__.py` succeeds as uid 1000 inside
   the `gdf6c7ad` image. Recorded permanently as gate step **7c**, which asserts it on every commit.
3. **Does the image already ship content under `/home/ubuntu/.cache`?** ~~open~~ **ANSWERED 2026-10-13
   at T1: no** — the path does not exist in the image, so the bind mount hides nothing. T4's
   `WARN hidden-by-mount` check is therefore moot for this image, but T4 still runs to prove the
   stacked `USER` directive itself.
4. **Is `$CTX_DIR` root-owned from past sudo builds?** If yes the first unprivileged *real* build
   EACCes on `rsync --delete`, and `PREFLIGHT_ONLY` will not show it (it exits before the rsync).
   *Ask*: T5 Verify, line 4.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Pin identity in `_run-lib.sh` + README in same commit | `common/_run-lib.sh`, `README.md` | **done** | Refused-sudo + no `uid-0` proven; `--user`/`HOME`/mount proven; dir owner proven. Deviations below |
| 2 | T2: Launcher-direct `docker` → `$DOCKER` | `qwen3.8-flash-next/run`, `muse/run`, **`qwen3.8-flash-next/run-legacy`** | pending | :279 swallows failure today. **Scope gap: `run-legacy:119` is a live bare-`docker` call in a file this plan's T2 list omits** — it sources `_run-lib.sh`, so it inherits T1 and must not be left half-converted |
| 3 | T3: `USER 1000:1000` in runtime stage | `container/Dockerfile` | pending | Edit verifiable now; assertion is T3b |
| 4 | T3b: Image `Config.User` assertion | (built image) | blocked: awaiting owner rebuild | |
| 5 | T4: Prove `USER` on the current image | throwaway `uidprobe` layer | pending | Do before asking for the rebuild |
| 6 | T5: `container/build` unprivileged + root guard + legacy-ctx preflight | `container/build` | pending | |
| 7 | T6: `container/archs` honours `DOCKER` | `container/archs` | pending | |
| 8 | T7: Gate step 7, named skip pre-rebuild | `context/plans/RULES.md` | **invalid — folded into T1** | Rule A.5 forces the gate update into the commit that adds the behaviour, and T1's own Verify line demands "gate steps … **and 7**", so T7 could not verify itself as written. Step 7 shipped as 7a/7b/7c in T1's commit; `:226` rewritten there too. **T7's remaining scope is nothing** — re-open only if 7a/7b/7c need rework |
| 9 | T8: Docs + correct stale `sudo` notes + harvest | `AGENTS.md`, `README.md`, `context/*` | pending | |
| 10 | T9: `container/cache-migrate` (opt-in) | `container/cache-migrate` | pending | No sudo needed |
| 11 | T10: Runtime confirmation (gate step 6) | — | blocked: awaiting owner rebuild | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

**Mandatory in this project** (Rule P1): its product is a number, and a number without provenance
gets cited. A claim not in this table has no provenance and cannot be cited by a later plan.

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-13 | `find $CACHE_DIR -maxdepth 4 -printf '%u\n' \| sort \| uniq -c` (pre-work baseline) | 1102 root / 117 iphands | 1 | n/a — an ownership census, not a timing |
| 2026-10-13 | `docker run --user 1000:1000 --gpus device=0 … nvidia-smi -L` | GPU visible to uid 1000 | 1 | Pre-T3 image; the plan's central premise |
| 2026-10-13 | `docker run --user 1000:1000 … ls -ld /root/.cache` | `Permission denied` | 1 | Why the mount target must move |
| 2026-10-13 | `du -sh $CACHE_DIR` | **no result — exceeded 10 min, killed.** Superseded by the `find` census below | 3 attempts | `du` is unusable on this NFS mount; do not re-run it, use `find -printf '%s'` |
| 2026-10-13 | `find $CACHE_DIR -path "$CACHE_DIR/uid-*" -prune -o \( -user root -print \| -type f -printf '%s\n' \)` | **243900 root-owned entries, 205142 regular files, 33.0 GiB** | 1 | Ownership census + byte sum, not a timing. Corrects the baseline row above: `-maxdepth 4`/`-maxdepth 6` reported 1102/13027, i.e. **up to 19× low** |
| 2026-10-13 | `docker run --user 1000:1000 --ulimit memlock=-1 --gpus device=0 -e HOME=/home/ubuntu … 'resource.getrlimit(RLIMIT_MEMLOCK); torch pin_memory 2GiB'` | `soft,hard = (-1, -1)`, `pinned ok: True bytes: 2147483648` | 1 | **Refutes Risk 3 as drafted.** `--user` does not cost the 95.4 GiB PLE pin. Now asserted on every commit by gate step 7c |
| 2026-10-13 | T1 Verify against a stub `docker`, `CACHE_DIR=/tmp/p1cache`, `MODEL_ROOT=/tmp/p1models` | emitted line carries `--user 1000:1000`, `HOME=/home/ubuntu`, `…/uid-1000:/home/ubuntu/.cache:rw`; `stat -c %u` → `1000`; `sudo -n … ./qwen/run` → exit 1, **no `uid-0` created** | 1 | Cold banner printed once, silent on warm boot; `/root/.cache` occurrences in the emitted line: 0 |
| 2026-10-13 | `chmod 700 uid-1000` then boot; and a root-owned `uid-1000` planted in a parent I own | mode **stays 700** across a boot; foreign-owned dir → refused, **0 docker calls reached**, mode/owner untouched | 1 | Proves `mkdir -m` ≠ `install -d -m` (measured: `install -d -m 0750` on a 0700 dir → **750**) |
| 2026-10-13 | Project gate, all steps, on the T1 tree | 1a pass · **1b `27 b` / `24 p`** · 1c `env ok` · **2 `SKIP(shellcheck not installed)`** · 3 `4/4 ok` · 4 `## preflight OK` ×2 · 5 `last-build == df6c7ad…`, `1` image, arch table `_vllm_fa2_C sm_120 x76` no sm_80 SASS · 7a `PASS`×5 · **7b `SKIP`** · 7c `PASS(7c) 1000 unlimited readable` | 1 | Steps 2 and 7b are **named skips, not passes**: shellcheck is absent from this host, and the image predates T3 |

Record the build every row was taken on: image tag, `ai.vllmcustom.vllm.ref`, and the launcher
env. `bench/bench-results.md` is gitignored — it is not evidence a later plan can cite.

**This plan's headline metric is an ownership census, not a throughput number.** The success
assertion is "zero new root-owned files after a full build and a serve" (T10). Decode throughput
is explicitly **not** measured here, and first-boot cold-cache wall time must never be quoted as a
regression (see T10's "within noise").

## Notes / Deviations

- **The plan draft originally derived the cache directory from `$(id -u)` while pinning
  `--user 1000:1000`.** Both reviewers flagged it as a blocking bug: under `sudo ./qwen/run` — the
  only way launchers work today, since they call bare `docker` — that creates root-owned `uid-0`,
  which the uid-1000 container then cannot write. Rewritten to a single `RUN_UID` constant before
  implementation, and T1's Verify now asserts the sudo run is *refused* and that no `uid-0` appears.
- **The draft also deferred all docs to T8**, which violates Rule B.6 for T1's path change; README
  § Caching moved into T1's commit.
- **T1 shipped five deliberate departures from its own Before/After block**, all reviewer-driven and
  all verified rather than reasoned-about:
  1. **`install -d -m 0755` → `mkdir -m 0750`.** `install -d -m` chmods a directory that *already
     exists* (measured: a `0700` dir becomes `0750`), so the plan's form would have widened a cache
     tree the owner deliberately locked, on every single boot. `mkdir` sets the mode only when it
     creates. The `elif [[ ! -d ]]` is what keeps `mkdir`'s swallowed error diagnosable.
  2. **Ownership asserted on uid alone, not `uid:gid`.** The plan asserted `== 1000:1000`, which
     assumes the invoker's *primary group* is gid 1000. True here; false on a host where it is not,
     where the launcher would refuse a perfectly usable directory and tell you to "run as that uid"
     — advice you cannot follow. Write access comes from the uid.
  3. **The sudo guard moved from `docker_core_args` to file scope.** Every launcher touches the socket
     first (`qwen3.8-flash-next/run` resolves its image by label, `require_image` inspects), so the
     plan's placement was reachable only *after* a docker call had already failed with the misleading
     "image not found" this project's own `pitfalls.md` names.
  4. **The cold banner dropped its `du -sh` instruction and its `cache-migrate` line.** The README
     documents that `du` never returns on this mount, so telling a waiting user to run it is telling
     them to hang their terminal; and `./container/cache-migrate` is T9 and does not exist yet. The
     estimate is now labelled `(unmeasured)` and paired with a liveness signal instead of a bare timer.
  5. **README cites a `find` census, not the `du -sh` figure the plan asked for.** `du` was attempted
     three times and never returned; `find -printf '%s'` completed in under 300 s and answered the
     actual question (33.0 GiB). Rule E.2: the measurement that exists beats the one the plan wanted.
- **Gate step 1b's documented `27 b / 24 p` could not be reproduced — and the snippet, not the tree,
  was wrong.** It classified by shebang only, so it silently skipped 16 shebang-less `.py` modules and
  the three sourced libs that open with `# shellcheck shell=bash`, reporting `24 b / 8 p` against a
  green run. Adding the extension fallback restores `27 b / 24 p` exactly, which proves the numbers
  were right and the snippet had drifted. **Lowering the documented numbers to `24 b / 8 p` would have
  laundered a coverage hole into the baseline** — that was the explicit advice, and it is why T1 fixed
  the classifier instead of the bar.
- **Risk 7 (`bench/`, `scripts/*`) decided: out of T1, into Follow-ups.** `bench/_bench-lib.sh:6,20`
  and `bench/bench-wrapper:85,89` call bare `docker`, so `crash-*.log` on a failed load is already
  empty today and stays empty. Pre-existing, reads logs rather than files, and `Out` names it — but it
  is the same root cause as `$DOCKER`, so it is recorded rather than argued away.
- **A live parser *was* broken by T1 and is fixed in the same commit**: `repin.md`'s stub step grepped
  `^docker `, and the new `sudo -n docker` prefix means the stub is bypassed entirely — the documented
  step would have booted a real 122B model while appearing to suppress the launch. The step now sets
  `DOCKER=docker`, the stub answers `image inspect` itself, and the anchor accepts an optional
  privilege prefix. Verified: launch suppressed, `require_image` satisfied, no socket touched.
- **The draft claimed T3 could not be verified until the multi-hour rebuild.** Wrong — only
  `Config.User` needs the rebuild; T4 proves the rest on the current image in ~2 minutes.
- **`--ulimit memlock=-1` was drafted as a risk.** Reviewed and dismissed: runc applies ulimits as
  root at container init, before the uid drop. Risk 4 removed from the plan; recorded here so the
  reasoning is not lost.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| `container/push` honours `DOCKER` | Out of scope (needs `docker login`, writes no files) but still bare `docker` | Plan 02 or a drive-by |
| `bench/_bench-lib.sh` + `scripts/*` (`docker logs`/`ps`/`rm`) | A failed boot stops printing its crash log when run unprivileged | Plan 02 |
| Reclaim the abandoned root-owned cache (`du -sh` + one command) | Owner chose to leave it; the disk is still theirs to reclaim | Plan 02, owner-decided |
| Persistent `pip install` in the running container | `docker exec` now lands as uid 1000 with an unmounted `~`; today's root flow let it persist in `/opt/venv`. Consider `-e PIP_USER_BASE=/home/ubuntu/.cache/pip-user` | Plan 02 |
| `container/patches/boot-timing/__pycache__/` in `.gitignore` | Container-written bytecode inside the repo tree; regenerates as uid 1000 after T1 | T8, if not already covered |
| `repin.md:197-199` predicts MTP 3 / util 0.96; the launcher emits MTP 4 / 0.95 | Stale expectation found during the repin run, deliberately left alone | Plan 02 |
| The cold-boot banner prints **after** `[[ -w ]]` and `mkdir`, the only two calls in the launcher that can block indefinitely on a hard NFS mount — so the one boot where a stall is likeliest prints nothing | Found at T1 review; the fix is to emit the first banner line before `mkdir`, or wrap it in `timeout` and name the mount | T8 or Plan 02 |
| `require_image` still reports `Image … not found. Run ./container/build first` when the real cause is that `sudo -n` did not answer | `context/pitfalls.md` names this exact lie; T1 fixed the socket-access half by routing through `${DOCKER[@]}`, not the message. A one-call `${DOCKER[@]} version` probe at source time kills the whole class | T2 (which touches the same call sites) |
| `muse/run`'s `--selftest` path is `exec docker run` with **no `--user`**, so it runs the container as root while every other path is pinned to 1000 | Sits inside T2's file list; noted so T2 does not convert the privilege prefix and leave the identity inconsistent | T2 |
| `vllm_launch`'s echoed line is a serialization, not a quoting-preserving one: `--override-generation-config '{ … }'` re-splits if pasted | The comment now says so. Needs `printf '%q '` only if the proposed DRY_RUN golden-file test ever lands | Plan 02, with that test |
