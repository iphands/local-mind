# Plan 01 — Ignis launchers for Flash-Next and the 27B on :8700 — Tracker

## Overview

- Status: 17% complete (1/6 tasks)
- Start date: 2026-10-27
- Plan: `context/plans/01_flashnext_27b_launchers.md`
- Evidence: `captures/` (gitignored: `bench_<date>_engine.md`, `conc_<date>_engine.jsonl`), the
  launchers' echoed `## docker run …` lines, boot logs and `PRINT_CONFIG=1` output pasted by the
  owner into the Evidence table below; the test environment: image
  `ghcr.io/gpillon/ignis:0.8.0@sha256:83d0ccc3…`, artifact files + HF revisions from
  `versions.env`, profile `rtxpro6000`, launcher env

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), then `01_flashnext_27b_launchers.md`
   (Key Facts first: they carry the v0.8.0 `path:line` cites).
2. Environment: the Ignis source at tag `v0.8.0` (`950ee0d1`) is the reference for every flag —
   clone it into a scratch directory outside this tree (`git clone https://github.com/gpillon/ignis
   && git -C ignis checkout v0.8.0`); read only, never build. T1–T3 need no GPU, no docker daemon
   beyond the stubs, no image pull.
3. Ordering: T1 → T2 → T3 (T2 fills `ignis_launch`, T3 reuses it). T4–T6 are **owner-run**
   (SERIES Standing Constraints): an agent hands the owner the exact commands, then records what
   is pasted; never runs a pull, a download, a boot or a bench itself. T4 before T5/T6 — it settles
   the two unconfirmed Key Facts T6 depends on (`--user 1000:1000`, `include_usage`).
4. Commit per task as `[ignis][P1][T<n>][<topic>] <summary>`, with **this tracker's row in the same commit**.
5. Restore the system state after the session (P2.4): the vLLM daily driver
   (`../vllmcustom/qwen3.8-flash-next/run-fp4local`) back up if the owner wants it, no leftover
   `ignis` container on `:8700`.

## Open Unknowns

- `--user 1000:1000` with `HOME=/cache`: does Ignis 0.8.0 write anywhere else? (T4)
- Does Ignis honour `stream_options.include_usage` (the bench client needs the usage chunk)? (T4)
- Exact per-model flag spellings for draft tokens / lanes (`--qwen38flashnext-spec-draft-tokens`,
  `--qwen38-spec-draft-tokens`, the lanes flag) at v0.8.0. (T2/T3)

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Pins, shared guards, models link, README download how-to | `versions.env`, `lib/common.sh`, `README.md` | done | sudo refusal rc 1 (word and path); pins load; 4 `hf download` commands; `require_files` hint verified. `hf`/`huggingface-cli` not installed here — README says how. |
| 2 | T2: `qwen38fn/run` + launcher dry-run (gate step 4) | `qwen38fn/run`, `scripts/launcher-dryrun` | pending | |
| 3 | T3: `qwen38-27b/run` | `qwen38-27b/run` | pending | |
| 4 | T4: Flash-Next first boot and smoke (owner) | runtime | pending | |
| 5 | T5: 27B first boot and smoke (owner) | runtime | pending | |
| 6 | T6: Same-session bench, Ignis vs vLLM FP4-Local (owner) | runtime, `captures/` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

**Mandatory in this project** (P1): its product is a number. A claim not in this table has no
provenance and cannot be cited by a later plan.

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|

## Notes / Deviations

<!-- Anything the plan asserted that turned out to be wrong, and what was done instead
     (Rule E). Negative and inconclusive results too. Be blunt — a wrong Key Fact
     recorded honestly is worth more than a clean-looking tracker. -->

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| A/B `SPEC_TOKENS=4` on Flash-Next single-stream code | upstream RF:92-101: 4 drafts 267 vs 3 drafts 257 code, but 177 vs 191 prose — a trade, not a win | a later tuning plan |
| A/B the reuse-retained device/host split and the loop guard's cost | upstream measured small second-turn gains (RF:192-197) and benched with the guard `off` | a later tuning plan |
| Artifacts on local NVMe vs NFS (load time) | ~35 GB of each load bypasses the page cache; NFS throughput unmeasured here | a later plan, if T4's load time is bad |
| Bump the Ignis pin past 0.8.0 | `main` is at 0.9.0, 260 commits ahead (2026-10-27) | a later plan (pin change = plan) |
