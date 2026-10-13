# Plan 01 — Run everything as uid/gid 1000:1000, host and container — Tracker

## Overview

- Status: 0% complete (0/10 tasks; T3b and T10 start `blocked`)
- Start date: — (set when T1 starts)
- Plan: `context/plans/01_uid_parity.md`
- Evidence: `context/plans/01_uid_parity.md` § Context (the 2026-10-13 probe transcripts), each
  task's Verify transcript pasted into this tracker's Notes, and — from T10 onward — the launcher's
  echoed `docker … serve …` line plus the boot report. **Test environment for every row**: image
  `iphands/vllm-blackwell:cu1321-sm120-main-vllm0.32.0.dev20261010-g187a0eb` (Wave 1 — the *pre*-T3
  image, which is deliberate: Wave 1 changes host behaviour only), `VLLM_REF=main` pinned at
  `df6c7ad03194a3051a197d57ca4a94e4a063014e`, host uid/gid `1000:1000`, `CACHE_DIR=/mnt/noir/scratch/ai/vllm/cache`.

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

1. **Is production `$CACHE_DIR` writable by the invoker?** The 1102/117 probe never answered who owns
   the top directory. If it is root-owned, `install -d` fails on every launch.
   *Ask*: T1 Verify line 1 (`test -w /mnt/noir/scratch/ai/vllm/cache`).
2. **Is `/opt/venv/lib/python3.12/site-packages` traversable by uid 1000?** Only the *file* mode of
   `bin/vllm` was probed; every prior run was root, so a 0700 directory would have gone unnoticed.
   *Ask*: T4.
3. **Does the image already ship content under `/home/ubuntu/.cache`?** If yes the mount hides it.
   *Ask*: T4's `WARN hidden-by-mount` line.
4. **Is `$CTX_DIR` root-owned from past sudo builds?** If yes the first unprivileged *real* build
   EACCes on `rsync --delete`, and `PREFLIGHT_ONLY` will not show it (it exits before the rsync).
   *Ask*: T5 Verify, line 4.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Pin identity in `_run-lib.sh` + README in same commit | `common/_run-lib.sh`, `README.md` | pending | The `uid-0` guard is the point of this task |
| 2 | T2: Launcher-direct `docker` → `$DOCKER` | `qwen3.8-flash-next/run`, `muse/run` | pending | :279 swallows failure today |
| 3 | T3: `USER 1000:1000` in runtime stage | `container/Dockerfile` | pending | Edit verifiable now; assertion is T3b |
| 4 | T3b: Image `Config.User` assertion | (built image) | blocked: awaiting owner rebuild | |
| 5 | T4: Prove `USER` on the current image | throwaway `uidprobe` layer | pending | Do before asking for the rebuild |
| 6 | T5: `container/build` unprivileged + root guard + legacy-ctx preflight | `container/build` | pending | |
| 7 | T6: `container/archs` honours `DOCKER` | `container/archs` | pending | |
| 8 | T7: Gate step 7, named skip pre-rebuild | `context/plans/RULES.md` | pending | Rewrite :226, don't delete |
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
| 2026-10-13 | `du -sh $CACHE_DIR` | *fill in at T8* | 1 | The number Risk 1 and README § Caching both cite |

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
