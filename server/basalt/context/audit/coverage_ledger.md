Target dc047a7 / tree 3f7b69ae — if that is not the tree you are looking at, you are in the wrong audit.

# `coverage_ledger.md` — lines read vs lines present, per tier

Every figure here is **pasted from the command named beside it** (RULES.md Rule E: a typed coverage
figure does not count and cannot be cited by a later plan). Reading unit = a commit slice of ≤1,500
Tier A lines (plan Execution rule 1). The row-count and per-tier commands live in
[`manifest/index.md`](manifest/index.md); the `status` and `tier` histograms there must match the row
below them at every commit.

| Date | Slice committed | Rows → `read` | Rows → `triaged` | Lines read (cumulative) | Manifest rows still `pending` | Command |
|---|---|---|---|---|---|---|
| 2026-11-05 | **T1** — manifest written, nothing read | 0 | 0 | **0 of 219,247** | **862 of 862** | `awk 'END{print NR+0}' manifest/*.tsv` → `862`; `LC_ALL=C cut -f8 *.tsv \| LC_ALL=C sort \| uniq -c` → `862 pending`; `LC_ALL=C cat *.tsv \| awk -F'\t' '{L[$5]+=$4} END{for(t in L) …}'` (output at `wave0/t1-freeze-and-manifest.txt`) |

## State at T1 (pasted, 2026-11-05)

```text
A-planned@T4          3 f     2651 l      A-planned@T9          5 f      818 l
A-planned@T5         10 f     1754 l      A-planned@T10        17 f     4908 l
A-planned@T6a         4 f     2538 l      A-planned@T12         2 f      933 l
A-planned@T6b         2 f      612 l      B-vendored@T5         1 f     7988 l
A-planned@T7         10 f     1526 l      unassigned          786 f   191600 l
A-planned@T8         22 f     3919 l      ------------------------------------------
                                          Tier A seeded (76 f)   27647 l   ALL UNREAD
                                          whole tree   862 f    219247 l
```

**Read so far: 0 lines.** `A-planned` is a debt owed by a named task, not coverage. `unassigned` is
not "clean" and not "Tier B" — it is undecided until T3's census assigns it.

## What is *not* in the ledger, and must be stated next to any figure from it

- **Plan 04's bulk is inside the `unassigned` figure**: `src/kernels/` 38,745 l · `src/core/` 19,976 l
  · `src/program/` minus `serve/` 15,432 l · `include/` 18,224 l ≈ 92k lines. T11 triages them with a
  recorded per-file reason; **none is read in this plan.** A future "audit complete" must be read as
  *boundary audit complete; engine bulk triaged, not read*.
- **LOC is a newline count of the blob** and meaningless for the 7 binary assets
  (`manifest/index.md` § definition limits), so the line totals above over-count what T10 owns.
- **Line coverage is not behavioral coverage.** No canary has run (T3), no file has been read, the
  pin is not yet proven against public GitHub (T2), and the runtime leg is Plan 03's.
- The verdict is **conditional on the hash gap** (unpinned wheels, T12/Risk 5): the audited source is
  not yet provably the built artifact.

## How to extend this table

Append one row per commit slice, in commit order. A row is not complete until its `status`/`tier`
histograms in `manifest/index.md` are re-pasted to match, and `PREFIX_DRIFT_COUNT=0` still holds.
When a slice moves a row out of `A-planned@T<n>`, the row count per task must not otherwise change —
**an `A-planned` row that becomes `triaged:*` is a Wave 0 blocker, not bookkeeping**
(`manifest/index.md` § invariants).
