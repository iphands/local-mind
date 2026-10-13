<!-- hands-plan:v1 2026-10-13 -->
# context/ — vllmcustom's living memory

You are reading inside `context/`, the project's knowledge base. Agents that skip it repeat
solved problems; agents that don't write to it leave traps for the next one. **Read the relevant
files before new work. Write what you learn as you go — in the same commit as the work.**

`plans/RULES.md` is authoritative for how work is planned, verified and committed. Read it in
full before writing or executing a plan.

## Map <!-- hp:context-map -->

| Path | Holds | Read when |
|------|-------|-----------|
| `plans/RULES.md` | Plan/tracker format; the per-task rules (verification gate, commits, append-only git, lifecycle, harvest, evidence); project rules P1–P2 | Before any plan work — in full |
| `plans/SERIES.md` | Dependency chain, status, next free plan number, north star, standing constraints | Before creating or picking a plan |
| `plans/NN_example.md`, `plans/NN_example_tracker.md` | The skeletons to copy | Writing a plan |
| `plans/completed/`, `plans/abandoned/` | Closed plans (newest = current conventions) | Looking for precedent |
| `distilled.md` | Confirmed, provenance-tagged facts about this stack: pin constraints, flag behavior, measured limits | Before new work; write on every confirmation |
| `pitfalls.md` | Bugs, gotchas and traps — especially the ones that took more than one attempt | Before new work; after any surprise |
| `high_level.md` | Why these pins and this shape, and what was rejected | Before touching the version set or adding a dependency |
| `loop.md` | One research write-up: looped / recurrent-depth transformers in vLLM — what runs, what pays off, which model to test on | Before any looped-model or depth-recurrence work |
| `../repin.md` | The runbook for moving `VLLM_MAIN_SHA` | **Before any pin change.** Follow it; do not paraphrase it |
| `../README.md` | The dense source of truth. Read the section you need, not the whole file: § *sm120-only: what the build actually emits*, § *Preflight*, § *Caching*, § *Memory-capped builds*, § *Performance tuning*, § *Notes / caveats* | Before build, tuning, launcher or cache work |
| `../container/patches/*/UPSTREAM`, `../muse/patches/*/UPSTREAM`, `../qwen3.8-flash-next/patches/*/UPSTREAM` | The convention, not a file list: each names the upstream file an overlay or `.diff` was derived from, and the ones that copy a file carry its content hash. They rotate on every repin | Before editing an overlay or a diff — the launcher refuses on a mismatch and says so |

## Where Findings Go <!-- hp:context-routing -->

| You discovered… | Write it in | Shape |
|-----------------|-------------|-------|
| A confirmed fact about the stack (a pin constraint, a flag's real effect, a measured limit, a tool's actual behavior) | `distilled.md` | `## <Topic — the rule in one line>` + provenance tag, with `path:line` or the command and its date |
| A bug, a gotcha, anything that took more than one attempt to fix | `pitfalls.md` | Problem / How to avoid / Sources; tag `[OBSERVED YYYY-MM-DD]` when it actually bit |
| Why a dependency, pin or architecture choice is what it is | `high_level.md` | The constraint, the options table, **Revisit if:** |
| The outcome of a repin | `../repin.md`'s procedure → the commit body; only the durable facts land in `distilled.md` | One repin, one commit |
| A topic worth more than ~15 lines of prose | a new `<topic>.md` here (like `loop.md`), and add it to the Map above | Dated, tagged, dense |
| Progress, a deviation from a plan, a negative or inconclusive result | The active plan's tracker | A row, or a line under Notes / Deviations |
| Anything that generalizes beyond this sub-project (docker/BuildKit behavior, this host's memory and power limits, a vLLM fact every model here inherits) | also `../../../context/` — the monorepo's context, **deliberately empty today: nothing has earned promotion yet.** Create the file on first use | Same shape; keep the vLLM-custom detail here too |

## Provenance <!-- hp:context-provenance -->

Tag every new fact with how it is known, and **never upgrade a tag without doing the work**:

- `[SOURCE]` — upstream code, cite `path:line` in `$BUILD_DIR/src/<tree>` at a named ref
- `[DOC]` — documentation (vLLM/FlashInfer/torch docs, the model card)
- `[REPO]` — this repo's own code, config or README
- `[LIVE YYYY-MM-DD]` — observed on a running system: a boot you watched, a bench row, a
  `cuobjdump` you ran. A number gets this tag only with its run count and spread (Rule P1)
- Pitfalls that actually bit are tagged `[OBSERVED YYYY-MM-DD]`, to tell them apart from hazards
  seeded from reading.

## House Style <!-- hp:context-style -->

- **Dense.** No fluff — but never compress so far that a detail is lost.
- **No full source code** and nothing private or trademarked. Short algorithm explanations,
  formulas, signatures and architecture ideas are fine.
- Dates are ISO, from `date +%F`.
- A file past ~40 KB is split: `<name>/` with an `index.md` and one file per topic.
- A file that turns out wrong or stale is **fixed in place**, with a dated note of what changed.
- `vendor/` holds third-party source clones: **read-only** reference; never write docs into it —
  distill into `context/`.

## Honesty

**Never claim something is recorded unless the bytes are in the file.** Re-read the file after
writing it. "I noted that in pitfalls.md" is a factual claim about the tree.
