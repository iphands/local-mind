# Plan 13 — Repin Basalt to v0.5.5 and rebuild the image — Tracker

## Overview

- Status: 0% complete (0/3 tasks)
- Start date: 2026-11-21
- Plan: `context/plans/13_basalt_repin_v055.md`
- Evidence: the table below; build logs in `captures/build_<date>_<sha7>.log` (gitignored); boot
  start blocks pasted by the owner. Test environment: host `cosmo`, RTX PRO 6000 (GPU 0), the packs
  on NFS (`/mnt/noir/scratch/ai/llm/models/vllm/jesdga/`).

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), then `13_basalt_repin_v055.md`.
2. Environment: docker as yourself (no `sudo`); `shellcheck` 0.11.0 on `PATH`; `/mnt/noir` mounted.
   The gate's scratch files default to `/tmp/opencode/`, which does not exist in every shell — create
   it or point the two paths at a directory that does. Check `id -nG` lists `docker` before claiming
   steps 3–6. `GIT_NO_LAZY_FETCH=1` on every probe of `vendor/basalt` (a `blob:none` clone).
3. Ordering: T1 → T2 → T3. **T1 commits nothing until the owner's `./build` has produced the image**
   — gate steps 5, 5b and 6 fail with no image for `BASALT_SHA`. T1 step 3 and all of T2 are the
   owner's to run; mark the task `blocked` (with the reason) while it waits.
4. Commit per task as `[basalt][P13][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**. Never push.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: repin to `ff0b978`, preflight, owner build, gate on the new image | `versions.env`, `README.md` | blocked | **Waits on the owner's `./build` (2026-11-21).** Steps 1–2 (the `versions.env` edit, the agent's `PREFLIGHT_ONLY=1 ./build`, the sweep of the patch's callers, gate 1–4) are made right after this commit; `versions.env` then stays uncommitted until the image exists, because gate steps 5, 5b and 6 fail with no image for `BASALT_SHA`. Owner: `./build` → `iphands/basalt-blackwell:cu1343-sm120-ff0b978`. Then steps 4–5: gate 1–6 with 5b on that image, `README.md`, this row, the commit. dirty: `versions.env` (the agent's, in progress). |
| 2 | T2: owner boots, `./run` and `./run-hy4`, a real reply each | tracker Evidence | pending | |
| 3 | T3: harvest and close | `context/` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-21 | project gate 1–6 with 5b, old pin `ad274aa`, image `iphands/basalt-blackwell:cu1343-sm120` (id `4de9f15bb9d2`, labels `ai.basalt.ref=ad274aa8…`, `ai.basalt.patches=0001-buffered-io-switch.patch:a3553ec6bb8c`), before the plan commit | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=6 AUDIT_B=5 TOTAL_B=11`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 157 engine flags known)`; `pack flags ok` ×3 (`./run`: `qwen4exp takes --max-context 362144 --kv int8 --ple-io ram --batch 3`; `./run --single`: the same without `--batch 3`; `./run-hy4`: `hy4flash takes --max-context 362144 --kv fp16`); `imports ok (42 serve modules)` | 1 | the baseline T1 is compared against; `shellcheck` 0.11.0 at `/usr/local/bin` in this shell; gate scratch paths pointed at the session's scratch directory. Run while the owner's `basalt` container was up (14 hours, the same image id); the dry runs report a busy port and do not refuse |
| 2026-11-21 | `git ls-remote --tags --heads https://github.com/jesdga95/basalt.git`, filtered to `v0.5` and `main` | `refs/tags/v0.5.5` = `3cfb5ef4…`, `refs/tags/v0.5.5^{}` = `ff0b978d2ee9ff195d113b6feb80961b2d2f99ef`; `main` = `97e2e711…` | 1 | verbatim in the plan § *Pre-Identified Bug/Issue* |
| 2026-11-21 | agent: `git -C vendor/basalt fetch --quiet origin`, then reads of the fetched objects with the checkout left at `ad274aa` (`OLD`; `NEW` = `ff0b978`): `rev-list --count` both ways, `merge-base`, `log -1`, `tag -l 'v0.5*'`, `cat-file -p v0.5.5`; `rev-parse OLD:<f>` against `NEW:<f>` for nine files; `diff --no-renames --name-only OLD NEW` counted by directory; `diff OLD NEW -- CMakeLists.txt cmake/models.cmake docs/building.md serve/config.py src/models/qwen4exp/flags.cpp`; `show NEW:docs/changelog/0.5.5.md`, `NEW:tests/models/qwen4exp/cli/kv_batch_test.cpp`; `grep -n` in `NEW:serve/config.py` and `NEW:serve/engine/vision.py` | 177 ahead, 0 behind, merge base `ad274aa`; `ff0b978… 2026-11-21 Merge Inkling 2 Mini support (Basalt 0.5.5)`; tags `v0.5.0`, `v0.5.5` in the clone, the `v0.5.5` object unsigned. Same blob at both pins: `Dockerfile`, `.dockerignore`, `requirements.txt`, `tools/model_file.py`, `LICENSE`, `THIRD_PARTY_LICENSES`, and the patch's three files (`include/basalt/platform/file.hpp`, `src/platform/direct_file.cpp`, `src/platform/file.cpp`). 277 files changed. The rest is the plan's Key Facts table, row by row | 1 | after the fetch `git -C vendor/basalt status --porcelain --untracked-files=all` is empty and `HEAD` is still `ad274aa`. Reads of named files fetched those blobs on demand; every other probe ran under `GIT_NO_LAZY_FETCH=1` (a rename-detecting `diff --name-status` refused under it, hence `--no-renames`) |

## Notes / Deviations

- **2026-11-21 — the agent fetched `origin` into `vendor/basalt` before the plan was written**, where
  Plan 10 left the fetch to the owner's preflight and pre-read through a web tool that paraphrased
  (and got one fact wrong, Plan 10 tracker Notes). This shell is in the `docker` group and runs gate
  step 3 itself, and that step makes the same fetch. Objects only: no checkout, no file in the tree
  changed.
- **2026-11-21 — the image for the old pin is not the one Plan 10 recorded.** `cu1343-sm120-ad274aa`
  is `4de9f15bb9d2`, created 2026-11-20 21:59:37 PST and labelled with the patch; Plan 10's rows name
  `2c2253095d35`. A rebuild after Plan 12 T1, then: Plan 12's tracker still lists T3 (the rebuild and
  gate step 6b) as `pending`, and `README.md` and `SERIES.md` still say the image on disk has no
  patch. Not edited here (plan § *Scope*) beyond the README sentence T1 step 5 rewrites; a Follow-ups
  row carries it. The baseline's `157 engine flags known` is this image's; Plan 10 recorded 158 on
  the unpatched build of the same commit, and the difference was not looked into.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Plan 12 T3: record the patched build and add gate step 6b | the owner's rebuild of 2026-11-20 21:59 is unrecorded, and the v0.5.5 build will be a second patched image; 6b can be written against either | Plan 12's tracker |
| Plan 08 T3/T5, Plan 10 T2/T3, Plan 11 T3/T4, Plan 12 T4/T5: boots and closes still open | each waits on an owner boot; once the pin moves a plain launcher boots the v0.5.5 image, so a boot of an older image needs `IMAGE_OVERRIDE`, or the row a `skipped` with the reason | owner decision; each plan's tracker |
| Re-target Plan 09 (audit) to `ff0b978` plus `patches/`; fix the lines in `context/AGENTS.md` and `context/audit/index.md` that call `d3b7574` the built pin | stale since Plan 10 T1, one release further now | owner decision; `SERIES.md` row 09 |
| A launcher for Inkling 2 Mini (`inkling2`, a 19.7 GB pack) | the image will hold a fifth model that no launcher serves; a launcher profile is a plan | owner decision; a new plan if wanted |
| Build only the served models (`-DBASALT_MODELS="qwen4exp;hy4flash"`) | five models are compiled and shipped, two are served; a build-flag change is a plan | owner decision; a new plan if wanted |
| `--api-key-file` in the launchers | new server option at v0.5.5: it keeps the key out of the process list, where `--api-key` puts it | a later plan (a launcher default) |
| Citations of upstream docs that v0.5.5 deleted | `docs/models/hy4-flash/{serve,kv,pack,tier,vision,dfly2}.md` are gone (folded into `docs/models/hy4-flash.md`); Plan 11's Key Facts cite `serve.md` (`grep -n 'hy4-flash/' context/distilled.md README.md` prints nothing) | a note in Plan 11's tracker when it is next edited |
