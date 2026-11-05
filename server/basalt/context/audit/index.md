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
awk 'END{print "ROWS="NR+0" (expect 862)"}' context/audit/manifest/*.tsv
# 3 · can a hand-edit to path/blob/sha256/LOC have escaped?
cd context/audit/manifest && drift=0; while read -r want f; do \
  got=$(LC_ALL=C cut -f1-4 "$f" | sha256sum | cut -d' ' -f1)
  [ "$want" = "$got" ] || { echo "PREFIX DRIFT $f"; drift=$((drift+1)); }
done < frozen-prefix.sha256; echo "PREFIX_DRIFT_COUNT=$drift (expect 0)"; cd ../../..
# 4 · what has been decided, per tier (the grid itself is OWNED by manifest/index.md — keep in step)
LC_ALL=C cut -f5 context/audit/manifest/*.tsv | LC_ALL=C sort | uniq -c \
  | LC_ALL=C awk '{print; s+=$1} END{print "TIER_HIST_SUM=" s+0 " (expect 862)"}'
# 5 · what has NOT been read yet
LC_ALL=C cut -f8 context/audit/manifest/*.tsv | LC_ALL=C sort | uniq -c \
  | LC_ALL=C awk '{print; s+=$1} END{print "STATUS_HIST_SUM=" s+0 " (expect 862)"}'
#     (the `print; s+=$1` form matters: `{s+=$1} END{print s}` consumes the histogram and leaves only
#      the total, which is the one number `uniq -c` was not needed to produce)
# 6 · do the manifest and the audited tree still describe the same file set?  (all four cardinalities —
#     `comm -3 | wc -l` alone passes on two empty inputs: context/pitfalls.md item 9)
A=$(mktemp); B=$(mktemp)
LC_ALL=C cat context/audit/manifest/*.tsv | LC_ALL=C cut -f1 | LC_ALL=C sort > "$A"
(cd vendor/basalt && GIT_NO_LAZY_FETCH=1 git ls-tree -r dc047a77 --name-only | LC_ALL=C sort) > "$B"
echo "SET_A=$(grep -c . "$A") SET_B=$(grep -c . "$B") INTER=$(LC_ALL=C comm -12 "$A" "$B" | grep -c .) \
  DIFF=$(LC_ALL=C comm -3 "$A" "$B" | grep -c .) (expect 862 862 862 0)"; rm -f "$A" "$B"
```

2–6 pass at T1 (2026-11-05) and were re-run as pasted at T1; T2 re-ran all six plus `ledger-grid.sh`
and its nine negative controls, transcript with each command above its own output:
[`wave0/t2-gate-and-checks.txt`](wave0/t2-gate-and-checks.txt). Commands 1–3 are the shape the
audit's own checks take; the full manifest check set, with counted sentinels, is
[`manifest/index.md`](manifest/index.md) § Checks, and `ROWS`/`LOC`/tier/wave/binaries are re-derived
in one go by [`scripts/ledger-grid.sh`](scripts/ledger-grid.sh) →
[`wave0/t1-ledger-grid.txt`](wave0/t1-ledger-grid.txt).

These six all talk to **our own** tree. Anything that touches `Niko1221/Strata` or another remote: the
next section, **before you type it**.

## If you are about to run something against upstream, read this first

Because three methods for relating `vendor/basalt` to the project it declares itself
forked from (`Niko1221/Strata`) were run here, gave confident answers, and were wrong. Full record:
[`wave0/t2-lineage.txt`](wave0/t2-lineage.txt) § 0.

- **Do not** probe a peer repo's commit API for our SHAs: GitHub resolves that against the repo's
  object store, not its default branch. **Do not** count or test objects inside a partial clone you
  have already queried: lazy-fetch makes our own objects look like shared ancestry.
- **Do not** expect a merge-base. The two repos share **0** commit SHAs, have different roots, and
  `git log upstream/main..pin` runs and returns a plausible wrong answer instead of an error.
- **Sound method:** a pristine `git fetch --filter=tree:0 --no-tags origin <branch>` into scratch, the
  **promisor full-40-hex** probe (`cat-file -e <full-sha>^{commit}` → `upload-pack: not our ref`) and
  matching on the content key `(tree, author-date, committer-date, subject)`. **Never compare SHAs
  between these two repositories.** Details, controls and the refuted hypotheses: that file, § 0–§ 4.

## Where things live

| Path | Holds | State |
|---|---|---|
| [`manifest/index.md`](manifest/index.md) | **Schema, column ownership, vocabulary, invariants, per-TSV map, definition limits, pin + clone config.** Read it before editing a TSV. | T1 |
| `manifest/*.tsv` | 862 rows, one per tracked file, 12 TSVs, headerless | T1 |
| `manifest/frozen-prefix.sha256` | Per-TSV hash of columns 1–4 (the frozen half of every row) | T1 |
| [`census.md`](census.md) | Primitive census + the 6-canary run | T3 — not written |
| [`argv-table.md`](argv-table.md) | Every exec/spawn/`dlopen` site in C++ **and** Python | T8 — not written |
| [`findings.md`](findings.md) | Capability-ranked findings (🔴🟠🟡⚪) | from T4 |
| [`coverage_ledger.md`](coverage_ledger.md) | Lines read vs present; the grid block is generated, the prose is hand-written | T1 seed written (0 lines read) |
| [`verdict.md`](verdict.md) | Channel matrix, depth table, three-sentence answer | T14 — not written |
| [`scripts/ledger-grid.sh`](scripts/ledger-grid.sh) | The **only generator of coverage figures** here: derives every coverage figure from the TSVs and pins `ROWS`/`LOC`/tier/wave/binary counts. Exits 3 on any FAIL. | T1 |
| [`scripts/t2-census.sh`](scripts/t2-census.sh) | The author census: blame attribution + history breadth per file, at a named pin, with the expected-row-count gate **inside** the producer and an `ERR_BYTES=0` assertion. `OK` certifies line accounting, **not** author identity. | T2 |
| `wave<N>/` | **Raw command output. Paste it here, never into a tracker.** `wave0/` = T1 (`t1-ledger-grid.txt`, the `ledger-grid.sh` negative controls) **+ T2** (`t2-provenance.txt`, `t2-github-tree-paths.txt`, `t2-author-census.tsv`, `t2-boundary-authorship.txt`, `t2-lineage.txt` — **read `t2-lineage.txt` § 0 before re-running any upstream comparison**, `t2-gate-and-checks.txt`) | T1, T2 |

## Coverage at T1 — nothing has been read

Tier A is **pre-assigned from the plan's own `File:` fields**, not from a census result, so the
census cannot shrink what must be read (`manifest/index.md` § invariants — an `A-planned` row may
never become `triaged:*`). Tier B and everything else is `unassigned` until T3.

Roll-ups only — **the per-bucket grid is owned by
[`manifest/index.md`](manifest/index.md) § Lines per tier**, and the generated bytes with their
sentinels are at [`wave0/t1-ledger-grid.txt`](wave0/t1-ledger-grid.txt). Copied verbatim from there,
not recomputed here:

```text
A-planned            75 f    19,659 l     owed a read, none done
B-vendored            1 f     7,988 l     stb_image.h — version-diffed, not read
unassigned          786 f   191,600 l     awaits T3's census
TOTAL               862 f   219,247 l
```

`A-planned` **excludes** `B-vendored`: an earlier revision of `coverage_ledger.md` summed them as
"76 f / 27,647 l" and that figure is retired. **Zero rows are `read`.** Plan 04's bulk
(`src/kernels`, `src/core`, `src/program` minus `serve/`, `include/`) is inside the `unassigned`
figure, and stays there until T11 triages it and Plan 04 reads it.

## Limits of this directory

The manifest is a statement about **paths and bytes at one commit**. It says nothing about behavior:
no file here has been read and no canary has been run. **Byte-identity is not equivalence of
behaviour** — two checkouts whose 862 blobs match are the same *bytes*, and that is the whole claim;
nothing here observes what the code does, and the same bytes can behave differently under a different
toolchain, wheel set or `.pth`/`sitecustomize` state (Risk 5, Plan 03). **The pin has been checked
against public GitHub (T2, 2026-11-05): all 862 blob SHAs match the tree object GitHub serves,
`BLOB_SHA_MISMATCH=0` in both directions, and the comparison is sensitive enough to resolve one changed
file against the pin's first parent.** What that does *not* buy: the signature is GitHub's own on a
GitHub-made squash merge and is unverifiable in this clone (`No public key`), basalt's three release
tags are unsigned, and the pin is untagged `main` — so **nothing outside GitHub anchors this hash.**
SWH does hold the ancestor `Niko1221/Strata` in 9 visits (8 `full`, 1 `failed`) and holds our **root** commit, so the
asymmetry is the finding: the ancestor line has a witness, the audited commit does not
([`manifest/index.md`](manifest/index.md) § Upstream provenance, § Attestation limit).

Two more scope limits, both found by refusing to let an unexplained number pass:

- **This audit covers one ref at one commit.** The audited repository publishes a second branch:
  `refs/remotes/origin/multimodel`, tip `61e26cbb` (2026-11-04), **105 commits ahead** of the pin
  (`multimodel..main` = 3, merge-base `1b3f6964` — basalt's own `v0.2.1` peel target). Those 105
  commits are in no manifest row, no tier and no read queue. Object accounting for the clone is
  otherwise clean — **2,299 commit objects, all reachable from some ref; present-but-unreachable 0** —
  so the 105-object excess over the pin's 2,194 is exactly that branch. Any future statement of the
  form "everything in the repo is accounted for" is false unless it names the ref it means; this
  belongs in `verdict.md`'s conditions and in T13's bump notes.
- **Narrative history in this supply chain is editable, and has been edited.** The ancestor project's
  published `main` shares 0 SHAs with ours while 844 commits match on `(tree, author-date,
  committer-date, subject)`, and the only measured difference across those identical-content pairs is
  that ours carry AI co-authorship trailers (1,049 vs 0) while every human and Codex trailer is
  preserved exactly — with Software Heritage holding Strata's `v0.1.0` at *our* root on 2026-10-05 and
  a different object there today. So: commit messages, author attribution and tag targets are **not**
  evidence in this audit. Hash pins, trees, blobs and patch-ids are. What is *not* established: who
  rewrote it, why, and whether one party controlled that origin URL at both dates
  ([`wave0/t2-lineage.txt`](wave0/t2-lineage.txt) §§ 3–8).
- **The clone is no longer blob-empty.** Blame lazy-fetched blobs into the `blob:none` partial clone
  (~53 MiB across 752 packs as of T2). The T1 `GIT_NO_LAZY_FETCH=1` discipline is for freeze checks, not
  for the census — the census cannot run under it — but it means *object counts and object-presence probes
  in this clone now measure what we asked for*, so re-freeze via `git diff --quiet <pin>` only.

Docker is unreachable
from the agent session, so the built-image questions (`.pth`/`sitecustomize`, true `DT_NEEDED`) are
Plan 03's, owner-run — see the plan's Gate status block. This whole directory is static evidence; the
plan's own verdict wording must remain conditional on Plan 03 and on the unpinned-wheel hash gap
(Risk 5).
