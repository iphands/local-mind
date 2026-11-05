<!-- hands-plan:v1 2026-11-04 -->
# context/ — basalt's living memory

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
| `distilled.md` | Confirmed, provenance-tagged facts: what Basalt loads, its flags and config keys and their real effect, container requirements, measured limits | Before new work; write on every confirmation |
| `pitfalls.md` | Bugs, gotchas and traps — especially the ones that took more than one attempt | Before new work; after any surprise |
| `high_level.md` | Why this engine, these pins and this container shape, and what was rejected | Before touching a pin or adding a dependency |
| `audit/` (`index.md`, `manifest/`, `census.md`, `argv-table.md`, `findings.md`, `coverage_ledger.md`, `verdict.md`, `wave<N>/`) | Plan 02's output: the per-file manifest of `vendor/basalt` at the audited pin, the primitive census, the cross-language argv table, capability-ranked findings, derived coverage, the channel matrix and verdict. **`wave<N>/` holds raw command output — paste it there, never into a tracker.** **Before any command relating `vendor/basalt` to the project it forked from, read `audit/index.md` § *If you are about to run something against upstream*: the two repos share 0 SHAs and have no merge-base, so `log A..B`, `cherry` and cross-repo `bisect` run and lie** | Before trusting *or* extending any "Basalt is safe" claim; before re-auditing or bumping the audited pin; before any upstream comparison |
| `../../vllmcustom/context/` | The sibling vLLM project: host facts (uid 1000, docker as yourself, NFS model cache), container conventions, and the FP4-Local daily-driver numbers | Before touching host-level setup or comparing against vLLM |
| `../../ninfer/context/` | The sibling NInfer project — where the `.ninfer` 27B QUASAR artifact belongs | Before anything involving `.ninfer` files |
| `../../../context/` | The monorepo's shared context — **started by Plan 02 T2**: `pitfalls.md` § *Cross-repo git forensics*, the reuse-shaped version of this project's cross-repo traps | When a finding generalizes beyond this sub-project |

## Where Findings Go <!-- hp:context-routing -->

| You discovered… | Write it in | Shape |
|-----------------|-------------|-------|
| A confirmed fact (a Basalt flag's or config key's real effect, a `.basalt` format rule, a container requirement, a measured limit) | `distilled.md` | `## <Topic — the rule in one line>` + provenance tag, with `path:line` at a named commit, or the command and its date |
| A bug, a gotcha, anything that took more than one attempt to fix | `pitfalls.md` | Problem / How to avoid / Sources; tag `[OBSERVED YYYY-MM-DD]` when it actually bit |
| Why a dependency, pin or architecture choice is what it is | `high_level.md` | The constraint, the options table, **Revisit if:** |
| A topic worth more than ~15 lines of prose (e.g. the VRAM/RAM expert budget) | a new `<topic>.md` here, added to the Map above | Dated, tagged, dense |
| Progress, a deviation from a plan, a negative result | The active plan's tracker | A row or a Notes/Deviations line |
| Whether a specific boundary in `vendor/basalt` is open, and where its call sites are | `audit/census.md` (add the site, with `path:line` and its disposition) | One row per call site under a named primitive; audited code goes `benign` / `needs-T<n>` / `suspect` — never blank |
| A security finding, ranked by what it *could* do rather than by intent | `audit/findings.md` (Plan 02) + a `pitfalls.md` entry if it is a trap for us | 🔴 can leave the machine / 🟠 needs a second mistake / 🟡 local-only / ⚪ hygiene, each with what-it-does, why-it-isn't-scary, what-would-make-it-scary, the one action |
| Anything that generalizes beyond this project (docker behavior, this host's limits) | also the monorepo's `../../../context/` — `pitfalls.md` exists there now (cross-repo git forensics); add a `<lib>.md` / `high_level.md` on first use | Same shape; **prune the project-specific figures** — keep the shape there, leave the numbers here |

## Provenance <!-- hp:context-provenance -->

Tag every new fact with how it is known, and **never upgrade a tag without doing the work**:

- `[SOURCE]` — upstream code, cite `path:line` in jesdga95/basalt at a named commit ·
  `[DOC]` — documentation (Basalt's `README.md`/`docs/`, a model card, an upstream issue) at a
  named commit · `[REPO]` — this repo's own code, config or README · `[LIVE YYYY-MM-DD]` —
  observed on a running system: a boot you watched, a bench row, a probe you ran.
- Pitfalls that actually bit are tagged `[OBSERVED YYYY-MM-DD]`, to tell them apart from hazards
  seeded from reading.
- **A dated observation scoped to an environment you cannot enter is superseded, never replaced.**
  Append your measurement with its own date and shell and leave theirs; a newer dated fact may
  contradict it, but you cannot "correct" it — you were not there. Read the `±10` lines for a scope
  qualifier before calling any recorded number stale: **a measurement's scope is part of the
  measurement**, and grep returns lines, not qualifiers
  (`pitfalls.md` § *A dated fact scoped to a shell I could not enter…*).

## House Style <!-- hp:context-style -->

- **Dense.** No fluff — but never compress so far that a detail is lost.
- **No full source code** and nothing private or trademarked. Short algorithm explanations,
  formulas, signatures and architecture ideas are fine.
- Dates are ISO, from `date +%F`.
- A file past ~120 KB is split: `<name>/` with an `index.md` and one file per topic. *(Amended
  2026-11-05 from ~40 KB, the owner's call: the warrant is `pitfalls.md` § *Item numbering, and what is
  frozen* → **Size**. A long file stays usable when its front sheet is short; "Dense" above outranks a
  byte count.)*
- A file that turns out wrong or stale is **fixed in place**, with a dated note of what changed.

## Honesty

**Never claim something is recorded unless the bytes are in the file.** Re-read the file after
writing it. "I noted that in pitfalls.md" is a factual claim about the tree.
