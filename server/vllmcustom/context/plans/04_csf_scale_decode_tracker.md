# Plan 04 — Decode NVFP4-CSF expert scales at load time — Tracker

## Overview

- Status: 25% complete (1/4 tasks)
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
| 2 | T2: real shards via real seam | tracker evidence | pending | |
| 3 | T3: launcher, docs, gate counts | `run-fp4local`, README, context | pending | |
| 4 | T4: owner boot | — | pending | owner-run |

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-15 | `python3 container/patches/csf-scale-decode/test_csf_scale_decode.py` | ALL TESTS PASS (29) | 1 | host fake seam; torch backend NOT exercised here (no torch on host) — T2 |
| 2026-10-15 | gate 1 / 3 | 28 b / 25 p, env ok; 5/5 patch tests ok | 1 | steps 2 (shellcheck absent) SKIP; 4, 5 not re-run: no pin, no build touched |

## Notes / Deviations

- Gate step 1 documented `27 b`; the tree already read 28 before this plan (`nemotron/run` was added without a bump). Corrected in RULES.md with the story.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
