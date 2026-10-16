# Plan 03 — vLLM boot stats JSONL log — Tracker

## Overview

- Status: 33% complete (2/6 tasks)
- Start date: 2026-10-16
- Plan: `context/plans/03_vllm_stats_log.md`
- Evidence: `./storage/vllm_boot_stats.jsonl` (one JSONL line per boot, append-only). Test environment: image tag from `container/.last-build`, `VLLM_REF` from `container/versions.env`, model from `qwen3.8-flash-next/run` `$MODEL`.

## Resume Instructions

1. Read the project's root `AGENTS.md`/`CLAUDE.md`, `context/plans/RULES.md` (in full), then `03_vllm_stats_log.md`.
2. Environment: `VLLM_BOOT_LOG=1` (opt-in), `VLLM_BOOT_LOG_DIR`, `VLLM_MODEL_NAME`, `VLLM_BOOT_ARGS` env vars; `BOOT_TIMING=1` (already default in `qwen3.8-flash-next/run`). Standing Constraints from `SERIES.md`: shellcheck absent (gate step 2 = `skip`), no sudo, pytest/uv/rsync absent, no model load in per-commit gate.
3. Ordering: T1 (grounding) → T2 (schema) → T3 (boot_timing.py writer) → T4 (launcher wiring) → T5 (gate verify) → T6 (commit). Each task commits before the next starts (Rule B).
4. Commit per task as `[P3][T<n>][<topic>] <summary>`, with this tracker's row in the same commit.
5. Restore the system state at the end of the session (Rule P2.4) — power limits, launcher env, debug flags.

## Open Unknowns

- Whether `VLLM_BOOT_ARGS` should be space-joined or JSON-encoded (risk: args with embedded spaces/JSON). Decide in T3/T4. ~~strike~~ RESOLVED (T2): JSON-encoded key→value dict, bare flags map to `true`.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Grounding & key facts | `container/patches/boot-timing/boot_timing.py`, `common/_run-lib.sh`, `qwen3.8-flash-next/run` | done | All Key Facts line numbers verified against source: format_report=585, _record=719, vllm_launch=370, BOOT_TIMING_DIR=610, BOOT_TIMING=254. |
| 2 | T2: Design JSONL schema & storage layout | `context/plans/03_vllm_stats_log.md` | done | Schema defined in plan with validated example JSONL object; vllm_args is a key→value dict (not array); storage at ./storage/vllm_boot_stats.jsonl, mounted to /opt/vllm-storage, gated by VLLM_BOOT_LOG_DIR env. |
| 3 | T3: Add JSONL writer to boot_timing.py | `container/patches/boot-timing/boot_timing.py` | pending | gated by `VLLM_BOOT_LOG_DIR` env |
| 4 | T4: Wire launcher to mount storage + pass model/args | `common/_run-lib.sh`, `qwen3.8-flash-next/run` | pending | mirrors cache-dir ownership pattern |
| 5 | T5: Verify with project gate | all touched files | pending | gate step 2 = skip (shellcheck absent) |
| 6 | T6: Commit plan + tracker + SERIES | `context/plans/03_vllm_stats_log*.md`, `context/plans/SERIES.md` | pending | `[P3][plan] add ...` |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` — the last three always carry a reason in the row's Notes cell.

## Evidence

**Mandatory in this project** (Rule P1): its product is a number, and a number without provenance gets cited. A claim not in this table has no provenance and cannot be cited by a later plan.

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|

Record the build every row was taken on: image tag, `ai.vllmcustom.vllm.ref`, and the launcher env. `bench/bench-results.md` is gitignored — it is not evidence a later plan can cite; the jsonl row or a named file under `bench/` is.

## Notes / Deviations

None yet.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Other launchers (qwen, muse, laguna) opt into jsonl logging | boot-timing patch is only mounted by qwen3.8-flash-next today | later plan |
| JSON-encode `VLLM_BOOT_ARGS` instead of space-join if args contain spaces | `--override-generation-config` has embedded spaces/JSON | T3/T4 decision point |

---
