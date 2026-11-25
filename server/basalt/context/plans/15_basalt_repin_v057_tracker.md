# Plan 15 — Repin Basalt to v0.5.7 and rebuild the image — Tracker

## Overview

- Status: 0% complete (0/3 tasks)
- Start date: 2026-11-25
- Plan: `context/plans/15_basalt_repin_v057.md`
- Evidence: the table below; build logs in `captures/build_<date>_<sha7>.log` (gitignored); boots
  read by the agent from the owner's running container. Test environment: host `cosmo`, RTX PRO 6000
  (GPU 0), the packs on NFS (`/mnt/noir/scratch/ai/llm/models/vllm/jesdga/`).

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), then `15_basalt_repin_v057.md`.
2. Environment: docker as yourself (no `sudo`); `shellcheck` 0.11.0 on `PATH`; `/mnt/noir` mounted.
   The gate's scratch files default to `/tmp/opencode/`, which does not exist in every shell — create
   it or point the two paths at a directory that does. Check `id -nG` lists `docker` before claiming
   steps 3–6. `GIT_NO_LAZY_FETCH=1` on every probe of `vendor/basalt` (a `blob:none` clone).
3. Ordering: T1 → T2 → T3. **T1 commits nothing until the owner's `./build` has produced the image**
   — gate steps 5, 5b and 6 fail with no image for `BASALT_SHA`. T1 step 3 and all of T2 are the
   owner's to run; mark the task `blocked` (with the reason) while it waits.
4. Commit per task as `[basalt][P15][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**. Never push.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: repin to `400b411`, preflight, owner build, gate on the new image | `versions.env`, `README.md` | pending | |
| 2 | T2: owner boots, `./run-ink`, `./run`, `./run-hy4`, requests answered | tracker Evidence | pending | |
| 3 | T3: harvest and close | `context/` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-25 | project gate 1–6 with 5b, old pin `ff0b978`, image `iphands/basalt-blackwell:cu1343-sm120` (id `a3441f94bdf1`, labels `ai.basalt.ref=ff0b978d…`, `ai.basalt.patches=0001-buffered-io-switch.patch:a3553ec6bb8c`), before the plan commit | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=7 AUDIT_B=5 TOTAL_B=12`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 159 engine flags known)`; `pack flags ok` ×4 (`./run`: `qwen4exp takes --max-context 362144 --kv int8 --ple-io ram --batch 3`; `./run --single`: the same without `--batch 3`; `./run-hy4`: `hy4flash takes --max-context 362144 --kv fp16`; `./run-ink`: `inkling2 takes --max-context 362144 --kv fp16`); `imports ok (44 serve modules)` | 1 | the baseline T1 is compared against; gate scratch paths pointed at the session's scratch directory. Run while the owner's `basalt` container served Inkling 2 Mini (three days up, the same image id); the dry runs report a busy port and do not refuse |
| 2026-11-25 | `git ls-remote --tags --heads https://github.com/jesdga95/basalt.git`, filtered to `v0.5` and `main` | `refs/tags/v0.5.7` = `cea3989c…`, `refs/tags/v0.5.7^{}` = `400b411c58031d064812ea21a259b2381acdaa3e`; `refs/tags/v0.5.6^{}` = `1e205ae8…`; `main` = `f71a5875…` | 1 | verbatim in the plan § *Pre-Identified Bug/Issue* |
| 2026-11-25 | agent: `git -C vendor/basalt fetch --quiet origin`, then reads of the fetched objects with the checkout left at `ff0b978` (`OLD`; `NEW` = `400b411`): `rev-list --count` both ways and through `1e205ae`, `merge-base`, `log -1`, `tag -l 'v0.5*'`, `cat-file -p v0.5.7`; `rev-parse OLD:<f>` against `NEW:<f>` for 22 files (three differ: `CMakeLists.txt`, `serve/config.py`, `serve/engine/vision.py`); `diff --no-renames --name-only OLD NEW` counted by directory, and `--name-status` under `serve`, `src/platform`, `src/core`, `src/ngram`, `src/vision`, `src/models`, `data`, `tools`; `diff OLD NEW -- CMakeLists.txt serve/config.py serve/engine/vision.py src/models/qwen4exp/flags.cpp src/models/hy4flash/adapter.cpp`; `show NEW:docs/changelog/0.5.6.md`, `NEW:docs/changelog/0.5.7.md`, `NEW:docs/models/inkling2.md`; `grep -n` in `NEW:serve/config.py`, `NEW:serve/engine/vision.py`, `NEW:serve/totals.py`, `NEW:src/models/inkling2/adapter.cpp`, `NEW:CMakeLists.txt` | 251 ahead (134 to v0.5.6, 117 more), 0 behind, merge base `ff0b978`; `400b411… 2026-11-23 0.5.7: changelog and index row (…); the version is 0.5.7`; tags `v0.5.0`, `v0.5.5`, `v0.5.6`, `v0.5.7` in the clone, the `v0.5.7` object unsigned. Same blob at both pins: `Dockerfile`, `.dockerignore`, `requirements.txt`, `tools/model_file.py`, `LICENSE`, `THIRD_PARTY_LICENSES`, `cmake/models.cmake`, `docs/building.md`; the patch's three files; `src/core/loader.cpp`, `model_file.cpp`, `conversation_store.cpp`, `file_reads.cpp`, `expert_source.cpp`, `src/vision/mmproj.cpp`, `src/ngram/ple_reader.cpp`; `tests/models/qwen4exp/cli/kv_batch_test.cpp`. Nothing changed under `src/platform` or `data`. 516 files changed. The rest is the plan's Key Facts table, row by row | 1 | after the fetch `git -C vendor/basalt status --porcelain --untracked-files=all` is empty and `HEAD` is still `ff0b978`. Reads of named files fetched those blobs on demand; every other probe ran under `GIT_NO_LAZY_FETCH=1` |

## Notes / Deviations

- **2026-11-25 — two releases in one repin.** The owner named 0.5.7; v0.5.6 was tagged the same day
  (2026-11-23) and is an ancestor of it. Both sets of release notes were read; no image is built
  for v0.5.6.
- **2026-11-25 — the agent fetched `origin` into `vendor/basalt` before the plan was written**, as in
  Plan 13: objects only, no checkout, no file in the tree changed. Gate step 3 makes the same fetch.
- **2026-11-25 — Plan 14 T4 was recorded first**, from the Inkling session that was still up
  (commit `5fe2fca`), because a boot on the new image replaces the container and its `docker logs`.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| `KV=int8` in `./run-ink` | the engine takes `--kv int8` for Inkling 2 Mini from v0.5.6 on (18 KiB a position against fp16's 32); the launcher refuses the value and gate step 4 asserts that refusal | owner decision; a small launcher change with its gate assertion and README row |
| The MX8 Inkling pack (`Inkling-2-Mini-NVFP4-MX8.basalt`, 19.6 GB, sha256 `28ec7072…1b99` in upstream's guide) | upstream's default from v0.5.6 on, "about 5% faster" on prompts; ours is the 0.5.5 pack. A download is the owner's; `./run-ink`'s default pack and the README's hash follow it | owner decision |
| `PARALLEL` above 1 for Inkling 2 Mini | v0.5.6 batches slots on every model ("Eight parallel slots at 4K keep every expert on the card"); Plan 14's follow-up said upstream had not measured it | Plan 05 (tuning), if wanted |
| Plan 13 T2's missing `./run-hy4` boot on the v0.5.5 image, and Plan 13 T3 | once the pin moves a plain `./run-hy4` boots v0.5.7; the v0.5.5 boot needs `IMAGE_OVERRIDE`, or the row a `skipped` with the reason | owner decision; Plan 13's tracker |
| Plan 14 T5 (harvest and close) | T4 is recorded; the harvest is agent work that needs a green gate, so it follows T1 here | Plan 14's tracker |
| Plan 12 T3 (record the patched build, gate step 6b); Plan 08 T3/T5, Plan 10 T2/T3, Plan 11 T3/T4, Plan 12 T4/T5 | carried from Plan 13's Follow-ups, unchanged | owner decision; each plan's tracker |
| Re-target Plan 09 (audit) to `400b411` plus `patches/` | stale since Plan 10 T1, three releases further now | owner decision; `SERIES.md` row 09 |
| `--api-key-file` in the launchers; building only the served models | carried from Plan 13's Follow-ups | a later plan each |
| Prometheus `/metrics`, persistent totals (`--totals-file`) | new server features at v0.5.7 that the launchers do not turn on; totals need a file path that survives the container | owner decision; a later plan if wanted |
