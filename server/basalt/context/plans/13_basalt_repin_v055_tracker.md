# Plan 13 — Repin Basalt to v0.5.5 and rebuild the image — Tracker

## Overview

- Status: 33% complete (1/3 tasks)
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
| 1 | T1: repin to `ff0b978`, preflight, owner build, gate on the new image | `versions.env`, `README.md` | done | `BASALT_SHA` is `ff0b978d2ee9ff195d113b6feb80961b2d2f99ef`; the owner's `./build` made image `a3441f94bdf1` (`cu1343-sm120-ff0b978`, labels `ai.basalt.ref` = the full SHA, `ai.basalt.patches=0001-buffered-io-switch.patch:a3553ec6bb8c`). The recipe needed **no change** and neither did the patch: `Dockerfile`, `Dockerfile.dockerignore`, `patches/0001` and the `RULES.md` gate snippets are untouched. The preflight says the patch fits, the build log shows it applied with no fuzz or offset, and the binary holds the string `BASALT_BUFFERED_IO`. Every `open_unbuffered` and `DirectFile` user is the same line at both pins (71 lines in 13 files; none under `src/models/inkling2`). Gate 1a/1b/1c/2/2b/3/4/5/5b/6 pass on the new image; step 5's negative control fires — Evidence rows 4–10. `README.md` names the new pin, tag and the patched image. Boots are T2. |
| 2 | T2: owner boots, `./run` and `./run-hy4`, a real reply each | tracker Evidence | blocked | **Half recorded; waits on the owner (2026-11-21).** The `./run` boot is in: the owner's own session on the new image, engine 0.5.5, buffered reads on, `3 × 362144` accepted, 14 requests finished with `end=stop` and no error line — Evidence row 11. What that row does not hold: `./run`'s own `==` lines (they went to the owner's terminal) and the text of a reply (the client was the owner's). Still needed: `./run-hy4` on this image (after `./preload` when the pack is on NFS), its `==` lines, start block and one reply. |
| 3 | T3: harvest and close | `context/` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-21 | project gate 1–6 with 5b, old pin `ad274aa`, image `iphands/basalt-blackwell:cu1343-sm120` (id `4de9f15bb9d2`, labels `ai.basalt.ref=ad274aa8…`, `ai.basalt.patches=0001-buffered-io-switch.patch:a3553ec6bb8c`), before the plan commit | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=6 AUDIT_B=5 TOTAL_B=11`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 157 engine flags known)`; `pack flags ok` ×3 (`./run`: `qwen4exp takes --max-context 362144 --kv int8 --ple-io ram --batch 3`; `./run --single`: the same without `--batch 3`; `./run-hy4`: `hy4flash takes --max-context 362144 --kv fp16`); `imports ok (42 serve modules)` | 1 | the baseline T1 is compared against; `shellcheck` 0.11.0 at `/usr/local/bin` in this shell; gate scratch paths pointed at the session's scratch directory. Run while the owner's `basalt` container was up (14 hours, the same image id); the dry runs report a busy port and do not refuse |
| 2026-11-21 | `git ls-remote --tags --heads https://github.com/jesdga95/basalt.git`, filtered to `v0.5` and `main` | `refs/tags/v0.5.5` = `3cfb5ef4…`, `refs/tags/v0.5.5^{}` = `ff0b978d2ee9ff195d113b6feb80961b2d2f99ef`; `main` = `97e2e711…` | 1 | verbatim in the plan § *Pre-Identified Bug/Issue* |
| 2026-11-21 | agent: `git -C vendor/basalt fetch --quiet origin`, then reads of the fetched objects with the checkout left at `ad274aa` (`OLD`; `NEW` = `ff0b978`): `rev-list --count` both ways, `merge-base`, `log -1`, `tag -l 'v0.5*'`, `cat-file -p v0.5.5`; `rev-parse OLD:<f>` against `NEW:<f>` for nine files; `diff --no-renames --name-only OLD NEW` counted by directory; `diff OLD NEW -- CMakeLists.txt cmake/models.cmake docs/building.md serve/config.py src/models/qwen4exp/flags.cpp`; `show NEW:docs/changelog/0.5.5.md`, `NEW:tests/models/qwen4exp/cli/kv_batch_test.cpp`; `grep -n` in `NEW:serve/config.py` and `NEW:serve/engine/vision.py` | 177 ahead, 0 behind, merge base `ad274aa`; `ff0b978… 2026-11-21 Merge Inkling 2 Mini support (Basalt 0.5.5)`; tags `v0.5.0`, `v0.5.5` in the clone, the `v0.5.5` object unsigned. Same blob at both pins: `Dockerfile`, `.dockerignore`, `requirements.txt`, `tools/model_file.py`, `LICENSE`, `THIRD_PARTY_LICENSES`, and the patch's three files (`include/basalt/platform/file.hpp`, `src/platform/direct_file.cpp`, `src/platform/file.cpp`). 277 files changed. The rest is the plan's Key Facts table, row by row | 1 | after the fetch `git -C vendor/basalt status --porcelain --untracked-files=all` is empty and `HEAD` is still `ad274aa`. Reads of named files fetched those blobs on demand; every other probe ran under `GIT_NO_LAZY_FETCH=1` (a rename-detecting `diff --name-status` refused under it, hence `--no-renames`) |
| 2026-11-21 | T1 step 2, agent: `PREFLIGHT_ONLY=1 ./build` with `versions.env` at `ff0b978` | `== vendor/basalt at ff0b978d2ee9ff195d113b6feb80961b2d2f99ef, clean`; `== patch fits: 0001-buffered-io-switch.patch`; both CUDA digests resolve; `Check complete, no warnings found.`; `preflight ok` | 1 | the build command it printed carries `--build-arg BASALT_PATCHES=0001-buffered-io-switch.patch:a3553ec6bb8c` |
| 2026-11-21 | T1 step 2, agent: `grep -rn 'open_unbuffered\|DirectFile' src include` in `vendor/basalt` at `ad274aa` (before the checkout moved) and at `ff0b978`, line numbers stripped, sorted, `diff`; `grep -rn 'open_unbuffered\|DirectFile\|O_DIRECT\|bypasses_cache' src/models/inkling2`; `grep -rn O_DIRECT src --include=*.cpp \| grep '::open'`; the three files copied out, `patch -p1 --fuzz=0 --no-backup-if-mismatch`, `g++ -std=c++20 -Wall -Wextra -fsyntax-only` on both `.cpp` against the new tree's headers | 71 lines in 13 files at each pin, `diff` empty for both the lines and the file list; nothing under `src/models/inkling2`; the same four `::open` lines as Plan 12 § *Pre-Identified Bug/Issue*, at the same line numbers; `patching file` ×3, `syntax ok` ×2, no warning | 1 | host g++ 16.2.1, not the image's 13.3; the image's compile is row 7. `vendor/basalt` clean afterwards |
| 2026-11-21 | T1 step 2, agent: project gate on the dirty tree, new pin, before any image for it | steps 1, 2, 2b, 3 as in row 1; `dry-run ok (image: stub:none)`; `FAIL(flags) no image for BASALT_SHA — run ./build`; `FAIL(pack-flags) no image for BASALT_SHA — run ./build`; `FAIL(imports) no image for BASALT_SHA — run ./build` | 1 | the three FAILs are why nothing was committed here. `DRY_RUN=1 ./run` ends `## no iphands/basalt-blackwell image is labelled ai.basalt.ref=ff0b978…; run ./build first.` |
| 2026-11-21 | owner: `./build`; log `captures/build_2026-11-21_ff0b978.log` (610 lines) | `#14 … == applying 0001-buffered-io-switch.patch`, `patching file` ×3, no `fuzz`, `offset` or `Hunk` line; the cmake configure + build step is `#15 DONE 327.9s`, 449 ninja targets; image created 12:37:54 PST | 1 | v0.5.0: 307.9 s, 427 targets (Plan 10 tracker, Evidence row 4). Five models built (`BASALT_MODELS` left at upstream's default, now the word `all`); `docker images` gives 3.5GB, v0.5.0's 3.49GB |
| 2026-11-21 | `docker image inspect` on `iphands/basalt-blackwell:cu1343-sm120-ff0b978`; in it (`--network none`): `ls /opt/basalt/bin /opt/basalt/data/profiles`, `grep -c BASALT_BUFFERED_IO /opt/basalt/bin/basalt` | id `sha256:a3441f94bdf1eba9345f683f8015326fad9a65b42120a743cbe39ec98d85754f`; labels `ai.basalt.ref=ff0b978d2ee9ff195d113b6feb80961b2d2f99ef`, `ai.basalt.patches=0001-buffered-io-switch.patch:a3553ec6bb8c`, `ai.basalt.cuda=13.4.3`, `ai.basalt.arch=sm_120`; `bin` holds `basalt`, `basalt-bench`, `basalt-probe`, `basalt-vision`; `data/profiles` holds `qwen4exp-48x512.prof`, `hy4flash-42x512.prof`, `inkling2-24x128.prof` and two `deepseek41` profiles; the grep counts 2 | 1 | `cu1343-sm120` and `latest` list the same id `a3441f94bdf1`. The string in the binary is what Plan 12 T3's step 6b was to check; that step is still not written |
| 2026-11-21 | project gate 1–6 with 5b, new pin, image `a3441f94bdf1` | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=6 AUDIT_B=5 TOTAL_B=11`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 159 engine flags known)`; `pack flags ok` ×3, the same three lines as row 1; `imports ok (44 serve modules)` | 1 | v0.5.0 image: 157 engine flags known, 42 serve modules. `image_for_pin` prints the moving tag, which lists the new id. Run while the owner's `basalt` container was up on this image |
| 2026-11-21 | gate step 5 with `EXTRA_ARGS=--no-such-flag` | `FAIL(flags) --no-such-flag` | 1 | the negative control fires on the new image |
| 2026-11-21 | owner: `./run`, container `basalt` started 12:50:11 PST; agent, about four minutes later: `docker inspect -f '{{.Image}} {{json .Args}} {{json .Config.Env}}' basalt` and its mounts, `docker logs basalt` (52 lines, later 14 `done` lines), the session's lines in `storage/cache/basalt/aefd5d98dc650ed4/engine.log` (from line 94130) | Image `sha256:a3441f94bdf1…`, the id labelled `ff0b978…` (row 8). Args: `serve /models/Qwen3.8-FN-NVFP4-DQ6_K.basalt --host 0.0.0.0 --port 8700 --alias cosmo-6000 --allowed-host cosmo.lan --allowed-host cosmo --gpu 0 --max-context 362144 --kv int8 --ple-io ram --parallel 3 --slot-full-context --vision dynamic`; env `BASALT_BUFFERED_IO=1`; `/mnt/noir/scratch/ai/llm/models/vllm/jesdga` → `/models` (ro). Server: `cosmo-6000  ctx=362144 kv=int8 ple-io=ram batch=3 spec=8 temp=1.0 top_p=0.95 top_k=20 vision=gpu0`; `placed   n-gram table 50.7GiB 0.4s`; `placed   cuda0 67.8GiB 148.8s`; `loaded   68.4GiB 149.7s  cuda0=67.8GiB ram=0.6GiB`; `kv       1.3/15.8GiB cuda0 elastic slots=3`; `slots    slots 3 × 362144 ctx, others queue`; `weights  loaded, ctx=362144 images=on`. First request: `s1 done  pp=108314/0c 9.4s 11574t/s  tg=1406 380.7t/s acc=66% dl=3.0  ttft=9.4s  end=stop`; then follow-ups on the cached prefix, e.g. `s1 done  pp=110335/109880c 0.3s  tg=1008 562.2t/s acc=88% dl=6.1  ttft=0.3s  end=stop`. Engine log: `basalt: BASALT_BUFFERED_IO=1: file reads go through the OS cache (no O_DIRECT)`; `PLE table locked in RAM (--ple-io ram) in 0.4 s`; `expert cache auto: 79.54 GiB free, 700 MiB reserved … -> 24576 slots`; `expert cache 24576 slots, 63.29 GiB of VRAM`; `pre-filled 24576 of 24576 slots from the profile (67948 MB, read in the start's pass; slot 0 verified after it)`; `session is up (engine 0.5.5)`; `FileExpertSource: allocating 23.53 GiB page-locked cache complement` | 1 | **The build that loaded:** the container's image id is row 8's and the engine names itself 0.5.5; the patch's own line is in the engine log. No flag refused, no context cap, no `error`, `stall` or `refus` in `docker logs` (`grep -ci` → 0). One boot, one client: the rates are single readings, and the follow-up requests' prompt rates are over a few hundred new tokens. Not compared with v0.5.0: Plan 10's boot (196.4 s load, the table locked in 52.0 s) read direct, this one through a page cache that already held the table, so the two differ in more than the engine. Two warnings, both also in the v0.5.0 session: no AVX-512 on this CPU; `the device-planned MoE tail is off: every expert is resident`. `video    off, no FFmpeg libraries found`, as before |

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
- **2026-11-21 — the owner reports the image published and in use**: "built, published and Im using it now",
  after `./build`. The agent did not run `./publish` and did not check the registry. Locally the moving tags
  `cu1343-sm120` and `latest` list the new image id.
- **2026-11-21 — T1's order differed from Plans 08 and 10 in one step**: the agent ran the preflight (step 2)
  itself, so the owner's only command was `./build`.
- **2026-11-21 — T2's `./run` half is the owner's working session, not a boot run for the record.** It gives more than the plan
  asked in one way (a 108,314-token prompt and 14 finished requests rather than one) and less in another: no `==` lines and no
  quoted reply text. The row says which. `./run --single` was not in this plan's T2 and has not been booted on any image since Plan 01.
- **2026-11-21 — this is the first recorded Flash-Next boot under `BUFFERED_IO=1`**, which Plan 12's Follow-ups asked for
  ("Boot Flash-Next (`./run`) under `BUFFERED_IO=1`"): the load finished and requests are served. Plan 12's tracker is not edited here.

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
