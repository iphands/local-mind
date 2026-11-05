Target dc047a7 / tree 3f7b69ae — if that is not the tree you are looking at, you are in the wrong audit.

# `context/audit/` — Plan 02 artifacts (entry point)

Everything Plan 02 produces, and the tree it describes: `vendor/basalt` @
`dc047a77beeddf1e91234d983a91d6bf929e9619`, tree `3f7b69ae4054969ed15b9b6f02d306eb44e0ebe4`,
frozen 2026-11-05. Plan: [`../plans/02_basalt_vendor_audit.md`](../plans/02_basalt_vendor_audit.md)
· tracker: [`../plans/02_basalt_vendor_audit_tracker.md`](../plans/02_basalt_vendor_audit_tracker.md).

**If you are resuming, read [`manifest/index.md`](manifest/index.md) next** — it holds the schema and
the `reviewed-in` column state, which is what tells you which *file* to open. This page only routes.

## Six commands, in this order

From the project root (`server/basalt`). Every one ends in a sentinel: **an empty result is not a
pass** (`context/pitfalls.md`).

```bash
# 1 · the plan's ONE freeze-check — run this first, always
cd vendor/basalt && GIT_NO_LAZY_FETCH=1 git diff --quiet dc047a77beeddf1e91234d983a91d6bf929e9619 \
  && echo "AUDIT TREE OK" || echo "AUDIT TREE DRIFTED — STOP"; echo "— freeze rc=$?"; cd ../..
# 2 · is the manifest still the 862 rows it was written as?
awk 'END{print NR+0}' context/audit/manifest/*.tsv; echo "— row total rc=$? (expect 862)"
# 3 · can a hand-edit to path/blob/sha256/LOC have escaped?
cd context/audit/manifest && drift=0; while read -r want f; do \
  got=$(LC_ALL=C cut -f1-4 "$f" | sha256sum | cut -d' ' -f1)
  [ "$want" = "$got" ] || { echo "PREFIX DRIFT $f"; drift=$((drift+1)); }
done < frozen-prefix.sha256; echo "PREFIX_DRIFT_COUNT=$drift (expect 0)"; cd ../../..
# 4 · what has been decided, per tier (pasted into manifest/index.md — keep them in step)
LC_ALL=C cut -f5 context/audit/manifest/*.tsv | LC_ALL=C sort | uniq -c
# 5 · what has NOT been read yet
LC_ALL=C cut -f8 context/audit/manifest/*.tsv | LC_ALL=C sort | uniq -c
# 6 · do the manifest and the audited tree still describe the same file set?
LC_ALL=C comm -3 <(LC_ALL=C cat context/audit/manifest/*.tsv | LC_ALL=C cut -f1 | LC_ALL=C sort) \
  <(cd vendor/basalt && GIT_NO_LAZY_FETCH=1 git ls-tree -r dc047a77 --name-only | LC_ALL=C sort) | wc -l
echo "— tree comm rc=$? (expect 0)"
```

2–6 pass at T1 (2026-11-05); their verbatim output is in
[`wave0/t1-freeze-and-manifest.txt`](wave0/t1-freeze-and-manifest.txt).

## Where things live

| Path | Holds | State |
|---|---|---|
| [`manifest/index.md`](manifest/index.md) | **Schema, column ownership, vocabulary, invariants, per-TSV map, definition limits, pin + clone config.** Read it before editing a TSV. | T1 |
| `manifest/*.tsv` | 862 rows, one per tracked file, 12 TSVs, headerless | T1 |
| `manifest/frozen-prefix.sha256` | Per-TSV hash of columns 1–4 (the frozen half of every row) | T1 |
| [`census.md`](census.md) | Primitive census + the 6-canary run | T3 — not written |
| [`argv-table.md`](argv-table.md) | Every exec/spawn/`dlopen` site in C++ **and** Python | T8 — not written |
| [`findings.md`](findings.md) | Capability-ranked findings (🔴🟠🟡⚪) | from T4 |
| [`coverage_ledger.md`](coverage_ledger.md) | Lines read vs present, pasted from commands | T1 seed written (0 lines read) |
| [`verdict.md`](verdict.md) | Channel matrix, depth table, three-sentence answer | T14 — not written |
| `wave<N>/` | **Raw command output. Paste it here, never into a tracker.** `wave0/` = T1 | T1 |

## Coverage at T1 — nothing has been read

Tier A is **pre-assigned from the plan's own `File:` fields**, not from a census result, so the
census cannot shrink what must be read (`manifest/index.md` § invariants — an `A-planned` row may
never become `triaged:*`). Tier B and everything else is `unassigned` until T3.

```text
A-planned (owed a read, none done)   75 f     19,659 l
B-vendored (stb_image.h, version-diffed, not read)  1 f  7,988 l
unassigned (awaits T3)              786 f    191,600 l
                                     ————     ---------
all rows                            862 f    219,247 l
```

Numbers above are the output of `LC_ALL=C cat context/audit/manifest/*.tsv | LC_ALL=C awk -F'\t'
'{L[$5]+=$4; C[$5]++} END{for(t in L) printf "%-18s %4d f %8d l\n", t, C[t], L[t]}'` on 2026-11-05
(pasted, per RULES.md Rule E — a typed coverage figure does not count). **Zero rows are `read`.**
Plan 04's bulk (`src/kernels`, `src/core`, `src/program` minus `serve/`, `include/`) is inside the
`unassigned` figure, and stays there until T11 triages it and Plan 04 reads it.

## Limits of this directory

The manifest is a statement about **paths and bytes at one commit**. It says nothing about behavior:
no file here has been read, no canary has been run, and the pin has not yet been checked against
public GitHub (T2). Docker is unreachable from the agent session, so the built-image questions
(`.pth`/`sitecustomize`, true `DT_NEEDED`) are Plan 03's, owner-run — see the plan's Gate status
block. This whole directory is static evidence; the plan's own verdict wording must remain
conditional on Plan 03 and on the unpinned-wheel hash gap (Risk 5).
