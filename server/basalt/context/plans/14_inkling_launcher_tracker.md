# Plan 14 — Inkling 2 Mini launcher: `./run-ink` — Tracker

## Overview

- Status: 0% complete (0/5 tasks)
- Start date: — (set when T1 starts)
- Plan: `context/plans/14_inkling_launcher.md`
- Evidence: the table below; boot blocks pasted by the owner. Test environment: host `cosmo`,
  RTX PRO 6000 (GPU 0), the pack on NFS (`/mnt/noir/scratch/ai/llm/models/vllm/jesdga/`).

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), then `14_inkling_launcher.md`.
2. Environment: docker as yourself; `shellcheck` 0.11.0; `/mnt/noir` mounted; the gate's scratch
   paths pointed at a directory that exists. Lint a new script **before** `git add` (an untracked
   file is invisible to every gate step).
3. Ordering: T1 → T2 → T3 → T4 → T5. T1 and T2 need no Inkling pack. **T3 waits on the owner's
   download** (the pack under its own name, its size equal to the header's `file_bytes`), T4 on the
   owner's boot; mark each `blocked` with the reason while it waits.
4. Commit per task as `[basalt][P14][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**. Never push.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `./run-ink`, the completeness helper, the gate | `run-ink`, `lib/common.sh`, `RULES.md`, `README.md`, `AGENTS.md` | pending | |
| 2 | T2: `./run` and `./run-hy4` name `./run-ink` | `run`, `run-hy4`, `RULES.md`, `README.md` | pending | |
| 3 | T3: the real pack — hash, and the gate on it | `RULES.md`, tracker Evidence | pending | |
| 4 | T4: owner boot — a real reply | tracker Evidence | pending | |
| 5 | T5: harvest and close | `context/` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-21 | project gate 1–6 with 5b before the plan commit, pin `ff0b978`, image `iphands/basalt-blackwell:cu1343-sm120` (id `a3441f94bdf1`) | `INTERPRETER bash=5.3.20(1)-release python=3.14.10 at=/usr/bin/python3`; `PRODUCT_B=6 AUDIT_B=5 TOTAL_B=11`; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 159 engine flags known)`; three `pack flags ok` lines (`./run`, `./run --single`, `./run-hy4`); `imports ok (44 serve modules)` | 1 | the baseline T1 moves: `PRODUCT_B` goes to 7 there. Run while the owner's `basalt` container served Flash-Next; the dry runs report a busy port and do not refuse |

## Notes / Deviations

- **2026-11-21 — the model card was read through a fetch tool** that returns quotes and paraphrase,
  not the page's bytes; the plan's Key Facts mark those rows `[CARD]`. Upstream's guide
  (`docs/models/inkling2.md`) and the model's sources were read from `vendor/basalt` at the pin.
- **2026-11-21 — the download changed shape while the plan was being written.** At 13:11 PST the
  pack's final name held a growing file (1.72 GB of 19.7); by 13:13 it was gone and an `hf download`
  temp file had taken its place under `.cache/huggingface/download/`. The first is what the
  completeness check is for; under the second the launcher simply finds no pack until the rename.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| The completeness check in `./run` and `./run-hy4` | they send a cut-short pack on with "architecture unreadable" (`./run-hy4`) or to the engine (`./run`); the helper exists after T1 | a doc-sized fix outside a plan, or the next launcher plan |
| `PARALLEL` above 1 for Inkling 2 Mini | upstream: "Not measured" | Plan 05 (tuning), if wanted |
| The context default (362144 here, 8,192 upstream, 1,048,576 the maximum) | a starting value, not a measured or chosen one | owner decision after T4 |
