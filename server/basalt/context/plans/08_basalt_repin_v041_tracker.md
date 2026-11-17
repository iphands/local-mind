# Plan 08 — Repin Basalt to v0.4.1 and rebuild the image — Tracker

## Overview

- Status: 40% complete (2/5 tasks)
- Start date: 2026-11-16
- Plan: `context/plans/08_basalt_repin_v041.md`
- Evidence: the table below; build logs in `captures/build_<date>_<sha7>.log` (gitignored); boot
  start blocks pasted by the owner. Test environment: host `cosmo`, RTX PRO 6000 (GPU 0), docker
  daemon 29.8.1, the pack on NFS (`/mnt/noir/scratch/ai/llm/models/vllm/jesdga/`).

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), then `08_basalt_repin_v041.md`.
2. Environment: docker as yourself (no `sudo`); `shellcheck` 0.11.0 on `PATH`. The gate's scratch
   files default to `/tmp/opencode/`, which does not exist in every shell — create it or point the
   two paths at a directory that does. Check `id -nG` lists `docker` before claiming steps 3–6.
3. Ordering: T1 → T2 → T3 → T4 → T5. **T2 commits nothing until the owner's `./build` has produced
   the image** — gate steps 5–6 fail with no image for `BASALT_SHA`. T2 steps 2 and 4 and all of T3
   are the owner's to run; mark the task `blocked` (with the reason) while it waits.
4. Commit per task as `[basalt][P8][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**. Never push.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: let a pack argument override the NFS default again | `run`, `README.md` | done | `./run`'s unconditional `PACK=` line folded into the default (NFS path kept); header, missing-pack hint and README now state the defaults the code has (`362144`, the NFS pack). Gate 1a/1b/1c/2/2b/3/4/5/6 pass on the old pin — Evidence rows 1–3. |
| 2 | T2: repin to `d3b7574`, adapt the recipe, owner build, gate on the new image | `versions.env`, `Dockerfile`, `Dockerfile.dockerignore`, `README.md` | done | `BASALT_SHA` is `d3b75742967c7f9fd892c6894a2b9ac6e36e0cf3`; the owner's `./build` made image `41619e9056f8` (`cu1343-sm120-d3b7574`, label `ai.basalt.ref` = the full SHA). The recipe needed **no functional change**: upstream's own `Dockerfile` at the pin ships the same tree (`bin/ serve/ tools/ data/ .venv`), and `serve.config` still has `build_parser`, `server_options`, `engine_flags_of`, so gate steps 5–6 ran unchanged. Comment-only edits to `Dockerfile` and `Dockerfile.dockerignore`. Gate 1a/1b/1c/2/2b/3/4/5/6 pass on the new image; step 5's negative control fires — Evidence rows 4–7. Not booted: that is T3. |
| 3 | T3: owner boots, default and `--single`, a real reply each | tracker Evidence | blocked | **Waits on the owner (2026-11-16):** stop whatever holds `:8700`, `./run` + one `/v1/chat/completions` reply as `cosmo-6000`, then `./run --single` + one reply; paste `./run`'s `==` lines, the engine's start block and the reply. |
| 4 | T4: retire the `dc047a7` audit plans, register the re-audit | `SERIES.md`, `abandoned/`, `audit/index.md` | pending | |
| 5 | T5: harvest and close | `context/` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-16 | `DRY_RUN=1 ./run /tmp/x.ninfer >/dev/null 2>&1; echo "ninfer rc=$?"` at `6275af2`, before T1 | `ninfer rc=0` — a `.ninfer` path was accepted; gate step 4 red | 1 | the argument was overwritten by `run` line 54 |
| 2026-11-16 | the same command after T1; `PACK=README.md DRY_RUN=1 ./run 2>&1 \| tail -1` | `ninfer rc=1`; `## README.md is not a .basalt pack (first bytes '# basalt', want 'BASALTMF').` | 1 | argument and `PACK=` both honoured |
| 2026-11-16 | project gate 1–6, old pin `dc047a7`, image `iphands/basalt-blackwell:cu1343-sm120` (id `63d25a81f328`, the `dc047a7` build) | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=4 AUDIT_B=5`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok`; `flags ok (3 engine, 8 server: 142 engine flags known)`; `imports ok (32 serve modules)` | 2 | the baseline T2 is compared against; `shellcheck` 0.11.0 at `/usr/local/bin` in this shell |
| 2026-11-16 | owner: `PREFLIGHT_ONLY=1 ./build`, then `./build` (JOBS = 16, half of 32 cores); log `captures/build_2026-11-16_d3b7574.log` (531 lines) | owner reported `preflight ok`; log ends `naming to docker.io/iphands/basalt-blackwell:cu1343-sm120-d3b7574 done`; the cmake configure + build step is `#12 DONE 319.6s`, 407 ninja targets; `pypdf==7.0.1` collected in the venv step | 1 | every model built (upstream's default `BASALT_MODELS`); old pin's image was 3.42 GB, this one 3.49 GB |
| 2026-11-16 | `docker image inspect` on `iphands/basalt-blackwell:cu1343-sm120-d3b7574` | id `sha256:41619e9056f8928f2808c689c94a5f362477f4f68ede1bd2b24a3bfd853e6f38`; labels `ai.basalt.ref=d3b75742967c7f9fd892c6894a2b9ac6e36e0cf3`, `ai.basalt.cuda=13.4.3`, `ai.basalt.arch=sm_120`, `ai.basalt.repo=https://github.com/jesdga95/basalt.git`; `/opt/basalt/bin` holds `basalt`, `basalt-bench`, `basalt-probe`, `basalt-vision`; `data/profiles` holds `qwen4exp-48x512.prof` and two `deepseek41` profiles | 1 | `git -C vendor/basalt rev-parse HEAD` = the pin, tree clean |
| 2026-11-16 | project gate 1–6, new pin, image `41619e9056f8` | `PRODUCT_B=4 AUDIT_B=5`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 153 engine flags known)`; `imports ok (40 serve modules)` | 1 | old image: 142 engine flags known, 32 serve modules. The dry run names the moving tag; `cu1343-sm120`, `cu1343-sm120-d3b7574` and `latest` all inspect to `41619e9056f8` |
| 2026-11-16 | gate step 5 with `EXTRA_ARGS=--no-such-flag` | `FAIL(flags) --no-such-flag` | 1 | the negative control fires on the new image |

## Notes / Deviations

- **2026-11-16 — the plan commit (`[basalt][P8][plan]`) was made on a gate that is red at its own
  tree.** Gate step 4 has been red since `6275af2`; the gate was run in the working tree, where T1's
  fix already sat unstaged, and passed there. The plan commit's message says so. T1's commit is the
  first tree where step 4 is green.
- **2026-11-16 — gate scratch paths.** `/tmp/opencode/` does not exist in this shell; the step 1b
  block was run with its two scratch paths pointed at another existing directory, otherwise as written.
- **2026-11-16 — `image_for_pin` names the moving tag.** It printed `…:cu1343-sm120`, not
  `…:cu1343-sm120-dc047a7`: three tags share image `63d25a81f328` and `docker images` lists that one
  first. Same image, so steps 4–6 ran against the pinned build; after T2 the new image will carry
  only its `-d3b7574` tag until `./publish` moves the others.
- **2026-11-16 — no pre-read of the v0.4.1 tree.** The agent tried to read upstream's `Dockerfile`,
  build docs and `serve/config.py` at `d3b7574` from GitHub before the owner's fetch, to adapt our
  `Dockerfile` ahead of one combined preflight + build; the session's permission layer refused the
  download. T2 step 3 therefore happens as planned, from the tree the owner's preflight checks out,
  and the owner runs two commands with a pause between them rather than one.
- **2026-11-16 — the agent fetched into `vendor/basalt` by accident.** Checking whether the new SHA
  was local with `git -C vendor/basalt cat-file -e <sha>^{commit}` triggered a lazy fetch: the clone
  is `blob:none`, so the probe downloaded a 2.8 MB pack (`pack-a45d1218…`, 16:58:19) holding the 698
  commits `dc047a7..d3b7574` and their trees, no blobs. The checkout was untouched. `build:54` makes
  the same call, so the owner's preflight then found the commit local and skipped `git fetch origin`
  — which is why **the `v0.4.1` tag is not in the clone** (`git tag -l v0.4.1` prints nothing). The
  fetch was the owner's step in this plan. Guard: `GIT_NO_LAZY_FETCH=1` on every probe of that clone.
- **2026-11-16 — the owner started `./build` right after the preflight, before the agent's step 3
  read.** Harmless as it turned out: the read found nothing to change in what the recipe does. The
  committed `Dockerfile` and `Dockerfile.dockerignore` differ from the files that built
  `41619e9056f8` in comment lines only.
- **2026-11-16 — the moving tags already point at the new image.** `cu1343-sm120` and `latest` inspect
  to `41619e9056f8` seconds after the build, though `./build` mints only the `-d3b7574` tag; the agent
  did not move them and does not know what did or whether anything was pushed. This supersedes the
  expectation in the `image_for_pin` note above. `cu1343-sm120-dc047a7` still names `63d25a81f328`,
  the rollback image.
- **2026-11-16 — plan Open Question 7 is not settled.** With no tag object in the clone, `git tag -v
  v0.4.1` cannot run without a fetch; `git ls-remote` shows the tag is annotated (`33bd410c` →
  `d3b75742`), which says nothing about a signature. Deferred to the re-audit plan (Follow-ups).

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Is `v0.4.1` a signed tag, and does the pin's tree equal GitHub's tree object for it? | `high_level.md` § *Attesting the Basalt pin `dc047a77`* → Revisit if (a)–(c); needs a tag fetch | the re-audit plan registered at T4 |
| Serve Qwen3.8-27B (QUASAR NVFP4 + DFlash 2) from this image | Basalt 0.4.0 loads it as a `.basalt` pack, which fires `high_level.md`'s "Revisit if"; the build includes `qwen35` | owner decision; a new plan if wanted |
| Video input (FFmpeg 6–9 libraries in the runtime stage) | optional upstream; the image ships none | owner decision; a Dockerfile plan if wanted |
| `./publish` header still shows `dc047a7` in its example tags | comment only; out of this plan's scope | a doc fix outside a plan |
