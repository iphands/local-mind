<!-- hands-plan:v1 2026-10-27 -->
# context/ — ignis's living memory

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
| `distilled.md` | Confirmed, provenance-tagged facts: Ignis flags, profiles and config fields and their real effect, the artifacts and where they live, container requirements, measured limits | Before new work; write on every confirmation |
| `pitfalls.md` | Bugs, gotchas and traps — especially the ones that took more than one attempt | Before new work; after any surprise |
| `high_level.md` | Why Ignis, why this image and these weights, and what was rejected | Before touching a pin or adding a dependency |
| `../../vllmcustom/context/` | The sibling vLLM project: host facts (uid 1000, docker as yourself, NFS models tree), the `:8700` daily driver's served names, and the FP4-Local numbers this project is measured against | Before comparing against vLLM or touching host-level setup |
| `../../ninfer/context/` | The sibling NInfer project — the engine whose kernels Ignis started from, same `.ninfer` artifact family, same card | When an artifact or kernel question may already be answered there |
| `../../../context/` | The monorepo's shared context — **empty today** | When a finding generalizes beyond this sub-project |

## Where Findings Go <!-- hp:context-routing -->

| You discovered… | Write it in | Shape |
|-----------------|-------------|-------|
| A confirmed fact (an Ignis flag's or profile's real effect, an artifact rule, a container requirement, a measured limit) | `distilled.md` | `## <Topic — the rule in one line>` + provenance tag, with `path:line` or the command and its date |
| A bug, a gotcha, anything that took more than one attempt to fix | `pitfalls.md` | Problem / How to avoid / Sources; tag `[OBSERVED YYYY-MM-DD]` when it actually bit |
| Why a dependency, pin or architecture choice is what it is | `high_level.md` | The constraint, the options table, **Revisit if:** |
| A topic worth more than ~15 lines of prose (e.g. the Flash-Next VRAM budget on 96 GB) | a new `<topic>.md` here, added to the Map above | Dated, tagged, dense |
| Progress, a deviation from a plan, a negative result | The active plan's tracker | A row or a Notes/Deviations line |
| Anything that generalizes beyond this project (docker behavior, this host's limits) | also the monorepo's `../../../context/` — **empty today**; create `pitfalls.md` (or a `<lib>.md` / `high_level.md`) there on first use | Same shape; keep project-specific detail here |

## Provenance <!-- hp:context-provenance -->

Tag every new fact with how it is known, and **never upgrade a tag without doing the work**:

- `[SOURCE]` — upstream Ignis code, cite `path:line` at a named tag or commit · `[DOC]` —
  documentation (Ignis's README, `docs/`, findings, a model card on Hugging Face) · `[REPO]` —
  this repo's own code, config or README · `[LIVE YYYY-MM-DD]` — observed on a running system: a
  boot you watched, a bench row, a probe you ran. A number gets `[LIVE]` only with its run count
  and spread (P1); upstream's own numbers stay `[DOC]`.
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
