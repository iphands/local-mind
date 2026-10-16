<!-- hands-plan:v1 2026-10-16 -->
# context/ — llama-proxy's living memory

You are reading inside `context/`, the project's knowledge base. Agents that skip it repeat
solved problems; agents that don't write to it leave traps for the next one. **Read the relevant
files before new work. Write what you learn as you go — in the same commit as the work.**

`plans/RULES.md` is authoritative for how work is planned, verified and committed. Read it in
full before writing or executing a plan.

## Map <!-- hp:context-map -->

| Path | Holds | Read when |
|------|-------|-----------|
| `plans/RULES.md` | Plan/tracker format; the per-task rules (verification gate, commits, append-only git, lifecycle, harvest, evidence); project rules | Before any plan work — in full |
| `plans/SERIES.md` | Dependency chain, status, next free plan number, north star | Before creating or picking a plan |
| `plans/NN_example.md`, `plans/NN_example_tracker.md` | The skeletons to copy | Writing a plan |
| `plans/completed/`, `plans/abandoned/` | Closed plans (newest = current conventions) | Looking for precedent |
| `vendor/vllm/`, `vendor/llama.cpp/` (project root: `llama-proxy/vendor/`; **outside this KB**) | upstream backend checkouts — **read-only**: never edit, never `git add` | learning exact vLLM/llama.cpp behavior (metrics, `/props`, gauges); read-only rule + search recipe in RULES P3 |

## Where Findings Go <!-- hp:context-routing -->

| You discovered… | Write it in | Shape |
|-----------------|-------------|-------|
| A confirmed fact | `distilled.md` (create on first use) | Tagged [SOURCE]/[OBSERVED] |
| A bug or gotcha that took more than one attempt | `pitfalls.md` (create on first use) | Tagged [OBSERVED YYYY-MM-DD] |
| A dependency choice (pros/cons, revisit-if) | `high_level.md` (create on first use) | Table form |
| Progress, a deviation from a plan, a negative result | The active plan's tracker | A row or a Notes/Deviations line |
| Backend behavior read from `vendor/*` | `distilled.md` (create on first use); after a split, the matching `distilled/<topic>.md` (see House Style) | Tagged [REPO] with `vendor/<tree>/<path>:<line>` |

## Provenance <!-- hp:context-provenance -->

Tag every new fact with how it is known, and **never upgrade a tag without doing the work**:

- [SOURCE] [DOC] [REPO] [LIVE YYYY-MM-DD]
- Pitfalls that actually bit are tagged `[OBSERVED YYYY-MM-DD]`, to tell them apart from hazards
  seeded from reading.

## House Style <!-- hp:context-style -->

- **Dense.** No fluff — but never compress so far that a detail is lost.
- **No full source code** and nothing private or trademarked. Short algorithm explanations,
  formulas, signatures and architecture ideas are fine.
- Dates are ISO, from `date +%F`.
- A file past ~40 KB is split: `<name>/` with an `index.md` and one file per topic.
- A file that turns out wrong or stale is **fixed in place**, with a dated note of what changed.

## Honesty

**Never claim something is recorded unless the bytes are in the file.** Re-read the file after
writing it. "I noted that in pitfalls.md" is a factual claim about the tree.
