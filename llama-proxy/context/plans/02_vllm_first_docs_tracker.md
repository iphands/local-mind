# Plan 02 — VLLM-first docs + read-only vendor discovery space — Tracker

## Overview

- Status: 0% complete (0/6 tasks)
- Start date: — (set when T1 starts)
- Plan: `context/plans/02_vllm_first_docs.md`
- Evidence: doc-framing plan — evidence is the grep output each Verify prints (the claim flipping
  vLLM-first) plus the `git check-ignore` / `rg` blindness pair in T3. No live perf numbers, so
  the Evidence table records commands + observed output, not measurements.

## Resume Instructions

1. Read the project's root `AGENTS.md`/`CLAUDE.md`, `context/plans/RULES.md` (in full), then
   `02_vllm_first_docs.md`. Note the **Preservation List** in T4 — it is the contract.
2. Environment: no live backend needed. `AGENTS.md` is a **symlink** to `CLAUDE.md` (root and
   `context/`) — always edit `CLAUDE.md`, never `AGENTS.md`, and **never `sed -i AGENTS.md …`**:
   write-and-rename tools (`sed -i`, most editors' save) replace the symlink with a regular file,
   after which `diff -q AGENTS.md CLAUDE.md` still passes while the two docs have silently forked.
   `vendor/{vllm,llama.cpp}` are on disk, untracked; after T3 they are gitignored, so search them
   with `rg --no-ignore`.
3. Ordering: **T1 → T2 → T3** (write the vendor rule + KB row *before* ignoring the tree, so the
   escape hatch is in place before grep goes blind), then T4, T5, T6 (independent). T4 must not
   touch the `hands-plan:begin` block; T5 must not rename the "Is it really streaming?" heading.
4. These three artifacts (plan + tracker + SERIES row) land first, before any task, as
   `[llama-proxy][P2][plan] add vllm-first docs + read-only vendor discovery` (RULES.md Rule B
   bookkeeping tag — do NOT fold them into T1's commit). Then commit per task as
   `[llama-proxy][P2][T<n>][<topic>] <summary>` (plan number **not** zero-padded), with **this
   tracker's row in the same commit**. When T1 starts, flip plan `Status` → `in-progress`, this row
   → `in-progress` under SERIES *Currently Active*, and set `Start date` — all in T1's commit.

## Open Unknowns

- Does any tracked doc outside CLAUDE.md/README.md/fixrate.md still claim llama.cpp is *the*
  target? → **assigned T6** (T6 greps with `--exclude-dir` and files a disposition row per hit).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: P3 read-only vendor rule + `rg --no-ignore` | `context/plans/RULES.md` | pending | write before T3 |
| 2 | T2: register vendor in context KB map | `context/CLAUDE.md` | pending | symlink to AGENTS.md |
| 3 | T3: gitignore `/vendor/`, observe ignore took | `.gitignore` | pending | record rg blindness pair |
| 4 | T4: root CLAUDE.md vLLM-first | `CLAUDE.md` | pending | keep hands-plan block + Preservation List |
| 5 | T5: README + fixrate vLLM-first | `README.md`, `fixrate.md` | pending | don't rename cited headings |
| 6 | T6: triage docs + stamp PLAN.md obsolete | `PLAN.md` (+grep) | pending | stamp, don't delete |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-16 | `hp-scan --brief .` | `NEXT computed=02` | 1 | plan number source |
| 2026-10-16 | `ls -l AGENTS.md context/AGENTS.md` | both → `CLAUDE.md` symlinks | 1 | edits go to CLAUDE.md |
| 2026-10-16 | `rg --files target` | listed gitignored `target/` files | 1 | env rg shows ignored tree when passed explicitly |

_(T3 adds the `rg --files vendor`=0 vs `rg --no-ignore --files vendor`>0 pair on the day it runs.)_

## Notes / Deviations

**Anything the plan asserted that turned out wrong, and what was done instead (Rule E).**

- (pre-exec) fixrate.md was found to already name the vLLM split path (`:47`, `:387`, `:411`), so
  T5's fixrate change was narrowed from "add the path" to "add a cross-reference + vendor pointer."

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Update CLI `about`/`long_about` strings (`src/main.rs:47`) + `main.rs:1`/`lib.rs:1` doc comments to vLLM-first | Observable `--help` behavior; code change, out of a docs plan's scope | a later code plan |
| Delete `PLAN.md` after human review (currently OBSOLETE-stamped) | Deletion is a human call; append-only until then | next resync / human |
| Consider a "which backend gives me what" table (`/props`, `/slots`, `timings`, `metrics`, `vllm:cache_config_info`, `max_model_len`) in README | Single table answers most vLLM-first positioning questions | optional README follow-up |
