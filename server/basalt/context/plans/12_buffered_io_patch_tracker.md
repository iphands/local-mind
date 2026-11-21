# Plan 12 — Local patch: a switch that stops Basalt's `O_DIRECT` reads — Tracker

## Overview

- Status: 0% complete (0/5 tasks)
- Start date: — (set when T1 starts)
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
| 1 | T1: the patch, the Dockerfile step, `./build` | `patches/`, `Dockerfile`, `build`, `README.md` | pending | |
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

## Notes / Deviations

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| A RAM-locked Engram mode for Hy4-flash (`--ple-io ram`, as Flash-Next has) | the page cache can be evicted; locked memory cannot. Offered to the owner as option B on 2026-11-20; he chose the switch first | a later plan, if T4 or use shows eviction |
| Boot Flash-Next (`./run`) under `BUFFERED_IO=1` | this plan turns the default on for `./run` and boots only Hy4-flash; Plans 08 and 10 still owe `./run` boots | Plan 10 T2 (its boots will run under the new default), or `BUFFERED_IO=0` there |
| Offer the switch upstream | a carried patch must be re-fitted at every repin | owner decision |
| The audit must read `patches/` | the built engine is the pin plus the patch | Plan 09 |
