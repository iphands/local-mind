# Plan 04 — Decode NVFP4-CSF expert scales at load time — Tracker

## Overview

- Status: 75% complete (3/4 tasks)
- Start date: 2026-10-15
- Plan: `context/plans/04_csf_scale_decode.md`
- Evidence: image `iphands/vllm-blackwell:cu1321-sm120-main-vllm0.33.0.dev20261015-g4d9a8d6`, model `nvidia/Qwen3.8-Flash-Next-FP4-Local`; launcher echo + boot log pasted by the owner.

## Resume Instructions

1. Read `CLAUDE.md`, `context/plans/RULES.md` (in full), then `04_csf_scale_decode.md`.
2. Environment: `CSF_SCALE_DECODE` (default 1). Standing constraints in `SERIES.md`: no model load in the per-commit gate; never push.
3. Order: T1 → T2 → T3 → T4 (owner).
4. Commit per task as `[P4][T<n>][csf-decode] <summary>`, tracker row in the same commit. `run-fp4local` is untracked and owner-owned: edit it, do not commit it.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: patch + host test | `container/patches/csf-scale-decode/` | done | 29 checks incl. coexistence with draft-load-filter in both install orders. Gate counts bumped in RULES.md in this commit. |
| 2 | T2: real shards via real seam | tracker evidence | done | patch mounted as the launcher will (`.pth` in site-packages); hooked via the real `DefaultModelLoader`; shard 1 only. |
| 3 | T3: launcher, docs | `run-fp4local`, README, context | done | `run-fp4local` edited but NOT in this commit: it is untracked (owner's file). Verified with a stub docker: `CSF_SCALE_DECODE=1` emits both mounts + `csf-decode=on`, `=0` emits neither. Gate counts were bumped in T1. |
| 4 | T4: owner boot | — | pending | owner-run |

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-15 | `python3 container/patches/csf-scale-decode/test_csf_scale_decode.py` | ALL TESTS PASS (29) | 1 | host fake seam; torch backend NOT exercised here (no torch on host) — T2 |
| 2026-10-15 | T2, image `…g4d9a8d6`, uid 1000, CPU only, shard 1 of 21 through real `DefaultModelLoader.get_all_weights` + real `safetensors_weights_iterator`, patch mounted via `zz_csf_scale_decode.pth` | `hooked: True`; 913 CSF pairs → 913 planes, 0 csf names leaked, every `…weight_scale` float8_e4m3fn with shape (640,160) gate/up or (2560,40) down; torch backend == pure-Python reference on 6 of 6 real planes; decoded scales finite, 14–448 (layer 0 expert 0 down_proj) | 1 | 0.3–0.4 s per shard decode (2 runs) → ~8 s for 21 shards, negligible next to the 95 GiB NFS read. Ground truth for the decode is still only the b12x fixture: coherence at T4 is the real test |
| 2026-10-15 | T3 gate step 2, `shellcheck 0.11.0` (now installed — SERIES/RULES still say absent) over tracked bash | **FAIL, pre-existing**: 19 findings in 12 files (`bench/bench-context`, `bench/bench-wrapper`, `container/archs`, `muse/run`, `scripts/muse-*`), none touched by Plan 04; `run-fp4local` (the file this plan edits) is clean | 1 | Not fixed here: out of scope. Follow-up below |
| 2026-10-15 | T3 gate steps 4, 5 | preflight OK for release and snapshot sets; `last-build == 4d9a8d6…`; exactly 1 image id. `container/archs` not re-run: no build | 1 | |
| 2026-10-15 | gate 1 / 3 | 28 b / 25 p, env ok; 5/5 patch tests ok | 1 | steps 2 (shellcheck absent) SKIP; 4, 5 not re-run: no pin, no build touched |

## Notes / Deviations

- Gate step 1 documented `27 b`; the tree already read 28 before this plan (`nemotron/run` was added without a bump). Corrected in RULES.md with the story.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| `shellcheck` is installed (0.11.0) and the tracked tree has 19 findings in 12 files | RULES.md gate step 2 / SERIES say it is absent, so step 2 has been `skip`; it is now a real FAIL baseline | a new plan |
| Commit `qwen3.8-flash-next/run-fp4local` | untracked, owner's file; carries the mount | owner |
