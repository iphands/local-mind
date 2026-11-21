# Plan 12 — Local patch: a switch that stops Basalt's `O_DIRECT` reads — Tracker

## Overview

- Status: 20% complete (1/5 tasks)
- Start date: 2026-11-20
- Plan: `context/plans/12_buffered_io_patch.md`
- Evidence: the table below; build logs in `captures/` (gitignored); boot blocks pasted by the owner.
  Test environment: host `cosmo`, RTX PRO 6000 (GPU 0), packs on NFS from `noir`.

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), then `12_buffered_io_patch.md`.
2. Environment: docker as yourself; `shellcheck` 0.11.0; `/mnt/noir` mounted; the gate's scratch
   paths pointed at a directory that exists. `GIT_NO_LAZY_FETCH=1` on every git command against
   `vendor/basalt`. Never edit `vendor/basalt`: build a patch from copies (plan § *Why an environment
   switch, patched at build time*).
3. Ordering: T1 → T2 → T3 → T4 → T5. T3 waits on the owner's `./build`, T4 on the owner's boot; mark
   each `blocked` with the reason while it waits. **T3 commits nothing before the build** (its gate
   step fails on the old image).
4. Commit per task as `[basalt][P12][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**. Never push.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: the patch, the Dockerfile step, `./build` | `patches/`, `Dockerfile`, `build`, `README.md` | done | `patches/0001-buffered-io-switch.patch` (the scratch diff with a prose header and file timestamps dropped; sha256 `a3553ec6bb8c…`). `Dockerfile`: `patch` in the apt list, a `RUN --mount=type=bind,from=patches` that applies `/patches/*.patch` with `patch -p1 --fuzz=0`, `ARG BASALT_PATCHES` → label `ai.basalt.patches`. `build`: `git apply --check` of every patch against the pin (in the preflight too), `--build-context patches=…`, `--build-arg BASALT_PATCHES=0001-buffered-io-switch.patch:a3553ec6bb8c`. The Dockerfile step was rehearsed in the devel image on copies of the three files and compiled with its g++ 13.3.0; a damaged patch stops the preflight by name — Evidence rows 2–3. Gate 1a/1b/1c/2/2b/3/4/5/5b/6 pass (5, 5b, 6 on the existing, unpatched image). **No image has been built from this recipe**: T3. |
| 2 | T2: `BUFFERED_IO` in the launchers and `./preload` | `run`, `run-hy4`, `preload`, `lib/common.sh`, `RULES.md`, `README.md` | pending | |
| 3 | T3: owner rebuild; the gate checks the image carries the patch | `RULES.md`, tracker Evidence | pending | |
| 4 | T4: owner boot — Hy4-flash from NFS, a long prompt | tracker Evidence | pending | |
| 5 | T5: harvest and close | `context/` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-20 | before the plan commit, the patch built from copies of the three files (`diff -u -r a b`): `GIT_NO_LAZY_FETCH=1 git -C vendor/basalt apply --check --verbose <patch>`; `g++ -std=c++20 -fsyntax-only` on both patched `.cpp` (host g++ 16.2.1); a harness linking the patched `file.cpp` and `direct_file.cpp`, run with `BASALT_BUFFERED_IO` unset, empty, `0`, `1`, reading each descriptor's flags from `/proc/self/fdinfo` | 89 lines, 7 hunks, 3 files; `Checking patch …` ×3, no error, and `git status --porcelain` in `vendor/basalt` stayed empty; `syntax ok` ×2. Harness, switch unset / empty / `0`: `buffered_io=0`, `open_unbuffered(path)=fd O_DIRECT`, `DirectFile fd: O_DIRECT`, `bypasses_cache=1`. Switch `1`: `basalt: BASALT_BUFFERED_IO=1: file reads go through the OS cache (no O_DIRECT)`, `buffered_io=1`, `open_unbuffered(path)=-1`, `DirectFile fd: cached`, `bypasses_cache=0` | 1 each | not the image's compiler (g++ 13) and not the engine: T1 rehearses the first, T3 and T4 show the second |
| 2026-11-20 | T1: `shellcheck -x build`; `PREFLIGHT_ONLY=1 ./build`; in `nvidia/cuda:13.4.3-devel-ubuntu24.04@sha256:5626a7d0…` (`--network none`, `vendor/basalt` and `patches/` mounted read-only): copy the three files, the Dockerfile's loop (`patch -p1 --fuzz=0 --no-backup-if-mismatch`), then `g++ -std=c++20 -Wall -Wextra -fsyntax-only` on both `.cpp` | lint: no output. Preflight: `== patch fits: 0001-buffered-io-switch.patch`, `#6 transferring patches: … done`, `Check complete, no warnings found.`, a build command with `--build-context patches=…/patches --build-arg BASALT_PATCHES=0001-buffered-io-switch.patch:a3553ec6bb8c`, `preflight ok`; `vendor/basalt` still clean. In the image: `/usr/bin/patch`, `g++ (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0`; `== applying 0001-buffered-io-switch.patch`, `patching file` ×3 with no fuzz or offset line; `syntax ok (image g++)` ×2, no warning | 1 | `patch` was already in the devel image; it is in the apt list anyway so the recipe does not lean on that. A syntax check is not a link: T3 |
| 2026-11-20 | T1 negative control: one context line of the patch altered, `PREFLIGHT_ONLY=1 ./build`; then the file restored (`cmp` against the kept copy) | `error: include/basalt/platform/file.hpp: patch does not apply`; `## patch does not fit vendor/basalt at ad274aa: patches/0001-buffered-io-switch.patch`; exit 1. Restored: sha256 `a3553ec6bb8c…` again | 1 | the fit check fails closed and names the file |

## Notes / Deviations

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| A RAM-locked Engram mode for Hy4-flash (`--ple-io ram`, as Flash-Next has) | the page cache can be evicted; locked memory cannot. Offered to the owner as option B on 2026-11-20; he chose the switch first | a later plan, if T4 or use shows eviction |
| Boot Flash-Next (`./run`) under `BUFFERED_IO=1` | this plan turns the default on for `./run` and boots only Hy4-flash; Plans 08 and 10 still owe `./run` boots | Plan 10 T2 (its boots will run under the new default), or `BUFFERED_IO=0` there |
| Offer the switch upstream | a carried patch must be re-fitted at every repin | owner decision |
| The audit must read `patches/` | the built engine is the pin plus the patch | Plan 09 |
