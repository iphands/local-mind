Target dc047a7 / tree 3f7b69ae — if that is not the tree you are looking at, you are in the wrong audit.

# `manifest/` — every tracked file of `vendor/basalt` at the audited pin

862 rows in 12 TSVs, one row per tracked file of `vendor/basalt` @
`dc047a77beeddf1e91234d983a91d6bf929e9619` (tree `3f7b69ae4054969ed15b9b6f02d306eb44e0ebe4`).
Written by **Plan 02 T1** on 2026-11-05. This is the artifact that makes "file by file" countable:
a coverage claim that cannot be reconciled against these rows is not a coverage claim.
Entry point: [`../index.md`](../index.md).

## Rule 6 exemption — declared, not silently taken

Execution rule 6 stamps every `context/audit/` file. **The 12 `*.tsv` files are exempt**, because the
schema forbids any header or comment row: the row count must equal the file count and `cut -f1` must
be a clean path set. The stamp lives in this file and in `../index.md`, nowhere else. **Never "fix" a
TSV by adding a header line** — it silently breaks every count on this page. (Declared by T1,
2026-11-05; this is the only place a later session will look for it.)

## Schema — 9 columns, tab-separated, **no header row**

| # | Column | Contents | Who may write it, and until when |
|---|---|---|---|
| 1 | `path` | path relative to `vendor/basalt` | **frozen after T1** |
| 2 | `blob-id` | git SHA-1 over git's `blob <len>\0` framing | **frozen after T1** |
| 3 | `sha256` | sha256 of the blob's bytes | **frozen after T1** |
| 4 | `LOC` | newline count of the blob (`git cat-file blob \| wc -l`) | **frozen after T1** |
| 5 | `tier` | disposition: `A-planned@T<n>` / `B-vendored@T<n>` / `read@T<n>` / `triaged:<reason>@T<n>` / `unassigned` | T1 seeded Tier A; **T3 onward** |
| 6 | `primitives` | comma-separated, no spaces, alphabetical (closed vocabulary below) | **T3** |
| 7 | `wave` | wave in which the row's disposition is decided (`0`–`4`) | T1 seeded where the plan fixes it; **T3** for the rest |
| 8 | `status` | `pending` / `read` / `triaged` / `escalated` | the reading slice that owns the row |
| 9 | `reviewed-in` | `pending`, or the task id that flipped it (`T5`, `T11a`, …) | the reading slice that owns the row |

**Columns 1–4 are facts about a frozen tree; 5–9 are the work log.** Nothing else may be edited, and
`frozen-prefix.sha256` exists to prove when someone does.

Never read the schema from memory — decode a real row (`context/audit/manifest/`):

```bash
head -1 src.tsv | LC_ALL=C awk -F'\t' '{split("path blob-id sha256 LOC tier primitives wave status reviewed-in",n," "); for(i=1;i<=9;i++) printf "f%-2s %-13s %s\n", i, n[i], $i}'
```

```console
f1  path          src/artifact/dequant.cpp
f2  blob-id       26031fd4317d67f7b1f68140c97321eed27eb013
f3  sha256        2000af33c29d7bef67f126639cbb70900bf8b3ea9aca22ef4774b21985771483
f4  LOC           123
f5  tier          unassigned
f6  primitives    unassigned
f7  wave          unassigned
f8  status        pending
f9  reviewed-in   pending
```

## Pin and clone config (recorded by T1, 2026-11-05)

```text
BASALT_SHA  dc047a77beeddf1e91234d983a91d6bf929e9619          (= vendor HEAD)
tree        3f7b69ae4054969ed15b9b6f02d306eb44e0ebe4          (= HEAD^{tree})
remote.origin.url               https://github.com/jesdga95/basalt.git
remote.origin.fetch             +refs/heads/*:refs/remotes/origin/*
remote.origin.promisor          true
remote.origin.partialclonefilter blob:none
core.repositoryformatversion    1          (extensions — required: partial clone)
.git/objects/info/alternates    ABSENT     (only commit-graphs/ in that dir)
core.objectsfile                UNSET      (git config --local --get → rc=1)
worktree vs commit              862/862 paths: git hash-object == ls-tree blob-id, under GIT_NO_LAZY_FETCH=1
git-free cross-check            git ls-files -z | xargs -0 sha256sum → 862 lines, 0 differ from column 3
git status --porcelain --ignored  →  VENDOR_DIRTY=0
object census                   862 objects, ALL type `blob`: 858 × mode 100644, 4 × mode 100755
path hygiene                    0 paths outside ^[\x21-\x7e]+$ — no spaces, no quoting, no non-ASCII
```

No gitlinks and no symlinks: **every one of the 862 rows points at real content**, so "all 862 files"
is a claim about bytes, not just names. The partial clone cannot corrupt content-addressed bytes; its
risk is silent skips, which is why every read here runs under `GIT_NO_LAZY_FETCH=1`.

## The 12 TSVs — where a path lives

| TSV | Covers (path prefix) | Rows | LOC |
|---|---|---|---|
| `assets.tsv` | `assets/` | 3 | 224 |
| `cmake.tsv` | `cmake/` | 2 | 62 |
| `data.tsv` | `data/` | 5 | 966 |
| `docs.tsv` | `docs/` | 31 | 6,352 |
| `github.tsv` | **`.github/`** | 3 | 231 |
| `include.tsv` | `include/` | 161 | 18,224 |
| `root.tsv` | depth-1 files (`.gitattributes`, `.gitignore`, `AGENTS.md`, `CMakeLists.txt`, `CONTRIBUTING.md`, `LICENSE`, `README.md`, `requirements-test.txt`, `requirements.txt`, `THIRD_PARTY_LICENSES`) | 10 | 2,049 |
| `serve.tsv` | `serve/` **including `serve/tests/`** | 94 | 24,604 |
| `site.tsv` | `site/` | 1 | 544 |
| `src.tsv` | `src/` | 353 | 106,928 |
| `tests.tsv` | `tests/` | 163 | 51,125 |
| `tools.tsv` | `tools/` | 36 | 7,938 |
| | | **862** | **219,247** |

Rule: **the first path element wins**, so `serve/tests/test_image_sources.py` → `serve.tsv`; depth-1
files → `root.tsv`. `.github/` is filed as **`github.tsv`** with the dot dropped on purpose: a
`.github.tsv` is invisible to `manifest/*.tsv` and would silently drop 3 rows from every count in this
audit — the exact failure the counts exist to catch. (T1 hit this live: an early draft wrote
`.github.tsv` and the row total read 859.)

Two cross-check traps: `CMakeLists.txt` is in `root.tsv`, so `cmake.tsv` = 2 rows can never match the
plan's Key Facts row "`cmake/` + `CMakeLists.txt` = 3 f / 1,569 l"; and `tests.tsv` (163 f) is **not**
where vendor's Python tests live — `serve/tests/` (53 f) is in `serve.tsv`.

```bash
grep -lF 'serve/tests/test_image_sources.py' context/audit/manifest/*.tsv      # find a file's home
LC_ALL=C awk -F'\t' '$1 ~ /^src\/kernels\//' context/audit/manifest/src.tsv | wc -l   # 86 — dir selector
```

## Column state: tier / primitives / wave / status / reviewed-in (pasted, never typed)

`context/audit/manifest/`, 2026-11-05 (T1). Re-paste these in every commit that flips rows; the
figures in `../coverage_ledger.md` must match them.

```console
$ LC_ALL=C cut -f5 *.tsv | LC_ALL=C sort | uniq -c
     17 A-planned@T10
      2 A-planned@T12
      3 A-planned@T4
     10 A-planned@T5
      4 A-planned@T6a
      2 A-planned@T6b
     10 A-planned@T7
     22 A-planned@T8
      5 A-planned@T9
      1 B-vendored@T5
    786 unassigned

$ LC_ALL=C cut -f6 *.tsv | LC_ALL=C sort | uniq -c      # primitives — T3 has not run
    862 unassigned
$ LC_ALL=C cut -f7 *.tsv | LC_ALL=C sort | uniq -c      # wave (filled exactly where tier is)
     52 1
     22 2
      2 3
    786 unassigned
$ LC_ALL=C cut -f8 *.tsv | LC_ALL=C sort | uniq -c      # status
    862 pending
$ LC_ALL=C cut -f9 *.tsv | LC_ALL=C sort | uniq -c      # reviewed-in
    862 pending
```

Lines per tier — the seed of `coverage_ledger.md`, same command as in `../index.md`:

```text
A-planned@T4    3 f   2,651 l   A-planned@T8   22 f   3,919 l
A-planned@T5   10 f   1,754 l   A-planned@T9    5 f     818 l
A-planned@T6a   4 f   2,538 l   A-planned@T10  17 f   4,908 l
A-planned@T6b   2 f     612 l   A-planned@T12   2 f     933 l
A-planned@T7   10 f   1,526 l   B-vendored@T5   1 f   7,988 l
                                unassigned    786 f 191,600 l
```

### Vocabulary — closed, and the two words that must never swap

- **`tier`**: `A-planned@T<n>` = a Tier A target named in this plan's `File:` fields, **not yet read**.
  `B-vendored@T<n>` = third-party source, version-diffed as a dependency, never read as first-party.
  `read@T<n>` = every line read by that task. `triaged:<reason>@T<n>` = not read; the reason is one of
  `no-primitives` · `numerical-kernel` · `vendored-third-party` · `test-only` · `docs-only` ·
  `build-config` · `data-blob` · `deferred-plan04`. `unassigned` = nobody has decided yet.
  A reason not on that list needs a dated line here before it may be used.
- **`primitives`**: comma-separated, no spaces, alphabetical, from T3's census sections —
  `net-socket` · `dns` · `exec-spawn` · `dlopen` · `raw-syscall` · `file-write-outside` ·
  `fingerprint-read` · `credential-store` · `proc-sys` · `persistence` · `input-capture` ·
  `parser-network-bytes` · `vendored-third-party` · `orphan-config-key` · `diagnostics-shaped`.
  Histogram: `cut -f6 *.tsv | tr ',' '\n' | LC_ALL=C sort | uniq -c | sort -rn`.
- **`unassigned` ≠ `none`.** `unassigned` = nobody looked. `none` = looked, nothing found. Only T3
  may write `none`. Collapsing the two is how "the grep found nothing" becomes "audited".
- **`pending`** in both `status` and `reviewed-in` means the work has not happened. It is deliberately
  a *boring* word: `status` must never read as completed before it is. `-` is not used, because it
  cannot tell "not yet" from "never — this is Plan 04's".

### Invariants (a violation is a blocker, not a cleanup)

1. **An `A-planned@T<n>` row may only ever become `read@T<n>`.** Never `triaged:*`. Tier A here was
   fixed by the plan's own scope *before* the census ran, precisely so a negative census result cannot
   reclassify a boundary file as "no primitives, skip it". If T3 does not assign such a row, that is a
   Wave 0 blocker.
2. `tier` is filled **iff** `wave` is filled (checked below; 0 violations at T1).
3. T3 may **add** Tier A rows; it may not remove them. Any later change to a column-1–4 value is
   tampering — see `PREFIX DRIFT`.
4. Every row a commit flips lands in that commit (RULES.md Rule B); a slice flips *all* its rows.

## Checks — run from `context/audit/manifest/`

```bash
# rows present (awk, not wc: a missing final newline makes wc lie)
awk 'END{print NR+0}' *.tsv; echo "— expect 862 rc=$?"
# every row really has 9 fields (a short row makes `cut -f5` emit the WHOLE line and poison every uniq -c)
LC_ALL=C awk -F'\t' 'NF!=9{print FILENAME":"FNR" has "NF" fields"}' *.tsv; echo "— fields rc=$? (silent = ok)"
# every row filed in the TSV its first path element implies
LC_ALL=C awk -F'\t' '{n=split($1,a,"/"); want=(n==1?"root":(a[1]==".github"?"github":a[1])) ".tsv"; \
  f=FILENAME; sub(/.*\//,"",f); if(f!=want) print "MISFILED",$1,FILENAME,want} END{print "filing done"}' *.tsv
# no dot-prefixed TSV hiding from *.tsv
find . -maxdepth 1 -name '.*tsv' -print; echo "— dot-tsv rc=$? (silent = ok)"
# every TSV ends in a newline (a missing one makes a row count lie) — the signal is the printed 0,
# not the loop's rc
nl=0; for f in *.tsv; do [ -n "$(tail -c1 "$f")" ] && { echo "NO-FINAL-NEWLINE $f"; nl=$((nl+1)); }; done
echo "NO_FINAL_NEWLINE_COUNT=$nl (expect 0)"
# rows are in LC_ALL=C order of column 1 *within each TSV* (= git ls-tree order), so later `comm`s
# need no re-sort
for f in *.tsv; do LC_ALL=C cut -f1 "$f" | LC_ALL=C sort -c || echo "UNSORTED $f"; done; echo "order ok"
# manifest vs the audited TREE (not the index) — the completeness claim
LC_ALL=C comm -3 <(LC_ALL=C cut -f1 *.tsv | LC_ALL=C sort) \
  <(GIT_NO_LAZY_FETCH=1 git -C ../../../vendor/basalt ls-tree -r dc047a77 --name-only | LC_ALL=C sort) | wc -l
echo "— tree comm rc=$? (expect 0)"
# columns 1–4 unchanged — frozen-prefix.sha256 is NOT `sha256sum -c` material (it hashes cut -f1-4,
# not whole files; `-c` reports 12 bogus FAILs). Use the loop:
drift=0; while read -r want f; do got=$(LC_ALL=C cut -f1-4 "$f" | sha256sum | cut -d' ' -f1); \
  [ "$want" = "$got" ] || { echo "PREFIX DRIFT $f"; drift=$((drift+1)); }; done < frozen-prefix.sha256
echo "PREFIX_DRIFT_COUNT=$drift (expect 0)"; echo "— prefix rc=$?"
# LOC column re-derived from the objects (all 862)
GIT_NO_LAZY_FETCH=1 git -C ../../../vendor/basalt ls-files >/dev/null   # prove git works, loudly
cut -f2,4 *.tsv | while IFS=$'\t' read -r h l; do \
  [ "$(GIT_NO_LAZY_FETCH=1 git -C ../../../vendor/basalt cat-file blob "$h" | wc -l)" = "$l" ] \
  || echo "LOC MISMATCH $h"; done; echo "— LOC rc=$? (silent = ok)"
```

Aggregate identity of columns 1–4 over all 862 rows (re-run and compare; a change means the frozen
half of the manifest was edited — stop and file a blocker):

```bash
LC_ALL=C cut -f1-4 *.tsv | LC_ALL=C sort | sha256sum
# 34711ffc30f1ead441beba183190a873fbedd002809723cc264ed4a2e91a0eae  (T1, 2026-11-05)
```

**Negative control on the drift check** (RULES.md: a clean result from a check that has never fired is
worth nothing). Tamper with a LOC digit in a **scratch copy**, never here, and the loop must report
exactly one drift:

```bash
cp -r . /tmp/opencode/basalt-audit/pfxctl && cd /tmp/opencode/basalt-audit/pfxctl
sed -i '1s/\t[0-9]\+\tunassigned\tunassigned\tunassigned\tpending\tpending$/\t999999\tunassigned\tunassigned\tunassigned\tpending\tpending/' cmake.tsv
# → PREFIX DRIFT cmake.tsv / CONTROL_DRIFT_COUNT=1   (observed 2026-11-05)
```

## How these rows were generated, and how to prove them again

`sha256` and `LOC` are computed over the **commit object**, not the worktree; each row's blob-id is
re-derived from the worktree file **inside the same loop**, so path↔blob↔bytes are one measurement
rather than two that could straddle a change. Run from `vendor/basalt`:

```bash
pin=dc047a77beeddf1e91234d983a91d6bf929e9619; out=/tmp/opencode/basalt-audit/regen; rm -rf "$out"; mkdir -p "$out"
GIT_NO_LAZY_FETCH=1 git ls-tree -r "$pin" --format='%(objectname)	%(path)' \
| while IFS=$'\t' read -r h p; do
    [ "$(git hash-object -- "$p")" = "$h" ] || { echo "MISMATCH $p" >&2; continue; }
    printf '%s\t%s\t%s\t%s\n' "$p" "$h" "$(git cat-file blob "$h" | sha256sum | cut -d' ' -f1)" \
                               "$(git cat-file blob "$h" | wc -l)"
  done > /tmp/opencode/basalt-audit/regen4.tsv; echo "— gen rc=$?"; wc -l < /tmp/opencode/basalt-audit/regen4.tsv
```

…then the tier/wave seeding and the 12-way split exactly as the tier rules below require (the full
pipeline is in `../wave0/t1-freeze-and-manifest.txt`). **Regeneration is allowed and cheap**: write to
`/tmp/opencode/basalt-audit/`, then

```bash
diff -r /tmp/opencode/basalt-audit/regen /home/iphands/prog/ai/local-mind/server/basalt/context/audit/manifest
```

If the TSVs differ by anything other than columns 5–9, paste the diff and **stop**. Two back-to-back
runs of the T1 pipeline were byte-identical (verified 2026-11-05), so a difference is a real change,
not flake.

### Tier A seeded at T1 — the exact rule set

Taken from Plan 02's `File:` fields, first match wins, **before** any census ran (Invariant 1):

| Seeded as | Files (globs are the rule, this is the reading) |
|---|---|
| `A-planned@T4` | `serve/engine/image_source.py`, `serve/security.py`, `serve/tests/test_image_sources.py` |
| `B-vendored@T5` | `src/vision/stb_image.h` — the one file the plan assigns Tier B up front (`vendored-third-party`) |
| `A-planned@T5` | everything else under `src/vision/` + `src/program/serve/vision.cpp` |
| `A-planned@T6a` | `serve/util/logbuf.py`, `serve/util/telemetry.py`, `serve/service.py`, `serve/handler.py` |
| `A-planned@T6b` | `serve/config.py`, `serve/engine/modelfile.py` |
| `A-planned@T7` | `serve/chat_template.jinja`, `serve/prompt.py`, `serve/api/*`, `tools/tokenizer.py` |
| `A-planned@T8` | `src/platform/*`, `include/basalt/platform/*`, `tools/check_platform_boundary.sh` |
| `A-planned@T9` | `src/program/serve/loans.cpp`, `src/program/serve/loan_moves.cpp`, `src/program/multi_gpu_plan.cpp`, `src/program/image_loan.cpp`, `src/program/generate/configure.cpp` |
| `A-planned@T10` | `data/*`, `serve/web/*`, `assets/*`, `docs/img/frontend.gif`, `THIRD_PARTY_LICENSES` |
| `A-planned@T12` | `requirements.txt`, `docs/community/benchmarks.json` |

Overlaps land on the earliest task: `src/vision/mmproj.cpp` is T5's ("the rest of `src/vision/`") even
though T9 also names it; `serve/chat_template.jinja` is T7's though T10 scans it; `THIRD_PARTY_LICENSES`
is T10's though T12 names it. **T9's remainder is not seeded** — it is whatever the census assigns, and
T9's file list freezes at Wave 0 exit with a line count (Risk 9). Because the schema has no
`assigned-TN` column, **this file's `tier` column is where ownership is recorded**: at Wave 0 exit, the
commit that freezes T9's list also flips those rows to `A-planned@T9` / wave `2`, and the two must
agree — a row `census.md` assigns to T9 but that still reads `unassigned` here is unowned work and the
Wave 0 gate fails. Waves: 1 = T4–T8, 2 = T9–T11d, 3 = T12–T13, 4 = T14 (tracker Resume step 5).

## What the columns do **not** mean

- **`LOC`** is a newline count of the blob at this pin. 12 of 862 blobs have no trailing newline
  (measured 2026-11-05, od-based; a `$(…)`-based count reads 9 because bash drops NUL bytes from
  binaries) — each of those undercounts by exactly 1. 0 blobs are empty. A Plan 04 session re-deriving
  LOC with an editor's counter will get different numbers; that is not drift.
- **LOC is meaningless for the 7 binary files** (`assets/*.png`, `docs/assets/basalt-format.png`,
  `docs/img/frontend.gif`, `serve/web/fonts/*.woff2`, `data/profiles/qwen4exp-48x512.prof`). T10 owns
  them and Tier A there means "fully explained by its parser/consumer", not "lines read".
- **`blob-id` and `sha256` are different identifiers over the same bytes** — git's SHA-1 over
  `blob <len>\0` framing vs sha256 of the bare bytes. **Never compare them.**
- Columns 2–4 were read from the **object store**. The out-of-band anchors are the recorded
  worktree `sha256sum` pass (0 differences, `wave0/`) and T2's GitHub-side tree SHA. If the object
  store itself is the suspect, only the latter settles it.
- `A-planned` is a **debt**, not a result. Nothing here has been read.
- Our own `run`, `lib/common.sh`, `Dockerfile`, `versions.env` (T13) are **not** in this manifest: it
  describes `vendor/basalt` only.

## Upstream provenance (T2 lands here)

*Reserved deliberately. T2 records GitHub's tree SHA for `dc047a7`, tag/signature status, and the
boundary-file author census. Do not restructure the sections above to hold it.*
