# Plan 04 — Decode NVFP4-CSF expert scales at load time — Tracker

## Overview

- Status: 0% complete (0/4 tasks)
- Start date: — (set when T1 starts)
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
| 1 | T1: patch + host test | `container/patches/csf-scale-decode/` | pending | |
| 2 | T2: real shards via real seam | tracker evidence | pending | |
| 3 | T3: launcher, docs, gate counts | `run-fp4local`, README, context | pending | |
| 4 | T4: owner boot | — | pending | owner-run |

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|

## Notes / Deviations

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
