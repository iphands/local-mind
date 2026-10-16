<!-- hands-plan:v1 2026-10-16 -->
# Plan NN — [Title]

> **Status**: pending
> **Created**: YYYY-MM-DD
> **Depends on**: Plan N | N/A
> **Goal**: One-sentence deliverable description.
> **Agent**: implementation agent | ralph-loop | sub-agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: One sentence describing what is being done.

**Deliverables**:
1. Concrete output one
2. Concrete output two

**Estimated effort**: Small (2 h) | Small–Medium (half day) | Medium (1 day) | Large (3 days)

---

## Scope

**(optional) Include when the work must stay inside a boundary; remove otherwise.**

**In**: `path/to/area/` only.
**Out**: everything else. If a fix needs to cross this line, **stop** and record the blocker in
the tracker instead of expanding scope.

---

## Context

Why this plan exists: what prompted it, what problem it addresses, what the intended outcome is.
Cite the `context/` entries you relied on.

### Pre-Identified Bug/Issue

Confirmed problems documented BEFORE work starts. State the evidence — the command you ran
and what it printed, verbatim — not just the claim. A bug described without a reproduction
gets "fixed" without being verified. For a feature: the command showing today's behavior.

```bash
$ <command that reproduces it>
<verbatim output>
```

### Why [Approach]

Justify the design choice against the alternative you rejected, and why it lost.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| <fact the plan depends on> | <value> | `<command>` on YYYY-MM-DD / `path:line` / unconfirmed — confirm in T1 |

### Rejected Claims

**(optional) Things that look like bugs or obvious fixes but are not — "do NOT re-fix", with
the evidence. Saves the next agent from "fixing" a non-bug.**

---

## Step-by-Step Tasks

**Large plans:** group tasks into waves and state the exit gate of each wave
("Wave 1 exits when …"). Tasks inside a wave may run in parallel.

### T1: [Task title]

**File**: `path/to/file.ext`

**What to do**: Detailed instructions — exact function names, line numbers, offsets where known.
Cross-reference sources (`path:line`) when the behavior has a spec.

**Before**:
```rust
<old code — enough surrounding context to locate the hunk uniquely>
```

**After**:
```rust
<new code>
```

**Verify**:
```bash
<command that exercises THIS task's change>    # → <expected observable output>
cargo check --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check && cargo test --workspace --lib
```
Expected: <the observable result that proves this task works — not just "the build passes">.

**Expected observation**: *(measurement / investigation tasks — write this BEFORE running anything)*
- **Confirms if**: …
- **Refutes if**: …
- **Within noise if**: …

**Commit**: `[proxy][P<n>][T1][<topic>] <short description>` — *commit before marking done (Rule B), tracker row in the same commit; the scope tag must match RULES.md Rule B.*

### T2: [Task title]

*(Same shape. Repeat per task.)*

---

## Critical Files

| File | Change | Priority |
|---|---|---|
| `path/to/file.ext` | Description of change | P0 |

Priority values: `P0` = blocking, `P1` = important, `P2` = nice-to-have.

---

## Open Questions / Risks

1. **Risk: [what could go wrong].** *Mitigation*: [what makes it safe].
2. **Question: [what is undecided].** *How we'll settle it*: [who decides, and when].

**Never delete a question because it went unanswered** — resolve it in place
(~~strike~~ + "RESOLVED (T3): …") or mark it "deferred — <reason>" when you defer it.

---

## Verification Checklist

**One checkbox per task. Each must be a testable assertion with an OBSERVABLE result** —
"looks correct" is not an assertion. Tick a box only against evidence you produced; an
untickable box stays unticked with a note saying why.

- [ ] T1: <command> → <expected observable result>
- [ ] T2: …
- [ ] All: the project gate (Rule A) passes on the final commit
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
