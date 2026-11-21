# Plan 14 — Inkling 2 Mini launcher: `./run-ink` — Tracker

## Overview

- Status: 20% complete (1/5 tasks)
- Start date: 2026-11-21
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
| 1 | T1: `./run-ink`, the completeness helper, the gate | `run-ink`, `lib/common.sh`, `RULES.md`, `README.md`, `AGENTS.md` | done | `run-ink` is `run-hy4`'s shape with Inkling 2 Mini's knobs and refusals and no NFS note (the two files are the same from the host checks on, but for two comments and that note). `pack_declared_bytes` in `lib/common.sh` reads the header's `file_bytes`; `run-ink` refuses a pack shorter than that, before it asks for the architecture. `RULES.md`: Expected counts `6 b` → `7 b`, the 1b identity guard and step 2's comment name `run-ink`, step 4 writes two packs and asserts the dry run, six refusals, the masked key and `BUFFERED_IO=0`. `README.md` and `AGENTS.md` name the third launcher. Gate 1a/1b/1c/2/2b/3/4/5/5b/6 pass — Evidence rows 2–4. **Done as a launcher that dry-runs.** Not shown: the model's parser taking these words from a real pack (T3, gate 5b) and a boot (T4). Steps 5 and 5b do not list `./run-ink` yet: its default pack does not exist under its name, so the dry run they read would die; its flag *names* are `./run-hy4`'s, which step 5 does check. |
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
| 2026-11-21 | T1: `shellcheck -x run-ink lib/common.sh` before `git add`; `DRY_RUN=1 ./run-ink <pack>` on three packs written by a copy of the gate's `mkpack` (a whole `inkling2` one, one whose header claims 1 MiB more than the file, a whole `deepseek41` one), on the real Flash-Next and Hy4-flash packs, with no argument, and with `KV=int8`, `MAX_CONTEXT=1048577`, `VISION=cpu`; one run with `API_KEY=k3y KV=fp8 PARALLEL=2 MAX_CONTEXT=1048576 BUFFERED_IO=0 EXTRA_ARGS='--spec 1'` | lint: no output. Whole pack, exit 0: `== mode:  1 slot × 362144, kv=fp16, vision=dynamic, io=buffered, http://0.0.0.0:8700/v1 as 'cosmo-6000'`; `… serve /models/Fake-Inkling.basalt --host 0.0.0.0 --port 8700 --alias cosmo-6000 --allowed-host cosmo.lan --allowed-host cosmo --gpu 0 --max-context 362144 --kv fp16 --parallel 1 --vision dynamic`; `== (DRY_RUN) nothing started`. Cut short, exit 1: `… is incomplete: 241 of 1048817 bytes (0%).` Others, exit 1 each: `is not an Inkling 2 Mini pack (architecture 'deepseek41', want inkling2)`; `is a Flash-Next pack (architecture qwen4exp).` / `serve it with ./run instead.`; `is a Hy4-flash pack (architecture hy4flash).` / `serve it with ./run-hy4 instead.`; `pack not found: /mnt/noir/…/Inkling-2-Mini-NVFP4-q8.basalt` with `has the download finished?`; `KV must be fp16 or fp8 …`; `MAX_CONTEXT must be 1024..1048576 …`; `VISION must be off or dynamic …`. The knob run: `2 slots × 1048576 (slot-full-context), kv=fp8, vision=dynamic, io=direct`, `… --kv fp8 --parallel 2 --slot-full-context --vision dynamic --api-key REDACTED --spec 1` | 1 each | `storage/` gained nothing. Run while the owner's `basalt` container served Flash-Next: each successful dry run also printed the busy report |
| 2026-11-21 | T1, 13:16–13:17 PST: `DRY_RUN=1 ./run-ink` on the owner's download in progress, the temp file `.cache/huggingface/download/….f7b4e5f8….incomplete`; then `stat -c 'apparent=%s blocks=%b'` and `du -B1` on it | **accepted, exit 0**: `== pack:  ….incomplete (19734300551 bytes, 18 GiB)`, architecture read as `inkling2`. The file had its full length, 19,734,300,551 bytes, with 19,520,479,232 allocated: about 214 MB not yet written | 1 | the check compares lengths, and this downloader sets the length first and fills in pieces. Plan Open Question 6. The earlier attempt, a file growing under the pack's own name (plan Key Facts), is the case the check does catch |
| 2026-11-21 | T1: project gate 1–6 with 5b; before it, step 1 with `run-ink` still untracked, and step 4 with `KV=fp8` in the environment | `PRODUCT_B=7 AUDIT_B=5 TOTAL_B=12`, `run-ink` in the printed product set; `env ok`; `shellcheck ok (product)`; `shellcheck ok (audit)`; `preflight ok`; `dry-run ok (image: iphands/basalt-blackwell:cu1343-sm120)`; `flags ok (3 engine, 8 server: 159 engine flags known)`; three `pack flags ok` lines; `imports ok (44 serve modules)`; no `FAIL`. Controls: untracked, `PRODUCT_B=6` and `FAIL(1b) missing product bash: run-ink`; `KV=fp8`, `FAIL(dry:ink) missing: --kv fp16` | 1 | the two controls are the new guard and the new step-4 block shown firing |

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
