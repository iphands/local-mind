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

Two more files take the same exemption, for the same reason, and are declared here rather than
decided silently: **`../wave0/binary-paths.txt`** (one path per line, consumed by `comm`/`diff` against
`git ls-tree`, so a comment line would read as a missing file) and the `../wave0/*.tsv`-style raw
captures, whose first line is whatever the command printed. `../wave0/t1-ledger-grid.txt` carries the
stamp anyway because `ledger-grid.sh` emits it; the hand-captured ones do not.

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

### Lines per tier — **this file owns the grid**; `coverage_ledger.md` and `../index.md` cite it

One line per `tier` value (column 5), `LOC` (column 4) summed per bucket, `LC_ALL=C`-sorted. Generated
by [`../scripts/ledger-grid.sh`](../scripts/ledger-grid.sh); the bytes and their sentinels are at
[`../wave0/t1-ledger-grid.txt`](../wave0/t1-ledger-grid.txt). The bucket names carry the task that owes
the reading, and they mutate as tasks seed rows — that is the data, not a formatting choice.

```text
# audited pin dc047a7 / tree 3f7b69ae · ROWS=862 LOC=219247 · 2026-11-05
A-planned@T10        17 f     4,908 l
A-planned@T12         2 f       933 l
A-planned@T4          3 f     2,651 l
A-planned@T5         10 f     1,754 l
A-planned@T6a         4 f     2,538 l
A-planned@T6b         2 f       612 l
A-planned@T7         10 f     1,526 l
A-planned@T8         22 f     3,919 l
A-planned@T9          5 f       818 l
B-vendored@T5         1 f     7,988 l
unassigned          786 f   191,600 l

A-planned            75 f    19,659 l
B-vendored            1 f     7,988 l
unassigned          786 f   191,600 l
TOTAL               862 f   219,247 l
```

**`A-planned` is 75 f / 19,659 l and does not include `B-vendored`.** An earlier revision of
`coverage_ledger.md` totalled the two as "Tier A seeded (76 f) 27,647 l"; that figure is retired — the
7,988-line `src/vision/stb_image.h` is version-diffed as a dependency and is never first-party reading
debt. Quote 19,659 or quote the grid, never a sum of the two.

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
# Every check below prints `<NAME>=<actual> (expect <literal>)`. A check that prints nothing, or an
# `rc=$?` on a loop, is NOT a pass: awk and while-loops exit 0 after reporting problems. `ledger-grid.sh`
# (in ../scripts/) runs the manifest-shape half of this list and exits 3 on any FAIL.
# rows present (awk, not wc: a missing final newline makes wc lie)
awk 'END{print "ROWS="NR+0" (expect 862)"}' *.tsv
# every row really has 9 fields (a short row makes `cut -f5` emit the WHOLE line and poison every uniq -c)
LC_ALL=C awk -F'\t' 'NF!=9{print FILENAME":"FNR" has "NF" fields"; n++} END{print "BAD_FIELD_ROWS="n+0" (expect 0)"}' *.tsv
# every row filed in the TSV its first path element implies
LC_ALL=C awk -F'\t' '{n=split($1,a,"/"); want=(n==1?"root":(a[1]==".github"?"github":a[1])) ".tsv"; \
  f=FILENAME; sub(/.*\//,"",f); if(f!=want){print "MISFILED",$1,FILENAME,want; bad++}} \
  END{print "MISFILED_ROWS="bad+0" (expect 0)"}' *.tsv
# no dot-prefixed TSV hiding from *.tsv, and nothing linked in: the glob and `find` must agree
echo "DOT_TSVS=$(find . -maxdepth 1 -name '.*tsv' | wc -l) (expect 0)"
echo "TSV_GLOB=$(ls *.tsv | wc -l) TSV_FIND=$(find . -maxdepth 1 -name '*.tsv' -type f | wc -l) (expect 12 12)"
# every TSV ends in a newline (a missing one makes a row count lie). awk counts records, wc counts
# newline BYTES: they can only differ if a TSV lost its final newline, which tells a lost row apart
# from a lost newline — two independent zeros cannot.
nl=0; for f in *.tsv; do if [ -n "$(tail -c1 "$f")" ]; then echo "NO-FINAL-NEWLINE $f"; nl=$((nl+1)); fi; done
echo "NO_FINAL_NEWLINE_COUNT=$nl (expect 0)"
echo "RECORDS=$(cat *.tsv | awk 'END{print NR+0}') NEWLINES=$(cat *.tsv | wc -l) (must be equal)"
# rows are in LC_ALL=C order of column 1 *within each TSV* AND that order is `git ls-tree -r` order
# restricted to the TSV's set — verified per TSV, not asserted (2026-11-05: 0 mismatches, 862 covered).
pin=dc047a77beeddf1e91234d983a91d6bf929e9619
GIT_NO_LAZY_FETCH=1 git -C ../../../vendor/basalt ls-tree -r --name-only "$pin" > /tmp/opencode/basalt-audit/ls.txt
obad=0; cov=0
for f in *.tsv; do
  LC_ALL=C cut -f1 "$f" > /tmp/opencode/basalt-audit/a.txt
  case "$f" in github.tsv) w='^\.github/';; root.tsv) w='^[^/]*$';; *) w="^${f%.tsv}/";; esac
  LC_ALL=C grep -E "$w" /tmp/opencode/basalt-audit/ls.txt > /tmp/opencode/basalt-audit/b.txt
  cov=$((cov + $(wc -l < /tmp/opencode/basalt-audit/b.txt)))
  LC_ALL=C cmp -s /tmp/opencode/basalt-audit/a.txt /tmp/opencode/basalt-audit/b.txt \
    || { echo "ORDER-DIVERGES $f"; obad=$((obad+1)); }
done
echo "ORDER_MISMATCH_TSVS=$obad (expect 0)  SUM_OF_WANTED_SETS=$cov (expect 862)"
# manifest vs the audited TREE (not the index) — the completeness claim
echo "TREE_COMM=$(LC_ALL=C comm -3 <(LC_ALL=C cut -f1 *.tsv | LC_ALL=C sort) \
  <(GIT_NO_LAZY_FETCH=1 git -C ../../../vendor/basalt ls-tree -r dc047a77 --name-only | LC_ALL=C sort) | wc -l) (expect 0)"
# columns 1–4 unchanged — frozen-prefix.sha256 is NOT `sha256sum -c` material (it hashes cut -f1-4,
# not whole files; `-c` reports 12 bogus FAILs). Use the loop:
drift=0; while read -r want f; do got=$(LC_ALL=C cut -f1-4 "$f" | sha256sum | cut -d' ' -f1); \
  [ "$want" = "$got" ] || { echo "PREFIX DRIFT $f"; drift=$((drift+1)); }; done < frozen-prefix.sha256
echo "PREFIX_DRIFT_COUNT=$drift (expect 0)"
# LOC column re-derived from the objects (all 862)
GIT_NO_LAZY_FETCH=1 git -C ../../../vendor/basalt ls-files >/dev/null   # prove git works, loudly
locbad=0; while IFS=$'\t' read -r h l; do \
  [ "$(GIT_NO_LAZY_FETCH=1 git -C ../../../vendor/basalt cat-file blob "$h" | wc -l)" = "$l" ] \
  || { echo "LOC MISMATCH $h"; locbad=$((locbad+1)); }; done < <(cut -f2,4 *.tsv)
echo "LOC_MISMATCH_COUNT=$locbad (expect 0)"
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

That `sed -i` is the **only** sanctioned in-place edit in this directory, and it is sanctioned because
the bytes it edits are a scratch copy under `/tmp/opencode/basalt-audit/`. Nothing in
`context/audit/**` is ever edited with `sed -i`, and the generated grid blocks are replaced by
re-running the generator and re-pasting, not patched in place. Negative controls for
`ledger-grid.sh` itself — nine tampers, every one of which had to abort non-zero, two of which initially
failed the *script* rather than the data — are at
[`../wave0/t1-grid-controls.txt`](../wave0/t1-grid-controls.txt).

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
- **LOC is meaningless for the 7 binary files** listed in [`../wave0/binary-paths.txt`](../wave0/binary-paths.txt)
  (`assets/*.png`, `docs/assets/basalt-format.png`, `docs/img/frontend.gif`, `serve/web/fonts/*.woff2`,
  `data/profiles/qwen4exp-48x512.prof`). Their `f4` newline counts total **4,000 l** (`BINARY_ROWS 7`,
  `BINARY_LOC 4000` in `../wave0/t1-ledger-grid.txt`) and **stay inside the 219,247** — subtracting
  them would break the `LOC` and `PARTITION_ROWS` pins. That list is derived by an `iconv` round-trip
  over all 862 paths (content test, re-runnable: 7 non-UTF-8 of 862); `ledger-grid.sh` finds the same 7
  by extension because a script that reads no file content can do nothing else. T10 owns
  them and Tier A there means "fully explained by its parser/consumer", not "lines read".
- **`blob-id` and `sha256` are different identifiers over the same bytes** — git's SHA-1 over
  `blob <len>\0` framing vs sha256 of the bare bytes. **Never compare them.**
- Columns 2–4 were read from the **object store**. The out-of-band anchors are the recorded
  worktree `sha256sum` pass (0 differences, `wave0/`) and T2's GitHub-side tree SHA. If the object
  store itself is the suspect, only the latter settles it.
- `A-planned` is a **debt**, not a result. Nothing here has been read.
- Our own `run`, `lib/common.sh`, `Dockerfile`, `versions.env` (T13) are **not** in this manifest: it
  describes `vendor/basalt` only.

## Upstream provenance (T2, 2026-11-05)

**Pin provenance.** Measured with `curl` over TLS to `api.github.com`, unauthenticated, on
2026-11-05 (`gh` is not installed here). Our `dc047a77^{tree}` = `3f7b69ae4054969ed15b9b6f02d306eb44e0ebe4`
= the tree GitHub reports for that commit. GitHub's tree object: **942 entries (862 blobs + 80 trees),
`truncated=false`**. Compared in **both directions**: `INTERSECT=862`, `ONLY_IN_API=0`,
`ONLY_IN_LOCAL=0`, `BLOB_SHA_MISMATCH=0`; `0` gitlinks, `0` symlinks; modes `858×100644 / 4×100755`.
Canonical `sha256(path⇥blob⇥…)` over all 862 paths, upstream and local, both
`a8216c6983f4d6345d1b67751b7242802f03a9a031a45822e9c90290b21bb857`. Sensitivity control: the same
comparison against the pin's **first parent** returns `BLOB_SHA_MISMATCH=1` — it resolves to one
changed file (`serve/util/telemetry.py`) out of 862. Raw:
[`../wave0/t2-provenance.txt`](../wave0/t2-provenance.txt) · the witness pairs:
[`../wave0/t2-github-tree-paths.txt`](../wave0/t2-github-tree-paths.txt).

**Attestation limit — read this as the ceiling, not as "verified upstream".** Every number above is
measured against **one witness**: an unauthenticated HTTPS request to GitHub. The commit is signed by
GitHub (`B5690EEEBB952194`) for a GitHub-created squash-merge; `git log --show-signature` in this
clone says *"Can't check signature: No public key"*, and the only verification of that signature is
GitHub's own API. **One authority, attesting itself.** Established: the objects we are auditing are
exactly the objects GitHub serves for `jesdga95/basalt` `main`. **Not** established: that those
objects are what the authors intended to publish. Three further holes, all measured rather than
assumed:

- Basalt's **own** three release tags are annotated **but unsigned** (0 PGP blocks each), so no
  contributor key anchors any history either. `v0.1.0 421063f0`/`v0.2.0 f0bb833c`/`v0.2.1 1b3f6964`
  are all ancestors of the pin; **3 commits** sit between basalt's `v0.2.1` and it.
  CORRECTED 2026-11-05: this bullet previously just said "the three release tags", which reads as
  *Strata's* — they are not, and they are not comparable. `Niko1221/Strata` publishes **95** tag refs
  reaching `v0.2.10`, and its `v0.1.0` (`01529aaa…`) and `v0.2.1` (`49ac15cb…`) are **different
  objects**. The two projects share version *names* and nothing else, so **every version string in
  this audit must name its repository**; where this corpus says an unqualified `v0.2.1`, it means
  basalt's. See [`../wave0/t2-lineage.txt`](../wave0/t2-lineage.txt) § 9.
- **Nothing outside GitHub anchors the pinned commit.** Software Heritage returns `404 NotFound` for
  both the revision `dc047a77…` and an origin for `https://github.com/jesdga95/basalt`, so a
  force-push predating our fetch would be undetectable from this clone.
  CORRECTED 2026-11-05: this bullet previously read "**Software Heritage has never archived this
  project**", which was false as written — it holds **9 visits of the ancestor `Niko1221/Strata`** (8 `full`, 1
  and it holds **our root commit**. The word "never" also exceeded the evidence: a point-in-time `404`
  licenses "as of this date", not "never". Corrected anchor tiers, in the next bullet.
- **Three anchor tiers, and they are not equally strong** (the audit's only genuine cross-authority
  corroboration is in the first one):
  | what is anchored | off-GitHub witness | status |
  |---|---|---|
  | **Strata's current `main`** | SWH snapshot 2026-11-03 lists `refs/heads/main → ffa80d73133e…`, equal to `git ls-remote` the same day | **two authorities, one hash** — the strongest provenance statement available here |
  | **our shared root `f2a08d4b`** | SWH revision `200`; recorded in its 2026-10-05 visit as **Strata's own `refs/tags/v0.1.0`** | anchored, under the ancestor's origin — see the rewrite below |
  | **our pin `dc047a77` and all post-fork basalt work** | none: absent from Strata's served objects, `404` at SWH, `422` at the API | **anchored nowhere outside GitHub.** Basalt has no off-GitHub witness at all |

  The tiers disagree exactly where it mattered: the five pre-2026-10-05 commits Strata's *object
  store* serves are `404` at SWH. **The inherited-looking half is the least externally witnessed part
  of this story.**
- The pin is the **tip of `main` as fetched that day** and is **untagged**. `v0.3.5` exists only in
  upstream's own `docs/` and `pyproject.toml`; there is **no upstream release artifact to diff the pin
  against**, which feeds Risk 5 (unpinned wheels).

**What the pin actually is:** PR #242, `serve: read NVML from nvml.dll on Windows`, author
`polygizer`, committer `GitHub`, authored **2026-11-05T00:48:24Z — the day we pinned it**. It changes
exactly one file, `serve/util/telemetry.py` (a Tier A file). We pinned a working `main` hours after it
landed. No `SECURITY.md`, `.github/SECURITY.md` or `docs/SECURITY.md` exists at this commit — only
`serve/security.py` and its test — so there is **no declared disclosure channel.**

**The pin is basalt's `v0.2.1` plus documentation and `serve/` only — the C++ engine is byte-identical
to that tag.** Our own `versions.env` claims *"the engine is identical to v0.2.1"*; measured at the pin,
that is true. `commits v0.2.1..dc047a77 = 3`, and the 9 changed files are `README.md`, three files under
`docs/`, and five under `serve/` (`api/responses.py`, `frontend.py`, `service.py`, two tests) —
**0 changed files under `src/`, `include/`, `cmake/`, `tools/` or `CMakeLists.txt`**. By blob identity
over those engine paths: `|v0.2.1 engine|=553`, `|pin engine|=553`, `|intersection|=553`, symmetric
difference **0**. Two consequences: **(i)** what Plan 01 established about the C++ engine at `v0.2.1`
still holds at this pin, so Plan 02's `src/platform/` reads start from a state Plan 01 partly
characterised — *except* `CMakeLists.txt` (287 commits), where Plan 01's read is stalest; **(ii)**
basalt's **own** annotated tag `v0.2.1` (`1b3f6964`, tree `515676c1`) is a usable, unambiguous anchor for
T2b's content-lineage join — unlike Strata's tags, whose names collide with these and whose targets have
already moved once.

### What "basalt ← Strata" actually means (T2, measured 2026-11-05)

`AGENTS.md:3`, `CONTRIBUTING.md:56`, `README.md:68` and `THIRD_PARTY_LICENSES:4,9` in the pinned tree
declare a fork of `Niko1221/Strata` (MIT). GitHub reports `fork=false`, which means **no fork-network
link**, not that the relationship is false. What the relationship is *not* is the surprise: measured
against a **pristine** `git fetch --filter=tree:0` of Strata's `main` (5,637 commits, `in-pack 0` → 5,637
verified, no alternates), **the two repositories share zero commit SHAs, have different root commits,
and have no merge-base.** Full evidence, controls and four refuted hypotheses:
[`../wave0/t2-lineage.txt`](../wave0/t2-lineage.txt).

- **They are two rewritten variants of one history.** Their root `01529aaa…` and our root `f2a08d4b…`
  are identical in tree (`4591e4ca…`), author, committer, **both** epochs and subject; ours additionally
  carries `Co-Authored-By: Claude Opus 5.5 (1M context)` and a `Claude-Session` link. Over whole
  histories, **844 commit pairs match on `(tree, author-date, committer-date, subject)`** with **0
  duplicate keys on either side**, partitioned on a **UTC instant** at 2026-10-05T00:00:00Z: **844 of our
  861 pre-fork commits have a
  twin (98.0 %) and 0 of our 1,333 after do.** That is the fork boundary, measured by content. (An earlier
  draft of this line said 882 / 1,312 at 95.7 %, which is the same history partitioned by the **author's
  local calendar date**; both bases are recorded in [`../wave0/t2-lineage.txt`](../wave0/t2-lineage.txt) § 4,
  which also lists the 17 unmatched pre-fork commits — owner work of 2026-10-03/04 that Strata's current
  `main` does not carry.)
- **The ancestor rewrote its published mainline, and the only measured delta is AI-attribution
  metadata.** Across those 844 byte-identical-content pairs: Claude trailers **1,049 ours / 0 theirs**,
  human co-authors **22 / 22**, Codex **3 / 3** — one tool's attribution removed, every other trailer
  preserved exactly. Whole-repo: **1,881 of our 2,194 commits (85.7 %)** carry an AI co-authorship
  trailer vs **3 of their 5,637**. Software Heritage's 2026-10-05 visit records **Strata's
  `refs/tags/v0.1.0` at our trailer-bearing root**, where today that tag is the trailer-free twin; and
  `jesdga95/basalt` was created `2026-10-06T22:17:02Z`, **after** that record, so the trailer-bearing
  line was the ancestor's published release before our fork's repository existed. **Motive is not
  established**, and four innocent explanations for basalt keeping the old line are listed in § 5 of the
  lineage file. Preserving both epochs while editing only the message is `git filter-repo`'s *default*
  behaviour.
- **Consequences for this audit, stated as method.** (1) **Never diff these two repositories by SHA** —
  `git log upstream/main..pin`, `git cherry`, cross-repo `bisect` and blame continuity all *run* and
  return a plausible wrong answer (the first would print our entire history as new work). `git diff A B`
  is still sound: it is tree-to-tree. Lineage here is the content key `(tree, author-date,
  committer-date, subject)`, blob SHAs for files, and patch-id for changes. (2) **"Inherited" can only
  ever mean byte-identical content**, never "same commit", never reviewed, never trusted; `census.md`
  dispositions stay lineage-independent and a lineage annotation may order reading but **never reduce
  depth**. (3) Basalt's `AGENTS.md` still instructs contributors to end commits with an AI co-author
  line, and **0 files** under `src/`, `include/` or `cmake/` mention Strata at all — lineage is
  invisible in the build and visible only in git metadata.
- **Not established:** which commit the fork came off (no merge-base exists), who ran the rewrite,
  whether one party controlled the ancestor's origin URL at both dates, motive, and whether any seam
  file's *bytes* are original — that last one needs T2b's blob join, because commit absence cannot
  prove code originality.

### Author census of the boundary (T2's second half)

Substrate [`../wave0/t2-author-census.tsv`](../wave0/t2-author-census.tsv) (87 rows: the 75 Tier A
rows + the 12 files the plan's T2 names that the manifest leaves unassigned), generator
[`../scripts/t2-census.sh`](../scripts/t2-census.sh), findings and read order
[`../wave0/t2-boundary-authorship.txt`](../wave0/t2-boundary-authorship.txt). Headline, all of it
`git blame` at the pin with **no** `-w`/`-M`/`-C`:

- 20,111 blamed lines over **80** first-party files (vendored `stb_image.h` excluded — blame credits
  importers, not authors), **53** distinct blamed authors; Jesus Gulfo 72.4 %, the two core authors
  81.0 %. Reconciles exactly: manifest LOC over those same 80 files is 20,108, and the +3 is 3 files
  with no trailing newline.
  CORRECTED 2026-11-05: this said **86** files, and the derivation is worth spelling out because three
  denominators were in circulation. The census TSV has **87 rows** and **no header line** (row 1 is
  `CMakeLists.txt`): 81 rows carry `census_status=OK` and 6 are `BINARY_SKIPPED` (the Tier A binary
  assets). First-party blameable text = **87 − 6 binary − 1 vendored `stb_image.h` = 80**, which is the
  denominator of every percentage here. Of those 80: **69 are `A-planned` and 11 are `unassigned`** —
  *not* 75 + 5, because 6 of the manifest's 75 Tier A paths are the binary assets. Completeness of the
  input does hold against the manifest as source of truth: all **75** Tier A paths are present in the
  census, `ONLY_IN_TIERA=0`, and the **12** census paths beyond Tier A are `src/vision/stb_image.h`
  (`B-vendored@T5`) plus the 11 the plan's T2 text names that the manifest leaves `unassigned` —
  `CMakeLists.txt` and the ten `serve/engine/` modules (`__init__`, `errors`, `flags`, `lifeline`,
  `mock`, `process`, `progress`, `slots`, `suspend`, `vision`). The tier arithmetic closes both ways:
  all 87 rows = **75 `A-planned@T<n>` + 1 `B-vendored@T5` + 11 `unassigned`**; of the 75 Tier A, 6 are
  binary assets (`A-planned@T10`) that blame cannot read, and the 1 `B-vendored` is `stb_image.h` — so
  the 80 blameable first-party rows = **69 `A-planned` + 11 `unassigned`**.
- **560 of those 20,111 lines (2.78 %) sit on a blame boundary — the same 560 that are credited to the
  repository's root commit** (`f2a08d4b`, 2026-09-24, Niko1221, 219 files / 50,165 insertions: the
  project arrived as one bulk commit under its ancestor's name), across 7 files
  (`chat_template.jinja` 184, `tools/tokenizer.py` 178, `direct_file.cpp` 72, `direct_file.hpp` 51,
  `CMakeLists.txt` 44, `memory.hpp` 17, `memory.cpp` 14). Equal totals *and* per-file-identical
  populations: blame cannot walk past a parentless commit, so root-attributed **is** the boundary
  population.
  CORRECTED 2026-11-05: this bullet claimed "**0** of those 20,111 lines sit on a blame boundary, so no
  share is an artifact of blame stopping early". False — an unanchored `grep boundary` over blame output
  matches source text, not just the porcelain header. And the framing was wrong too: 2.78 % is not an
  "irreducible error bar" but a **lineage** quantity — those lines are the ancestor project's bulk
  import, i.e. the 1,001-line `inherited` bucket, where Niko1221 holds 94.8 %. Command and per-file
  counts: [`../wave0/t2-provenance.txt`](../wave0/t2-provenance.txt) § 6.
- **The OS seam is the concentrated one.** C++/CUDA Tier A is 33.9 % non-core vs `serve/`'s 3.8 %.
  Per file, top blame author: `process.cpp` 216/321 · `file.cpp` 247/456 · `direct_file.cpp` 255/455
  — one outside contributor (`jeancmyk`), each majority arriving in **a single commit per file**;
  plus `file.hpp` 100/157 and `process.hpp` 72/96. `memory.cpp` is the maintainer's (470/865) with
  that contributor second at 199/865 — plurality, not majority. This matters because `src/platform/`
  is the project's *only* legal home for `fork`/`exec`/`dlopen`/`mmap`/`/proc` (their `AGENTS.md`;
  enforced by `tools/check_platform_boundary.sh`), so it is where such a call is least likely to look
  out of place. **Attribution orders the reading; it never ranks people.**
  Two qualifications this task earned: (i) the commits that created these files **exist in neither
  repository's published line** — basalt's remote serves them, `Niko1221/Strata`'s refuses each one
  (`upload-pack: not our ref`) and none has a content-key twin in Strata's 5,637 commits
  ([`../wave0/t2-lineage.txt`](../wave0/t2-lineage.txt) § 2, § 4) — so "upstream would have caught it"
  is unavailable as a defence for precisely these files, which is a **review-existence** fact, not a
  claim about the person, and is what actually carries their rank; (ii) `memory.cpp`'s seam work came
  in through **basalt PR #47**, so its review history is checkable — that check, not the blame
  plurality, is the load-bearing question, and it belongs to the read, not to T2.
- **Never-revised files at the seam:** `serve/engine/image_source.py` 570 l / 1 commit / 1 author
  (T4's one outbound channel) and `src/platform/dynlib.cpp` 64 l / 1 commit / 1 author (the `dlopen`
  boundary). `serve/security.py` 228 l / 5 commits / 2 authors.
- **A tiering gap this census found:** `serve/engine/process.py` (1,419 l, 6 blamed authors — the
  code that spawns the engine) and `CMakeLists.txt` (1,507 l, **287 commits**, 18 blamed authors, 34
  contributors at ≤2 commits) are both `unassigned`.

**`CMakeLists.txt` was deliberately not retiered here.** Tier A is seeded from the plan's own `File:`
fields so that a census can never move the goalposts, and 1,507 l also exceeds the 1,500-l slice cap.
Assigned forward: **amend plan `### T11`'s `File:` field first**, then retier from it — proposed
**T11** build-file split `T11a` (toolchain/options/gate) + `T11b` (targets/install). Plan 01 read
`CMakeLists.txt:40,94-117,528,533`; 287 commits have touched it since, so that reading is stale.
Same for `serve/engine/process.py`, which belongs in T4's or T6's scope, not in a census's.

### Definition limits this section adds

- **`f4` (LOC) is a newline count from the checkout**, not from the blob. `git status --porcelain` is
  clean and the two `.gitattributes` rules (`tests/data/native_experts/** -text`,
  `docs/community/benchmarks.json text eol=lf`) were spot-checked: blob LOC == worktree LOC.
  Three files have no final newline, so `blame` counts one line more than `f4` does — hence the
  `no_trailing_nl` column rather than a wrong `f4`.
- **The clone is no longer blob-empty.** Blame on a `blob:none` clone lazy-fetches: `count-objects`
  went to 19,352 in-pack across 752 packs. The tree hash still matches and the worktree is clean, but
  the T1 `GIT_NO_LAZY_FETCH=1` discipline applies to *freeze checks*, not to this census.
- **Plan § Effort says Tier A is "~34,000 lines"; the manifest says 19,659.** The manifest is the
  measurement, the plan's estimate predates it, and the 14–19 session figure was derived from the
  larger number. Recorded, not silently superseded — the plan's effort basis needs revisiting at T13.
- Tier A's 19,659 l is **54 code files / 13,615 l + 21 non-source files / 6,044 l** (a 2,409-line GIF,
  a 960-line `.prof`, a 922-line JSON dump, fonts, PNGs, token dumps, a `.jinja`, CSS/HTML, an
  `.exe.manifest`). This **does not reduce the owed-read count**; it says what "a line of Tier A" is.
  Re-tiering them is T11's, not the census's (`§ invariants`: no `A-planned` row becomes `triaged:*`).
