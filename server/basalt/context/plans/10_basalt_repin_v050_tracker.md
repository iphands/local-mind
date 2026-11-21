# Plan 10 — Repin Basalt to v0.5.0 and rebuild the image — Tracker

## Overview

- Status: 33% complete (1/3 tasks)
- Start date: 2026-11-20
- Plan: `context/plans/10_basalt_repin_v050.md`
- Evidence: the table below; build logs in `captures/build_<date>_<sha7>.log` (gitignored); boot
  start blocks pasted by the owner. Test environment: host `cosmo`, RTX PRO 6000 (GPU 0), the pack on
  NFS (`/mnt/noir/scratch/ai/llm/models/vllm/jesdga/`).

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), then `10_basalt_repin_v050.md`.
2. Environment: docker as yourself (no `sudo`); `shellcheck` 0.11.0 on `PATH`. The gate's scratch
   files default to `/tmp/opencode/`, which does not exist in every shell — create it or point the
   two paths at a directory that does. Check `id -nG` lists `docker` before claiming steps 3–6.
   `GIT_NO_LAZY_FETCH=1` on every git command against `vendor/basalt` (a `blob:none` clone).
3. Ordering: T1 → T2 → T3. **T1 commits nothing until the owner's `./build` has produced the image**
   — gate steps 5–6 fail with no image for `BASALT_SHA`. T1 steps 2 and 4 and all of T2 are the
   owner's to run; mark the task `blocked` (with the reason) while it waits.
4. Commit per task as `[basalt][P10][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**. Never push.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: repin to `ad274aa`, owner build, read the tree, gate on the new image | `versions.env`, `README.md` | done | `BASALT_SHA` is `ad274aa8dc243f0fdcc1ad382f2e31c4591b4552`; the owner's `./build` made image `2c2253095d35` (`cu1343-sm120-ad274aa`, label `ai.basalt.ref` = the full SHA). The recipe needed **no change**: `Dockerfile`, `.dockerignore`, `requirements.txt`, `docs/building.md` and `serve/config.py` are identical at both pins (absent from `git diff d3b7574 ad274aa`), so `Dockerfile` and `Dockerfile.dockerignore` are untouched and gate steps 5–6 ran unchanged. `--ple-io`, the flag the pre-read could not find, is accepted (step 5). Gate 1a/1b/1c/2/2b/3/4/5/6 pass on the new image; step 5's negative control fires — Evidence rows 2–7. `README.md` names the new pin and tag. Boots are T2. |
| 2 | T2: owner boots, default and `--single`, a real reply each | tracker Evidence | blocked | **Half recorded; waits on the owner (2026-11-20).** The default boot is in: `./run`'s `==` lines and the start block, on the new image, engine 0.5.0, `3 slots × 362144` accepted — Evidence row 8. Still needed: one `/v1/chat/completions` reply from that boot as `cosmo-6000` (none had been served when the logs were read), then `./run --single` with its `==` lines, start block and a reply. |
| 3 | T3: harvest and close | `context/` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-20 | project gate 1–6, old pin `d3b7574`, image `iphands/basalt-blackwell:cu1343-sm120` (id `41619e9056f8`, the `d3b7574` build), before the plan commit | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=4 AUDIT_B=5`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 153 engine flags known)`; `imports ok (40 serve modules)` | 1 | the baseline T1 is compared against; `shellcheck` 0.11.0 at `/usr/local/bin` in this shell; gate scratch paths pointed at the session's scratch directory |

| 2026-11-20 | owner: `PREFLIGHT_ONLY=1 ./build` (pasted) | fetched 280 objects, 1.45 MiB; `== vendor/basalt at ad274aa8dc243f0fdcc1ad382f2e31c4591b4552, clean`; both CUDA digests resolve; `Check complete, no warnings found.`; `preflight ok` | 1 | the build command it printed tags `iphands/basalt-blackwell:cu1343-sm120-ad274aa`, `JOBS=16` |
| 2026-11-20 | tree read, `GIT_NO_LAZY_FETCH=1 git -C vendor/basalt …`: `rev-parse HEAD`; `rev-list --count d3b7574..ad274aa` and the reverse; `diff --stat d3b7574 ad274aa -- Dockerfile .dockerignore requirements.txt CMakeLists.txt cmake docs/building.md serve/config.py data`; `diff --name-only d3b7574 ad274aa` | HEAD = the pin, tree clean; 206 ahead, 0 behind; of the named paths only `CMakeLists.txt` (190 lines), `cmake/models.cmake` (1 line: `hy4flash` joins `BASALT_BUILTIN_MODELS`) and five new `data/` files (`data/profiles/hy4flash-42x512.prof`, four `data/bench/hy4flash/*.ids`) differ; 285 files changed in all | 1 | `CMakeLists.txt`: `project` version 0.5.0, a new option `PEER_ADA` (default `OFF`), no new `find_package`, `cmake_minimum_required` still 3.24, C++20. `serve/config.py` defines `engine_flags_of` :32, `server_options` :42, `build_parser` :519. `.gitignore` lost two Windows-notes lines only |
| 2026-11-20 | owner: `./build` (JOBS = 16); log `captures/build_2026-11-20_ad274aa.log` (570 lines) | the cmake configure + build step is `#12 DONE 307.9s`, 427 ninja targets; image created 20:00:11 | 1 | v0.4.1: 319.6 s, 407 targets. Four models built (upstream's default `BASALT_MODELS`); image 3.49 GB, as v0.4.1's |
| 2026-11-20 | `docker image inspect` on `iphands/basalt-blackwell:cu1343-sm120-ad274aa`; `ls` of `/opt/basalt` in it | id `sha256:2c2253095d3570e8bb60d2daf38a87143751fb4acccc5d19edafe7eef6132d9d`; labels `ai.basalt.ref=ad274aa8dc243f0fdcc1ad382f2e31c4591b4552`, `ai.basalt.cuda=13.4.3`, `ai.basalt.arch=sm_120`, `ai.basalt.repo=https://github.com/jesdga95/basalt.git`; `/opt/basalt/bin` holds `basalt`, `basalt-bench`, `basalt-probe`, `basalt-vision`; `data/profiles` holds `qwen4exp-48x512.prof`, `hy4flash-42x512.prof` and two `deepseek41` profiles | 1 | `cu1343-sm120` and `latest` inspect to the same id |
| 2026-11-20 | project gate 1–6, new pin, image `2c2253095d35` | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=4 AUDIT_B=5`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 158 engine flags known)`; `imports ok (42 serve modules)` | 1 | v0.4.1 image: 153 engine flags known, 40 serve modules. `image_for_pin` prints the moving tag, which inspects to `2c2253095d35` with `ai.basalt.ref` = the new SHA. Run while the owner's `basalt` container was up; the dry run reports a busy port and does not refuse |
| 2026-11-20 | gate step 5 with `EXTRA_ARGS=--no-such-flag` | `FAIL(flags) --no-such-flag` | 1 | the negative control fires on the new image |
| 2026-11-20 | owner: `./run` (pasted), container `basalt` started 20:05:38 PST; agent: `docker inspect -f '{{.Image}}' basalt`, `docker logs basalt`, and the session's lines in `storage/cache/basalt/aefd5d98dc650ed4/engine.log` (from line 86716) | `== image: iphands/basalt-blackwell:cu1343-sm120`; `== pack:  /mnt/noir/scratch/ai/llm/models/vllm/jesdga/Qwen3.8-FN-NVFP4-DQ6_K.basalt (131681979197 bytes, 122 GiB)`; `== mode:  3 slots × 362144 (slot-full-context), kv=int8, ple-io=ram, vision=dynamic, http://0.0.0.0:8700/v1 as 'cosmo-6000'`. Server: `cosmo-6000  ctx=362144 kv=int8 ple-io=ram batch=3 spec=8 temp=1.0 top_p=0.95 top_k=20 vision=gpu0`; `loaded   68.4GiB 196.4s`; `kv       1.3/15.8GiB cuda0 elastic slots=3`; `slots    par=3 ctx/slot=362144 (slot-full-context), others queue`; `weights  loaded, ctx=362144 images=on`; `video    off, no FFmpeg libraries found (…)`. Engine log: `session is up (engine 0.5.0)`; `expert cache 24576 slots, 63.29 GiB of VRAM`; `experts live in VRAM 63.3 GiB (100%)`; `15891 MiB of VRAM free with everything loaded`; `RAM 64.30 GiB free, guard 2.00 GiB`; `elastic KV: about 1086432 cells fit in the 3 sessions (1086432 at full context) …` | 1 | **The build that loaded:** the container's image is `sha256:2c2253095d35…`, the id whose `ai.basalt.ref` is the new SHA (row 5), and the engine names itself 0.5.0. **No reply yet:** `docker logs` holds nothing after the `video` line. No flag refused, no context cap. The n-gram table went into RAM first (`50.7/50.7 GiB`, `PLE table locked in RAM (--ple-io ram) in 52.0 s`); the 23.53 GiB page-locked expert complement was ready in 138 s (`24.21 GB read in the start's pass`), where the last v0.4.1 session copied it in 6 s; that session left 16153 MiB of VRAM free. Two warnings, both also in the v0.4.1 sessions' log or expected: no AVX-512 on this CPU; `the device-planned MoE tail is off: every expert is resident` |

## Notes / Deviations

- **2026-11-20 — the pre-read of v0.5.0 is paraphrase.** Upstream's changelog, `Dockerfile`,
  `requirements.txt`, `serve/config.py` and two docs pages were read at the tag through a fetch tool
  that returns summaries and short quotes, not file bytes; the compare API's file list was read only
  in part. The plan's Key Facts mark each such row. Nothing was fetched into `vendor/basalt`.
- **2026-11-20 — the v0.4.1 image has served.** `storage/cache/basalt/aefd5d98dc650ed4/engine.log`
  holds four `session is up (engine 0.4.1)` lines (lines 81588–86137 of 86715; file last written
  2026-11-19 10:05), one followed by a `stall report (engine 0.4.1)` at prompt token 0. That is not
  Plan 08 T3's evidence (no `./run` `==` lines, no quoted reply, modes unknown) and is not recorded
  there; it is noted here because the pin now moves on.

- **2026-11-20 — a Key Fact this plan wrote was wrong: "none seen under `src/core` or `src/kernels`".**
  The local diff lists 10 files under `src/core` and 8 under `src/kernels` (and 43 under `include/`,
  where `core/direct_upload.hpp` is deleted and `core/elastic_kv.hpp`, `core/loader.hpp` are new). The
  claim came from the partly-read compare API list and was marked unverified; the row's other figure
  holds (`src/models/qwen4exp`: 34 files). Corrected in the plan's Key Facts. `distilled.md` cites
  `include/basalt/core/direct_upload.hpp:1-6` at `dc047a77`; that file no longer exists at the pin —
  for T3's harvest.
- **2026-11-20 — the owner started `./build` right after the preflight, before the agent's step 3
  read.** Harmless: the read found nothing for the recipe to follow, and no recipe file was edited.
- **2026-11-20 — the `v0.5.0` tag object is not in the clone** (`git -C vendor/basalt tag -l v0.5.0`
  prints nothing, after the owner's fetch): `build` fetches `origin` without tags reaching it. Open
  Question 6 stays deferred to the audit plan.
- **2026-11-20 — the moving tags point at the new image, and the owner reports it published.**
  `cu1343-sm120` and `latest` inspect to `2c2253095d35`; the owner said "Build done an published". The
  agent did not run `./publish` and did not check the registry.

- **2026-11-20 — `README.md` and `run`'s header give the PLE n-gram table as "28.8 GB … ~27 GiB"; the
  engine reads 54400261120 bytes (50.7 GiB) of it into RAM.** Not new at this pin: every `basalt load:
  ngram 0 54400261120` line in the engine log carries the same total, the earliest in a 0.2.1 session.
  Not edited here — which figure the README meant is not established; a Follow-ups row carries it.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Plan 08's open rows: T3 (boots of the `d3b7574` image) and T5 (harvest and close) | owner decision 2026-11-20: later. Once the pin moves, a plain `./run` boots the v0.5.0 image, so T3 as written needs `IMAGE_OVERRIDE` or a `skipped` with the reason | owner decision; Plan 08's tracker |
| Re-target Plan 09 (audit) from `d3b7574` to `ad274aa`; fix `context/AGENTS.md` (the `audit/` row) and the note opening `context/audit/index.md`, which call `d3b7574` the built pin | owner decision 2026-11-20: later. Stale from T1's commit on | owner decision; `SERIES.md` row 09 |
| Is `v0.5.0` a signed tag, and does the pin's tree equal GitHub's tree object for it? | plan Open Question 6; same question Plan 08 left for v0.4.1 | the audit plan |
| Build only Flash-Next (`-DBASALT_MODELS=qwen4exp`) | four models are compiled and shipped, one is served; a build-flag change is a plan | owner decision; a new plan if wanted |
| `README.md` (knob table, Prerequisites) and `run`'s header size the PLE table at 28.8 GB / ~27 GiB; the boot shows 50.7 GiB locked in RAM | Notes, 2026-11-20; a host-RAM figure the owner plans around (125 GiB box, `RAM 64.30 GiB free` after load) | T3's harvest, or a doc fix outside a plan |
| `./publish` header still shows `dc047a7` in its example tags | comment only; carried from Plan 08's Follow-ups | a doc fix outside a plan |
