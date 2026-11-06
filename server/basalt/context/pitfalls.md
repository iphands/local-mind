# Pitfalls & Gotchas — basalt

Every bug, gotcha and trap — **especially anything that took more than one attempt to fix.**
Cross-cutting entries also go up to `../../../context/pitfalls.md`; project-specific ones stay here.

**Read the front sheet before new work; grep the rest by the token you are about to type.** Below this
page are the six rules everything else cites, then the shell-idiom list (the part you actually look up
mid-task), then one entry per incident. Nobody reads forty-odd traps and entries end to end before a
task; the front sheet is what makes the rest retrievable. Tag entries that actually bit
`[OBSERVED YYYY-MM-DD]`, or `[MEASURED …]` when a control produced the number; untagged entries are
hazards seeded from reading.

## The six rules <!-- hp:pitfalls-rules -->

Everything below cites these instead of re-deriving them. They are the generalisation of twenty-six
observed shell traps and twenty incidents, all from one week, all the same family: *the check that
reports green because the thing it checks was never reached, or was looking somewhere else.*

- **R1 — Print a number.** Never let absence of output, or a trailing `rc=$?`, carry a verdict when a
  count can be printed: `| wc -l | sed 's/^/PREFIX_DRIFT_COUNT=/'`, `awk 'END{print "ROWS=" NR+0}'`,
  and assert the expected value **inside the producer** (`[ "$rows" = "862" ] || exit 5`) so it refuses
  to emit a short artifact. Tie two different zeros together (`RECORDS=` from `awk END{NR}`, `NEWLINES=`
  from `wc -l`) so "a row went missing" cannot masquerade as "a newline went missing", and print the
  **expected value as a literal** with the actual derived from it, never the reverse.
- **R2 — Name the set you compared.** A comparison over two key sets is not a verdict until you print
  `SET_A`, `SET_B`, `INTER`, `DIFF`; a comparing command inherits the emptiness of what it compares;
  and per-row checks inherit the emptiness of their own extraction, one layer below.
- **R3 — Size stderr.** Never suppress or swallow a stream inside a check: collect it and assert
  `ERR_BYTES=$(wc -c < err.log)` is `0`. A probe must be able to answer UNKNOWN — `command -v <tool>`
  before anything consumes its answer, and never a reassuring default on a `||` branch.
- **R4 — Paste it or label it.** A line in a stamped artifact asserts *this ran, this came out*. No
  transcript from memory; verify a stated precondition ("no X yet") against the tree; when two records
  of one step disagree, re-run the step rather than adjudicate from recall.
- **R5 — Show it fail once.** A check never observed non-zero is decoration. Give every comparison a
  control input that must disagree, control the *checker* and not only the data, and when a control
  does not fire, check what it was aimed at.
- **R6 — Name the population behind the number.** The program by hash, the tag *and* the run state,
  the pathset, the column index, the date basis, the shell it was measured in — in the same breath as
  the number, and in the same place a threshold would be seeded from it.

## Index <!-- hp:pitfalls-index -->

| § | One line |
|---|---|
| *Shell idioms that report green on the thing they did not check* | numbered traps, `1`–`27` today, **next free: 28** (the bound is asserted by the `items=` field in § *Item numbering* → **Size**, not remembered); grep the idiom. **A new one takes the next free number — never renumber: these numbers are addresses, cited as bare `item N` from `RULES.md`, this project's plans, `context/distilled.md` and `context/audit/index.md`** |
| *Entry template* — the comment block closing this front sheet | the four labels (`Trap.` / `Guard.` / `Grep:` / `Src:`) and the one-line form for a shell idiom; kept as a comment so documenting the shape does not create a citable entry |
| *The "NInfer" QUASAR 27B looks like a Basalt model, and is not* | wrong artifact family, caught at `./run` |
| *The image boots the mock engine but not a real pack: `serve/` imports `tools/model_file.py`* | `tools/` missing from the image; gate step 6 owns it |
| *Empty output from a failed command was recorded as a measurement* | the founding incident: permission-denied and a missing `ss`, both read as "nothing there" |
| *A gate prerequisite recorded in RULES.md can silently stop being true* | toolchain moved, gate went red for a markdown commit |
| *A live daily-driver server is not a test bench* | the owner's running server, and what you may read from it |
| *A shared worktree has no such thing as "my uncommitted edits are private"* | two writers, one index |
| *A boot from the NFS copy crawls at 5–10 MB/s* | `O_DIRECT` × fragmented extents |
| *Cross-repo git forensics: three commands that answer a different question than they look like* | `A..B`, `cherry` and the commit API across unrelated histories |
| *Provenance that cannot contradict its own claim* | a line range as a version oracle; greps on invented identifiers |
| *An apostrophe in a comment inside a single-quoted `awk '…'` body is still inside the quotes* | the one that failed closed |
| *A control run's blast radius — the baseline its tag wipes, and the stray files its command block leaves* | `rm -rf` before validation |
| *An LC_ALL naming a locale the host does not have falls open to C — no warning, no error* | a tripwire that cannot fire |
| *A dated fact scoped to a shell I could not enter was declared "stale" — the scope caveat was on the adjacent line* | the reader defect, mirror of the instrument defect |
| *A commit is the index, not the file I was looking at* | staged, edited four times, never re-staged |
| *A number measured on mutated copies got recorded as a property of the tag they ran under* | `SENTINEL_TOTAL=28` attributed to a tag |
| *"You can reproduce this from the file alone" — a re-derivation claim that never names which cut of the program it starts from* | recipe precise, base vague |
| *A census keyed on a path set cannot see a modified file — and the gate that looks like a content check is not one* | the universe is paths, not bytes |
| *A hash retyped from scrollback came out as 64 valid hex characters that were not the hash* | four of ten wrong |
| *A checker's self-test re-ran the checker unguarded — a fork bomb, and the checker had never once run* | code after an unconditional `sys.exit` |
| *A census that drops wide-only hits cannot see a hard-coded URL* | wide hit, never promoted |

## Item numbering, and what is frozen <!-- hp:pitfalls-numbering -->

Item numbers in the shell-idioms § are **addresses**: `RULES.md`, this project's plans, `context/distilled.md`
and `context/audit/index.md` cite them as bare `item N`. They are not renumbered casually.

**Renumbered 2026-11-05.** The § held **26 items wearing 22 numbers**: the problem list ran 1–22 and a
second series of four bullets inside *How to avoid* re-used **18, 19, 20, 21** for four different
traps. Both series were cited from outside as bare "items", and `RULES.md` and the Plan 02 tracker
cited both. The four bullets became **23–26**; nothing else moved. Mapping for anything written before
this date: old *How to avoid* `18→23`, `19→24`, `20→25`, `21→26`.

- Frozen (`context/audit/wave0/*` are stamped transcripts and are never edited backwards) cite **1, 15, 16, 17,
  18** only — all problem-list items, all still correct. `wave0/t1-freeze-and-manifest.txt` cites this §
  as "Four shell idioms…", which was already stale when it was stamped: that line is the reconciliation.
- Live citations re-pointed in the same commit: `context/plans/…_tracker.md` `(items 20–21)` → `items 25–26`
  (tree contamination), `item 19` → `items 20–21` (it described the defaults and denominator traps, not
  the classifier one — an off-by-one that predates this change), and `items 18–19` → `item 23` (the
  cache short-circuit, which is what the parenthetical is about).
- The count stays out of the § heading on purpose: it grows, and every heading-based citation then
  goes stale. Prefer the identifier you already have over a string you must maintain (item 18).

**Size, decided 2026-11-05.** This file **stays single** — the owner's call, taken against
`context/AGENTS.md` § *House Style*, which read *"a file past ~40 KB is split"* and has been **amended to
~120 KB** the same day to say what is actually meant. The warrant, so the amendment can be re-examined
rather than re-litigated: the rewrite cut **118,140 B → 92,306 B** — both re-derivable,
`git cat-file -s 52b1a13^:context/pitfalls.md` and `git cat-file -s 52b1a13:context/pitfalls.md` — while
keeping **99 % of the payload** — the backticked commands, the falsely-reported strings as printed, the
evidence numbers: 19,514 of 19,686
bytes. What remained was prose at **3.87 : 1** against payload, and this §'s own tightest items (item 2,
item 21) sit at **4.4 : 1**. The 45 KB target implied **1.38 : 1** — every one of the corpus's 46 units
tighter than the tightest unit that exists, guards and symptom strings intact. That is deleting the
*why-it-looked-right* half, which is the only half that transfers to a reader who wasn't there.
`context/AGENTS.md` also says *never compress so far that a detail is lost*, and that outranks a byte
count. **The number that matters is the front sheet**: read that before work, grep the rest by the
token you are about to type. *(Corrected in place 2026-11-05, one commit after it was written. It first
read "the front sheet — § *The six rules* and § *Index*, ~9 KB … it is 9,396 B / 113 lines", which
mis-attributed the whole front block to two of its three sections; the count then moved with every edit
to the note holding it, including the edit that recorded this correction, and one draft was caught wrong
only by re-running the measurement instead of re-reading the sentence. This note sits inside the block it
measures, so it now quotes **no size for itself** and gives one command instead. It partitions the file
at the two § boundaries, and the sum must equal `wc -c` — so the parts cannot drift from the whole,
which is § *The six rules* → **R2** applied to this file's own arithmetic:)*

`LC_ALL=C awk 'BEGIN{p=1} /^## Shell idioms/{p=2} /^## The "NInfer"/{p=3} p==2&&/^[0-9]+\./{n++; split($1,a,"."); if(a[1]+0>mx)mx=a[1]+0} {s[p]+=length($0)+1} END{if(s[1]==0||s[2]==0||s[3]==0){print "BOUNDARY MISSED - both /^## / patterns must fire"; exit 9} if(n!=mx){printf "NUMBERING NOT DENSE - items=%d max=%d\n", n, mx; exit 8} printf "front=%d shell=%d narratives=%d items=%d sum=%d\n", s[1],s[2],s[3],n,s[1]+s[2]+s[3]}' context/pitfalls.md`

Three arms, three observed exit codes, because **closure proves conservation, not partition identity.**
`sum == wc -c` says the three buckets tile the file; it says nothing about whether both boundaries *fired*
— and the second one is made of prose, since § *Item numbering, and what is frozen* freezes item
**numbers**, never entry titles. Retitle the first narrative `##` and that boundary slides to end-of-file:
`narratives=0`, `shell` swallows the corpus, and the published assertion stays green. Observed on a copy
with exactly that retitling: unguarded it prints `sum=97849` against `wc -c` **97849** — conservation
perfect, partition a lie — while this command prints `BOUNDARY MISSED` and exits **9**. The other arms,
each run and not assumed: either boundary *pattern* broken → `BOUNDARY MISSED`, **9**; a numbered item
deleted from the middle → `NUMBERING NOT DENSE - items=26 max=27`, **8**; wrong cwd → `awk`'s own fatal,
**2**, rather than a plausible zero. Healthy: **0**, `items=27`, `sum` equal to `wc -c`.

That last field is the one a future contributor depends on: **`items=` is the upper bound the Index row
advertises**, asserted here rather than remembered. Add item 28 and this command prints `items=28` against
an Index row still saying `1`–`27`, so the row goes visibly stale the moment it is stale, and nobody has to
recall a rule. If the two ever disagree, the Index row is what is wrong.

The other is **item 27** above, this file's own: `awk`'s `length()` counts **characters**, so an unpinned
run under-reads. Run it with and without `LC_ALL=C`; the gap is `gap = bytes − chars`, which is the number
of non-ASCII **continuation bytes** — one per byte past the first in each multi-byte character, so a 3-byte
character contributes two and a 4-byte one contributes three. It is *not* the number of multi-byte
characters, and reading it as that is the trap. One paired run on the front block here, 2026-11-05, under
`LANG=C.UTF-8` with `LC_CTYPE` unset and `AWK=GNU Awk 5.4.1` at `/usr/sbin/awk`, gave a gap of **121**
against **70** such characters: 51 three-byte (`—` `–` `→` `…`) carrying two each plus 19 two-byte
(`§` `·` `×`) carrying one, which is where the extra 51 comes from. All five of those numbers have moved
since that sentence was written; that the gap **exceeds** the character count has not, and the relation is
the trap, not the reading. Two limits on reproducing it, and § *The six rules* → **R6** is the reason they
are written down rather than implied: `locale -a` here lists only `C`, `C.utf8` and `POSIX`, so do not reach
for `en_US.UTF-8` to raise the other side — § *An LC_ALL naming a locale the host does not have falls open
to C* is exactly that trap, and it hands back a byte-correct-looking green. And on `mawk` or busybox `awk`
the gap reads **0** whatever the locale, because those count bytes always; there the relation is
**unobservable, not contradicted**. Record `AWK=` beside `LANG=` when you run it, or the reading you get
means nothing about which of the three cases you are in.

Re-open the split question if `wc -c` passes ~120 KB, or if `front` in that output exceeds ~15 % of `sum`
**and** the front sheet has stopped being a sheet. The `and` is deliberate and weaker than an `or`: the
ratio on its own is satisfied merely by the tail growing, and **stays single** above is an owner's call, so
that guard is meant to need two independent failures before it fires.

<!-- Entry template (new shape — keep the three labels, quote the symptom verbatim):

## <Pitfall title>
**Trap.** [OBSERVED YYYY-MM-DD] What happened, the exact error/output, what was tried first.
**Guard.** The rule, the command that enforces it, how to detect it early next time. Cite R1–R6.
**Src:** `<file>` § `<section>` — the artifacts that hold the raw output.

For an idiom in the list above: one line, `**N · idiom** → what it falsely reported → guard · R# · src`.
-->

---

## Shell idioms that report green on the thing they did not check

[OBSERVED 2026-11-05, Plan 02 T1–T3 — plus item 27, a docs edit] Twenty-seven commands that came back
*passing* while the measurement had not happened, plus one transcript line that recorded output no command
ever produced (item 17 — the mirror image: not silence but invention). Each item carries its own tag; in
aggregate: **1–7** are untagged hazards from T1's freeze, manifest and grid controls, **8–18**
`[OBSERVED 2026-11-05, T2]` (item 14 `twice in one session`), **19–21** `[MEASURED 2026-11-05, T3]`,
**22** `[OBSERVED 2026-11-05, T3]`, **23–26** `[OBSERVED 2026-11-05]` — T3's security harness and
tree-contamination incidents, the four renumbered bullets — and **27**
`[OBSERVED 2026-11-05, docs edit — no plan task]`, the one item added after the rebuild, the only one that
came from *editing this file*, and the only one with no `wave*/` artifact behind it. Items **25–26** are
the two a passing audit rested on while wrong: a freeze check blind to an ignored directory, and a file
listing that contained itself.

**Grep this § by the literal you are about to type.** Field order is fixed: the idiom, what it
falsely reported *as printed*, the guard.

1. **`IFS= read -r h p` over `git … --format='%(objectname) %(path)'`.** `IFS=` disables whitespace
   splitting, so the whole line lands in `$h` and `$p` is empty → `fatal: could not open '' for
   reading` ×862 and `MISMATCH_COUNT=862`, while an earlier loop written as plain `read -r h p` had
   already put the *correct* answer (0) on screen → split on a tab (`--format='%(objectname)\t%(path)'`
   with `IFS=$'\t'`), never on a default-IFS space and never with `IFS=`. · R1 · `wave0/t1-freeze-and-manifest.txt`
2. **A dot-prefixed file invisible to a glob.** The manifest split wrote `.github.tsv`; `awk 'END{print
   NR+0}' manifest/*.tsv` said **859** instead of 862 and `wc -l manifest/*.tsv` never lists it → count
   through `find . -maxdepth 1 -name '*.tsv' -type f | wc -l` (dot-aware, and `-type f` rejects a
   symlinked substitute), and name artifacts so no glob that counts them can skip them. · R1 R5 · `context/audit/manifest/index.md`
3. **`diff -r a b` over two empty directories → "DETERMINISTIC".** The generator had run from the wrong
   cwd, printed `fatal: not a git repository` and produced nothing; the comparison of two empty outputs
   succeeded → assert the inputs are non-empty before comparing anything that compares. · R1 R2 R5
4. **`sha256sum -c` on a sidecar of `cut -f1-4` hashes.** `-c` re-hashes **whole files**, so all 12 TSVs
   reported `FAILED`: the design was sound, the verifier was not, and nothing said so → write the
   recompute loop (`cut -f1-4 | sha256sum` against the recorded value) and keep it away from `-c`. · R5
5. **An assertion that prints `FAIL` and still exits 0.** The verdict was a *string* computed inside
   awk, so a typo'd tier printed `ACTUAL=861 FAIL` and the script went on to print
   `ALL_SENTINELS_PASS=1` → a verdict computed in a sublanguage cannot set the exit status: bring the
   value back (`part=$(awk '…END{print A+B+U+0}')`) through the same `check` helper that sets the flag. · R1 · `context/audit/scripts/ledger-grid.sh`, `wave0/t1-grid-controls.txt`
6. **A guard written to catch a bug it cannot see.** `M=(*.tsv)` **never matches a dot-prefixed file**,
   so `check TSV_FILES 12 "${#M[@]}"` could only ever catch a 13th *visible* TSV while its comment
   claimed to cover item 2, which had already happened → cross-prove any glob against `find`. · R5
7. **`uniq -c | awk 'END{print sum}'` deletes the histogram.** The sentinel survives and the evidence
   the command existed to show is gone → `awk '{print; s+=$1} END{print "SUM=" s+0 " (expect 862)"}'`. · R1
8. **`2>/dev/null` turns a bad flag into an empty column that still prints.** The census asked breadth as
   `git rev-list --count --follow <pin> -- <path>`; **`rev-list` has no `--follow`**, so the usage block
   went to the suppressed stderr and the `commits` column came back **empty for all 75 rows** while every
   row printed and the census declared itself validated; same mechanism made a `git blame` probe print
   `0` for a path that does not exist → never suppress stderr inside a check; size it
   (`ERR_BYTES=$(wc -c < err.log)` asserted `=0` in `VALIDATION`). Correct form: `git log --follow --format=%H <pin> -- <path> | wc -l`. · R3 · [OBSERVED 2026-11-05, T2] · `context/audit/scripts/t2-census.sh`, `wave0/t2-provenance.txt` § 4
9. **A comparison over two key sets that never overlap reports a perfect score.** One swapped awk field
   made the *local* keys blob SHAs (`git rev-list`, same shape from `git ls-tree -r`), so the intersection
   was empty: `ONLY_IN_API=862
   ONLY_IN_LOCAL=861 BLOB_SHA_MISMATCH=0` — a zero over nothing, one step from "862/862 verified".
   Every `comm` / `join` / `diff <(sort a) <(sort b)` / set-minus has this hole, including
   `context/audit/index.md` check 6 (`comm -3 … | wc -l`, expect 0 — two empty inputs sail through it) → print
   and assert all four cardinalities (`SET_A=$(grep -c . a)`, `SET_B=$(grep -c . b)`,
   `INTER=$(comm -12 a b | grep -c .)`, `DIFF=$(comm -3 a b | grep -c .)` = `862 862 862 0`), then make
   the comparator prove it can fail: the identical comparison against the pin's **first parent** returned
   `BLOB_SHA_MISMATCH=1`, isolating `serve/util/telemetry.py` out of 862. **Same species:** an
   end-anchored pattern against a headerless TSV when the field you mean is not the last —
   `cut -f1,2 manifest/*.tsv | grep -vE '\.(py|cpp|hpp)$'` matches *nothing* because every line ends in
   the LOC digits, and a "which Tier A files are not source" classification came back listing all 75 →
   index the field (`awk -F'\t' '$1 ~ /\.[ch](pp)?$/'`); on a headerless line you cannot eyeball which
   column is last. · R2 R5 · [OBSERVED 2026-11-05, T2] · `context/audit/scripts/t2-census.sh`, `wave0/t2-provenance.txt` § 4 (controls C1–C5)
10. **A uniform negative across an entire endpoint family is a broken probe, not a finding.** Every
    `GET …/api/1/revision/<sha>/get/` returned `404` — nine SHAs, including one the archive
    demonstrably holds — because Software Heritage's revision route is `/api/1/revision/<sha>/` **with
    no `/get/` suffix**. "Software Heritage has never archived this project" was drafted off the broken
    run; with the correct route and controls (their tip → `200`, zeros → `404`) it inverted: the archive
    holds the ancestor repo in 9 visits (8 `full`, 1 `failed`) *and our own root commit* → put a
    known-positive beside any new endpoint before believing a single negative, and treat an
    all-negative sweep as more suspicious than a mixed one. The same signature appeared twice more as a
    uniform zero: 367 blamed commits none of which "carried an AI trailer" (item 12), and "0 blame
    boundary lines" (item 13). · R5 · [OBSERVED 2026-11-05, T2] · `wave0/t2-lineage.txt` § 0
11. **Abbreviated SHAs — three incidents in one session.** Local `git` resolves an 8-hex prefix, so
    probes look right while sending a different request shape: seven "is this commit upstream?" probes
    went out as 8-hex and their `422`s were partly request-form artefacts (the control that *did*
    discriminate was a full-40-hex nonsense SHA); a **partial clone refuses abbreviated OIDs outright**
    (`Not a valid object name 09ae16000f84…`), which is indistinguishable from a legitimate negative;
    and a hand-typed 34-hex root SHA used in a `startswith` comparison briefly "proved" our root was a
    snapshot branch-tip → assert length before use (`[ ${#sha} -eq 40 ]`), and generate probe inputs
    from `--format='%H'`, never from anything a human or `--short` touched. · R2 R6 · [OBSERVED 2026-11-05, T2] · `wave0/t2-lineage.txt`
12. **`git log --format='%H%x00%B'` piped through `split("\0")` yields `BODY` + `NEXT_SHA` chunks, not
    records.** The NUL was the *field* separator, so each SHA glued to the previous commit's body; the
    "SHA set" that came out contained commit subjects and its intersection with the 367 blamed commits
    was **0** — nearly recorded as "no AI trailers in the audited lines" → `git log -z
    --format='%H\t%B'` (NUL *between* records, tab inside them), or `%H\x1f%ae`. When you pick both the
    record and the field separator, name which is which, and validate the parse (`malformed_headers=0`,
    sampled values look like 40 hex) before the set feeds a comparison. · R2 · [OBSERVED 2026-11-05, T2] · `wave0/t2-lineage.txt` § 0
13. **Grep for a `git` porcelain *header* must be anchored — porcelain echoes your source lines.**
    Counting blame boundaries with `git blame --line-porcelain | grep -c boundary` also matches the word
    inside the blamed code (`… a page boundary` in `memory.cpp`, `boundary` in `CMakeLists.txt`); the
    unanchored figure published as **0** against an anchored truth of **560 boundary lines across 7
    files** — the same population as the 560 lines blamed on the root commit, because blame cannot walk
    past a parentless commit → `grep -cE '^boundary( [0-9a-f]{40})?$'`. Same hazard for `parent`,
    `author`, `summary`, `previous`: the format prints arbitrary user text between headers. · R2 · [OBSERVED 2026-11-05, T2] · `wave0/t2-lineage.txt` § 0
14. **`tail -n +2` on a headerless TSV deletes a data row — and this one was in the *check* of the row
    count.** `context/audit/wave0/t2-author-census.tsv` is headerless by manifest convention, so
    `tail -n +2` discarded `CMakeLists.txt`, the largest row in it, and every figure derived through it
    was short one file: a "80 OK + 6 skipped = 86 ≠ 87" *arithmetic anomaly* that took three commands to
    explain, and a derivation sentence ("87 − 1 vendored = 86") that nearly shipped as the documented
    denominator. The measurement survived only because it had used a field filter (`$13=="OK"`) → **know
    whether an artifact has a header before skipping a line, and prefer an explicit predicate over a
    positional skip**; if a header really exists, `head -1` it into the same transcript so the reader
    sees what was skipped. · R1 R6 · [OBSERVED 2026-11-05, T2, twice in one session] · `wave0/t2-author-census.tsv`
15. **A pathspec-scoped `git grep` cannot support a "does not exist anywhere" claim.** To decide whether
    `data/profiles/qwen4exp-48x512.prof` had a generator I ran
    `git grep -l -I -E "\.prof…|open\(.*prof|\.save\(.*prof" <pin> -- '*.py' '*.sh'`, got empty, and
    wrote "no writer/generator for `.prof` exists anywhere in the repo at this commit". **False** — the
    writer is C++ (`basalt::core::write_expert_profile`), documented in the very `docs/model-file.md` I
    had cited to argue the opposite. The empty result measured my pathspec, not the repository → **state
    the pathset in the claim, or drop it**; for a negative existence claim search the whole tree with no
    pathspec (`git grep -I <pat> <pin>`, then subtract what you excluded), or downgrade to "no generator
    in `tools/`, `tests/`, `.github/`"; and
    search for the *thing* (the writer's symbol name), not a guess at its spelling.
    - **Same trap as a number.** A path in `git diff --numstat -- <paths>` that matches nothing
      contributes **nothing and says nothing**, so a hand-assembled path list silently under-counts: my
      § 7 diff-shape figure was **18 lines short** because one path omitted an `context/audit/` segment. Any sum
      over an explicit path list must be reconciled against the same sum with **no** pathspec
      (`954 + 1861 + 349 = 3164` against `git show --numstat`) — a partition check, not a second opinion.
      And when you fix such an under-count, sweep for the *other* numerals it implied: the fix corrected
      the number it had looked at and left a stale one standing three lines below, because a value-search
      for a wrong number cannot find a number you do not know about.
    - **Ownership of those two files:** the ledger is `context/audit/coverage_ledger.md` (the mispath
      above dropped the `context/audit/` segment) and its tier roll-ups are **derived** —
      `context/audit/manifest/index.md` **owns the grid** and is what to cite for a tier count. · R2 R6 · [OBSERVED 2026-11-05, T2] · `wave0/t2-boundary-authorship.txt`
16. **A date partition is not defined until you name its basis.** `git log --format='%ad'
    --date=iso-strict | awk '$1 < "2026-10-05"'` looks like a UTC cut; the field carries the **author's
    own UTC offset**, so it buckets by local wall-clock day. The pre/post-fork split published as
    **882 / 1,312** where `%at` epochs at `2026-10-05T00:00:00Z` give **861 / 1,333** — 21 commits
    authored Oct 4 local are Oct 5 in UTC — and the two figures disagreed across three documents before
    it was caught → partition on **`%at` / `%ct`** (or parse to a tz-aware datetime), never on a
    formatted date string, and **print the cutoff as an instant with its offset**. Corollary:
    `--date=short` written *inside* a `--format=` string is literal text, not a flag — it silently does
    nothing and the field format changes under you. · R6 · [OBSERVED 2026-11-05, T2] · `wave0/t2-provenance.txt` §§ 6, 9
17. **A transcript line in a stamped artifact is a claim that a command ran — paste it or don't write
    it.** `context/audit/wave0/t2-gate-and-checks.txt` recorded gate step 1c as `SKIP(1c: no versions.env yet)`
    annotated "← SKIP, not pass. Pre-existing state, unchanged by T2." **No run ever printed that
    line:** `versions.env` has been tracked since `1eafda7`, so the `else` branch that emits that string
    is unreachable here, and T1's own stamped gate block records `env ok` for the same step. The sentence
    was a *prediction* of output, typed into a transcript — and the tracker note written to explain it
    then asserted an unobserved *before* state ("it changed state under this task, and it is a pass, not
    a regression") to reconcile the fabrication with T1's genuine observation. Cost had it shipped:
    `RULES.md` § *Project gate* makes "**skip is not pass**" load-bearing for the genuinely owner-only
    steps 3–6, so spending that marker on a line that never ran trains the next reader to discount the
    real SKIPs → every `text` block in a `wave*/` artifact is pasted bytes or is labelled (`expected` /
    `template` / `not run`); check a stated precondition against the tree before writing it; and when
    two records of one step disagree, **re-run the step** — recall is what manufactured the invented
    "before" state. · R4 · [OBSERVED 2026-11-05, T2] · `wave0/t2-gate-and-checks.txt`
18. **A locator you have to maintain is not a locator.** To name the newest row of a scope-evidence
    table whose own SHA did not exist yet, I built a `git log -S'<phrase unique to that row's note>'`
    pickaxe anchor and then wrote a paragraph declaring the anchor string load-bearing. One commit later
    the mechanism was false, twice over: the anchor resolved to one commit when typed and to **two** one
    commit later — nothing was renamed, but a later commit deleted an unrelated command that happened to
    quote the phrase, and `git log -S` fires on any change in **occurrence count** — and a renamed §
    left its old phrase resolving to a 2-commit set (`6dbb312` in, `d388d17` out). Worse, the paragraph
    documenting the hazard **quoted the phrase it was counting**, so reading and re-copying the file kept
    degrading the instrument → anchors rot through ordinary prose edits, not only renames: use the
    identifier you already have as soon as it exists, **never quote verbatim the literal whose count or
    match-set a check reports**, and prefer a convention to a mechanism (*the commit that adds a table
    row fills in the SHA of the row above*, so exactly one row is ever nameless).
    **Governance corollary:** append-only history protects **claims about the world, not scaffolding
    about the document** — a false assertion (`1,843`, the invented `SKIP`, the flat SHA bans) can only
    be answered by a correction commit, but an **instrument** shown unsound is *deleted*, not annotated
    in perpetuity; mistaking the second for the first is how an artifact accretes defensive prose until
    it buries its payload. · R5 · [OBSERVED 2026-11-05, T2] · `wave0/t2-gate-and-checks.txt` § 8
19. **A pattern anchored to the token after a delimiter silently misses order inversions — and an
    under-count that flatters your conclusion is invisible downstream.** `t2-provenance.txt` § 10
    counted AI co-authorship trailers with `co-authored-by: (claude|openai|chatgpt|copilot)` and
    published **1,881 of 2,194 (85.7 %)**. The figure reproduces exactly — nothing was miscalculated —
    but the alternation matches the token **immediately after the colon**: `Co-Authored-By: OpenAI Codex
    <codex@openai.com>` matches, `Co-Authored-By: Codex <noreply@openai.com>` does not. Anchoring on the
    **domain** gives **1,907 (86.9 %)** and **19,682 of 20,111 lines (97.9 %)**, and exposes five tool
    families the pattern could never see (DeepSeek, opencode, Antigravity, a Qwen-named co-author, a
    'Hermes orchestrator') → whenever a classifier tests `key: (value|…)`, it is testing *which word
    comes first*: write the pattern against the field that identifies the thing (domain, path, type), run
    at least one order-inverted sample through it before publishing a count, and **verify the subset
    relation** rather than just the new number (`|old − new| = 0` is what proved the wider net was a
    strict superset). This error *strengthened* the audit's own headline, which is exactly why it
    survived 11 commits and a sign-off: a count that argues against you gets re-derived, a count that
    argues for you gets cited. When a number agrees with your conclusion, re-derive the classifier. · R5 R6 · [MEASURED 2026-11-05, T3] · `wave0/t2-provenance.txt` § 10
20. **A default value in an aggregation silently manufactures evidence — an absent field scored as `0`
    reads exactly like a measured zero.** Aggregating GitHub's `/pulls?state=all`, a check written as
    `(p.get("comments") or 0) > 0` printed `with issue comments>0: 0/15` beside an empty `merged_by` map.
    Both looked like findings; the **list** endpoint does not return `comments`, `review_comments` or
    `merged_by` at all (`key in obj` → `present=False` for all three), so `or 0` turned three *absent*
    fields into three confident zeros → (a) print `key in obj` for every field an aggregate reads, once,
    before believing any count built from it; (b) prefer the **dedicated endpoint** over a list projection
    (`/pulls/N/reviews` is authoritative, the list is not); (c) write "the endpoint does not report X",
    never "X is 0" — the first is a fact about an API, the second a claim about the world. Corollary for
    **negative controls**: a control that "fires" on an absent field proves nothing, because the default
    produced the expected answer with no observation taking place. · R1 R6 · [MEASURED 2026-11-05, T3]
21. **Take a denominator from the producer's schema, not from a guess about which column means what.**
    To re-derive a published blame figure over "the 80 census files", the filter was written against
    `t2-author-census.tsv` **field 8** — which holds a top-author *name* (`Jesus Gulfo`); the status
    marker `BINARY_SKIPPED` lives in **field 13**. The filter returned **86** files, and 86 is close
    enough to 80 that the percentages would have been quoted against § 10's 80 as the same population →
    **print the column layout before writing the filter** (`head -1 | awk '{for(i=1;i<=NF;i++) print i,
    $i}'`) plus one known-special row; a probe loop that only examines fields 2–10 reports "nothing holds
    this marker" for a marker in field 13, which is what happened. And: **an error caught by unrelated
    luck is still an error** — this surfaced only because `git blame --line-porcelain` then hit a
    non-UTF-8 byte and raised `UnicodeDecodeError`; had that file been text the 86-file denominator went
    into a table silently. Never treat a lucky crash as a passing check. · R6 · [MEASURED 2026-11-05, T3]
22. **A row label is not a unique key — `grep -m1` and `head -1` turn a cross-table collision into a
    clean-looking answer.** A fence re-checking a table of ten rebuilt program hashes took its *expected*
    value from the same markdown file it was checking, with `grep -m1 "^| $c | "`. **Three** tables in
    that file begin rows with `| C1 | ` — a panel, a parentage table, the re-fire table — so `-m1`
    returned the panel, which carries no hashes: every expectation came back empty, all ten rows printed
    a confident `NO EXPECTED HASH`, and the clean run and the deliberately-corrupted run were
    **byte-identical**. A check that cannot fail; the hit was non-empty and plausible, which is why it
    survived — an empty one would have been caught → (a) anchor on the content of the column you want
    (require the label **followed by a backticked 64-hex**, so a table without that column cannot match),
    (b) extract with a **capture group** so column position is stated not assumed — each row carries a
    *second* 64-hex blob, so a column reorder silently substitutes a different quantity, (c) **never let
    `-m1` or `head -1` stand between a multi-match and a comparison**; assert the selected row count
    instead, and mind which assertion does that: `grep -cxE '[0-9a-f]{64}'` must equal 1, whereas
    `grep -qxE` **cannot detect a duplicate** because it passes when *any* line matches. · R2 R5 · [OBSERVED 2026-11-05, T3] · `wave0/t3-negctl-rechecks.txt` § 3b (the fix and its five-case battery; guarded by `context/audit/scripts/t3-paste-check.py`)
23. **A lookup with an existence-gated early return can silently disable the probes that come after
    it.** *(numbered 18 here until 2026-11-05)* In `t3-model-file-poc.sh` a probe that plants
    `~/.claude/settings.json` printed `wrote` while the file did not exist: two probes whose `meta` bytes
    are identical resolve to **one** `cache_dir` (the key is the meta digest *as declared in the file* —
    finding F-2), so the second `extract_tokenizer()` takes `if (out / "vocab.json").exists(): return
    out`, writes **nothing**, raises **nothing**, and the caller scores a pass. The finding under test was
    switching off its own probes → the fix is structural, not careful: give every probe a unique cache
    key and promote the collision to a named assertion
    (`F2_SAME_META_SHORTCIRCUITS_EXTRACTION=short-circuited`) so the mechanism is *proven live*.
    Generalise: whenever the code under test caches, assume later probes are no-ops until an assertion
    says otherwise. · R5 · [OBSERVED 2026-11-05] · `context/audit/scripts/t3-model-file-poc.sh`
24. **A harness that reassigns `HOME` changes what `git` reports, not just where files land.** *(18→24)*
    The same harness overwrites `HOME` so absolute-path writes are sandboxed — correct, and the point of
    the guard. But `core.excludesFile` is `~/.gitignore`, so under the sandbox `HOME` git cannot see the
    operator's global ignore file and `git check-ignore` answered **untracked** for
    `.claude/settings.json`, a path genuinely **IGNORED** (`~/.gitignore:16 /.claude`) in the real
    environment. One command, two answers, because the *environment* moved and only `$HOME` was thought
    about — and that column is the tamper-visibility evidence a 🔴 rating leans on → any probe that shells
    out to a tool with per-user config (`git`, `gh`, `ssh`, package managers) must **pin the config it
    means to measure**: `HOME="$REAL_HOME"` on each git call, plus `--no-optional-locks` so a read-only
    query cannot refresh the index of somebody else's repo. · R6 · [OBSERVED 2026-11-05] · `context/audit/scripts/t3-model-file-poc.sh`
25. **`git diff --quiet <pin>` proves tracked contents, not an untouched tree — and inside a gitignored
    directory it proves almost nothing.** *(20→25)* `server/basalt/.gitignore` line 1 is `/vendor/`, so
    every file created inside `vendor/basalt` is untracked *and* ignored, and `git diff <pin>` — tracked
    contents against a commit — cannot see one. This audit's own PoC harness ran with `cwd=vendor/basalt`
    and imported `serve.*` and `tools.model_file`, so CPython wrote **13 `__pycache__` files into the
    tree under audit** while a string of `AUDIT TREE OK` lines was true and useless. It surfaced only as
    `t3-census.sh`'s universe hash: **875 files against a pinned 862** (and LOC +999, because a LOC pass
    counts newline bytes and `.pyc` files are full of them) → a freeze check is **three legs**: `git
    diff` for contents, a **file count** against the frozen number, and `git status --porcelain
    --ignored` being **empty** — only the last names the offending paths, which a count cannot. Any tool
    that imports its subject sets `PYTHONDONTWRITEBYTECODE=1` *and* asserts afterward that it left
    nothing: the harness exits **3** on `POC_TREE_BYTECODE>0`, because "the experiment modified its
    subject" is a different failure from "an assertion failed" and earns its own exit code. What did
    **not** save us: 32 named assertions all passed on the contaminated tree. · R2 R5 · [OBSERVED 2026-11-05] · `wave0/t3-tree-contamination.txt`
26. **`ls` lists its own redirect target.** *(21→26)* `ls "$FIX" | sed … > "$FIX/fixlist.tmp"` creates
    `fixlist.tmp` before `ls` runs, so the listing contains the file being written: in `t3-census.sh`
    that inflated `FIXTURE_FILES` by one and handed `xargs` a path the following `mv` had just deleted.
    The worse sibling is writing the list *inside the corpus it describes* — on the next run the list is
    a corpus member, and here its absolute paths were greppable by the very family patterns being scored
    against it → `find … ! -name 'fixlist*'`, and keep derived lists outside what they enumerate. · R2 ·
    [OBSERVED 2026-11-05] · `context/audit/scripts/t3-census.sh`

27. **`+=length($0)+1` in `awk` counts *characters*, not bytes, in a UTF-8 locale — the total disagrees
    with `wc -c`, downward and silently; pin `LC_ALL=C`.** A byte-size assertion summed from `length($0)+1`
    read `10066` for a file's own front block while the same program under `LC_ALL=C`, and
    `head -n 121 | wc -c`, both read `10179` at that instant — 113 bytes short. Nothing downstream noticed:
    the number merely looked like a slightly smaller size, and nothing in the pipeline fails on a size that
    is only wrong. Both sides of that pair shared one boundary expression (the same `awk` program, one
    `LC_ALL` apart), which is the only reason the 113 is pure encoding — **use the same boundary on both
    sides**, because a mismatched one donates its own bytes to the "gap" and no arithmetic in the report
    shows it: `sed -n '1,/^## Shell idioms/p'` *keeps* the 66-byte heading that
    `/^## Shell idioms/{exit}` drops → pin `LC_ALL=C` (or use `wc -c`), print the unit beside the number,
    and record `AWK=` with it: `mawk` and busybox `awk` count bytes whatever the locale is, so on those the
    gap reads `0` and this trap is **unobservable** there, not absent. · R1 R3 ·
    [OBSERVED 2026-11-05, docs edit — no plan task] · `context/pitfalls.md` § *Item numbering, and what is
    frozen* → **Size** — no `wave*/` artifact: that pair was transient, and the live one comes from the
    guarded command in that §

## Sources

- basalt: `context/audit/manifest/index.md` (the checks, as corrected) and
  `context/audit/wave0/t1-freeze-and-manifest.txt` (the control output)
- basalt: `context/audit/scripts/ledger-grid.sh` + `context/audit/wave0/t1-grid-controls.txt`
  (items 5–7: the generator and the nine controls that caught them — two of nine exposed a bug in the
  script itself; 2026-11-05)
- basalt: `context/audit/scripts/t2-census.sh` + `context/audit/wave0/t2-provenance.txt` § 4
  (items 8–9: the `rev-list --follow` empty column and the disjoint-set zero, plus the five controls
  that now cover them — C1 parent-tree, C2 empty input, C3 sentinel, C4 empty field, C5 row count)
- basalt: `context/audit/wave0/t2-provenance.txt` §§ 6, 9, 10 +
  `context/audit/wave0/t2-lineage.txt` § 0 (items 10–13: the `/get/` endpoint sweep that returned 404
  for everything, the abbreviated-SHA probes, the `%H%x00%B` record-split that reported 0 AI-trailed
  commits out of 367, and the unanchored `boundary` grep that published 0 where the anchored count is
  560 — each refuted by its own control, all four *after* being drafted for the record, 2026-11-05)
- basalt: `context/plans/02_basalt_vendor_audit_tracker.md` (T1 Notes, 2026-11-05)
- related: § *Empty output from a failed command was recorded as a measurement* — the incident that
  produced R1–R3; and the three controls refinements folded into them (control the checker not just the
  data; tie two zeros together; size stderr).

---

## The "NInfer" QUASAR 27B looks like a Basalt model, and is not

**Trap.** Seeded from reading at init (2026-11-04), not an incident. The project's first request named
`models/vllm/jesdga/Qwen3.8-27B-QUASAR-DFlash2-nvfp4-NInfer` for `./run`, and the HF author (`jesdga`)
is also Basalt's author and publishes Basalt packs — so the artifact reads as Basalt's. It is
`qwen3_8_27b_quasar.ninfer`, a dense 27B file for NInfer. Basalt loads only Qwen3.8-Flash-Next `.basalt`
packs and would fail at load, after the container started.

**Guard.** `./run` checks the pack before starting anything: the path ends in `.basalt` and exists, else
it dies naming `../ninfer` for `.ninfer` files. Out of scope in `context/plans/SERIES.md`.

**Src:** `context/distilled.md`; the artifact's `README.md` (`library_name: ninfer`).

## The image boots the mock engine but not a real pack: `serve/` imports `tools/model_file.py`

**Trap.** [OBSERVED 2026-11-04] The first image (`27770926a3f6`) shipped `bin/`, `serve/`, `data/` and
the venv, passed every gate step and served the mock engine — then the first real `./run` died just after
`[server] legend`: `File "/opt/basalt/serve/engine/modelfile.py", line 14 … from model_file import
ModelFile` → `ModuleNotFoundError: No module named 'model_file'`. `modelfile.py` does `sys.path.insert(0,
ROOT / "tools")` and imports by **bare name**, so the planning-time grep for `from tools` / `import
tools` over `serve/` found nothing, and the mock engine never imports `serve.engine.modelfile` (it has no
model file). Upstream's `docs/building.md` says an install ships `serve/` **and `tools/`** — in
`context/distilled.md` at init and still left out of the Dockerfile.

**Guard.** Ship all of `tools/` (448 KB). Gate step 6 imports every non-test `serve.*` module *inside the
image*, so a missing file fails before a boot — on the broken image it printed exactly
`FAIL(imports) serve.engine.modelfile: ModuleNotFoundError: No module named 'model_file'`. **When
upstream's docs list what an install ships, copy that list; don't derive it from imports.**

**Src:** `Dockerfile` runtime-stage `COPY`s; `serve/engine/modelfile.py:12-14` at `dc047a77`; `RULES.md`
§ *Project gate* step 6.

## Empty output from a failed command was recorded as a measurement

**Trap.** [OBSERVED 2026-11-05] The founding incident of this file. `docker images --format … | grep -i
basalt | head` printed **nothing**, and the plan's Key Facts recorded *"No built image exists for the pin
yet — `docker images | grep -i basalt` → empty."* Docker was unreachable from the agent session at all:
`/run/docker.sock` is mode `660`, numeric owner `0:995`, and the session is `uid=1000` groups `1000,27`.
The real error — `permission denied while trying to connect to the docker API at
unix:///var/run/docker.sock` — went to **stderr**, and `grep` on stdout turned the failure into a
confident-looking empty result; it surfaced only when `PREFLIGHT_ONLY=1 ./build` exited 1. Same class,
same day, nearly cost a GPU window: `ss -ltnp 2>/dev/null | grep -E ':8700' || echo ":8700 free (no
listener)"` printed *":8700 free"*, was reported as verified, and a boot block was drafted — but **`ss`
is not installed in the agent container** (`command -v ss` → nothing) and the port was not free: `curl
http://cosmo.lan:8700/v1/models` returned **200** with `{"id":"cosmo-6000"}`, 77 GiB of 97.9 GiB
committed. Third over-claim: `git cat-file -e <blob>` "proving every blob is local" ran **without
`GIT_NO_LAZY_FETCH=1`**, so it may have succeeded by fetching mid-check.

**Guard.** **An empty result from a command that was never allowed to run is not a measurement.** R1 +
R3, and: end measurement commands with an unconditional sentinel (`; echo "— done rc=$?"`) and read it
before the output; never trust a pipeline's emptiness when the head tool can fail on permissions — check
`2>&1` once, or the *first* command's status, not the pipeline's last; in a partial clone
(`promisor=true` / `blob:none`, how `./build` clones `vendor/basalt`) put `GIT_NO_LAZY_FETCH=1` in front
of anything asking whether an object is here, and remember `git cat-file -e` proves **existence**, never
that a working file matches it (only `git hash-object -- <path>` against the blob id does); **a probe
must be able to answer UNKNOWN** — `cmd | grep x || echo "not running"` collapses *found*, *not found*
and *tool absent* into two and `pipefail` cannot help because `||` is exactly where the failure was
swallowed, so `command -v <tool>` before anything consumes its answer; **use the oracle the project
ships** — `DRY_RUN=1 ./run` runs `refuse_busy` and prints `== (DRY_RUN) a real run would refuse: :8700
or the GPU is busy` (`run:92-97`), and where a generic check is needed `curl -o /dev/null -w
'%{http_code}'` beats `ss` because a live answer *is* the answer and cannot silently not exist; and
**an environment assertion needs the right environment** — socket ownership, gid→name, "tool X is
installed", "port Y is free" are facts about *the shell that ran them*: label them "agent sandbox view,
host unknown" unless the owner's shell produced them.

**Src:** `context/plans/02_basalt_vendor_audit.md` (Key Facts, both rows downgraded with limits);
`context/plans/02_basalt_vendor_audit_tracker.md` § Notes/Deviations; `context/plans/completed/01_flashnext_container.md`
§ *Gate status*.
**Corrected in place 2026-11-05:** this entry once stated the socket *is* `root:rtkit 660` and the
session "not in group `docker` (gid 991)". That named the **agent container's** `/etc/group`, where gid
995 resolves to `rtkit` and `docker` is 991; the host socket's group was never measured and is unknown.
What is known is the numeric fact (`660`, `0:995`) and the refusal.

## A gate prerequisite recorded in RULES.md can silently stop being true

**Trap.** [OBSERVED 2026-11-05] `RULES.md` § *Project gate* step 2 stated *"`shellcheck` 0.11.0 is
installed (`/usr/local/bin/shellcheck`, 2026-11-04)"* — and on 2026-11-05 there was no `shellcheck` at
that path, on `PATH`, or in `~/.local/bin`, `~/bin`, `~/.cargo/bin`, `~/.bun/bin`, `~/.pyenv/shims`.
Step 2 failed for a markdown-only commit that could not affect it, which left unexplained reads to the
next session as *"my change broke the gate."* Same session, same shape: gate steps 3–6 are unrunnable
because the agent cannot reach the docker socket (sandbox view — see the entry above, **Corrected in
place**). Two of six steps were red for reasons no file said.

**Guard.** Fix the stale prerequisite **in place, dated** (house style for wrong knowledge). Carry gate
state where a session looks — a per-step **Gate status** block in the plan (step, last run date,
pass/fail/blocked, who *can* run it), not a paragraph in the tracker's Notes; say plainly which steps are
owner-only, and have commit messages name the steps they could not run. Never let a red environmental
step quietly become a "close enough" pass — if the owner proceeds anyway, record *"gate knowingly
degraded, owner decision, dated"*. Verify a restored tool with a **negative control** before trusting
its clean output (Plan 02 T0 required SC2115 to fire on a deliberately bad script).

**Src:** `RULES.md` § *Project gate* step 2, § *Prerequisites*; `context/plans/02_basalt_vendor_audit.md` and
`context/plans/completed/01_flashnext_container.md`, both § *Gate status*.

## A live daily-driver server is not a test bench

**Trap.** [OBSERVED 2026-11-05] Believing `:8700` was free (§ *Empty output from a failed command…* was
the cause), an agent session aimed Plan 01 T6's prescribed probes at the server actually running — the
owner's **live daily driver**, carrying their real traffic — and read its `/metrics` and
`storage/cache/basalt/<hash>/engine.log` as the product of a boot it had directed. **A chat probe costs
the owner a slot:** `GET /v1/models` and `/metrics` are free, but `POST /v1/chat/completions` is admitted
into one of `batch_slots` (4) and allocates KV, and KV here is elastic *against the expert cache* ("the
other 13.73 GiB of KV are the expert cache's last 5334 slots until a request needs them") — so the probe
can move the number being measured. Only responses that never reach the scheduler were harmless (the
bogus-model `404`, the `Host:`-check `403`). **Its telemetry is not our Evidence:** `requests[]` and the
log's prompt lines are the owner's conversation data, and a server the owner started has unconfirmed
provenance (args, image, pin). **There is no room to work around it:** one card, `expert_cache_primary_mib
64806` wanted against `vram_free_mib 24435` free (`nvidia-smi`: 77,117/97,887 MiB used); `--single`
shrinks KV slots, not the expert cache, so it needs the same ~63 GiB and **cannot** boot alongside.

**Guard.** Against a server you did not start: **read geometry, never submit work** — `/v1/models` and
`/metrics` yes, any `POST` no; never `read` the engine log's request lines, run a filter that can only
emit aggregates and record the filter and its numbers. Tag live-box readings `[LIVE <date>]` **with
"owner's server, launch provenance unconfirmed"**, and keep them out of a task's Evidence column — they
corroborate, they do not verify. Bucket every fact before writing it: *directed boot* → Evidence; *live
observation* → `context/distilled.md` with its caveat; *reachability* → a fact about the network. When the only
way forward is to stop the owner's work, say what the window costs (two weight loads at ~40 s each, plus
their clients on 1 slot not 4), put the clock in their hands, and say that declining is a legitimate
answer that keeps the task `in-progress` with the reason recorded. (`./run` would have refused anyway —
`refuse_busy`, `run:96` — so the risk was a wrong claim and a stolen slot, never a clobbered server.)

**Src:** `run:92-97`; `context/plans/completed/01_flashnext_container.md` § T6 and § *Gate status*; the live
server's `/metrics` and `storage/cache/basalt/aefd5d98dc650ed4/engine.log` lines.

## A shared worktree has no such thing as "my uncommitted edits are private"

**Trap.** [OBSERVED 2026-11-05] The agent was editing the Plan 01 plan and tracker while a second writer
worked in the **same worktree** on the host. He ran a commit; it collected the agent's in-flight files
with his own: `b308244`, subject *"record memlock unlimited inside the container (plan Q2 resolved)"*,
carried 30 lines of the agent's `## Gate status` block and two tracker edits under *his* subject and
authorship, so blame and every later `git log -S` point at the wrong task. **Two authors edited the same
bullet within the same minute and it merged into a self-contradicting paragraph on disk** (his `RESOLVED
… every expert sits in VRAM anyway` with the agent's `— still open: …` glued onto the same sentence) and
nothing in git flags it. And the agent's model — "that edit is uncommitted, it's on the bench" — became
false without notice, after which **Rule B2 removes the escape hatch**: `--amend`, `rebase`,
`reset --hard`, `revert`, and `stash` / `checkout -- <path>` / `restore` / `clean` *on work you did not
make* are all banned. Only forward commits. *(Correction 2026-11-04, host agent session: the other writer
was the **host agent session**, not the owner — `b308244` is its commit (`0b1efb6`, `aec717a`); the
lesson is unchanged.)*

**Guard.** The writer with more bandwidth keeps the tree clean: stage **explicit paths** as each logical
unit finishes (`git add <paths>`, never `-A` / `-a` — `RULES.md` § Rule B.4) to shrink the in-flight
window to minutes. **Name the dirty files in every handoff** (`dirty: context/pitfalls.md,
context/distilled.md (mine, in progress)`) — the only signal the other writer can act on without reading
a diff. **Re-read before writing a hot shared file:** `git log -1 --format='%h %ad %s'`, `git status`,
then read the *current* text of the region rather than trusting what you last wrote — a
self-contradicting sentence is the tell that someone else was there. **HEAD is ground truth; fix forward,
with dates** — keep *their* verdict and measurement verbatim, delete only your own mechanical half, and
enter disagreement as a separately attributed note (softening another author's assertion by hand is
overreach). **A `RESOLVED` headline may claim only what its own row proves** (his row said `VmLck 0` /
`VmPin 0`: the absence of a warning proves the negative case only). **Skip the machinery** — lock files,
per-agent worktrees and hooks that refuse the owner's commits cost more than the collision, and a hook
that blocks him at 02:22 gets routed around inside a week.

**Src:** `RULES.md` § Rule B2 (append-only, banned-command table) and § Rule B.4; `git show b308244`
(subject vs `--stat`: `01_flashnext_container.md` +30/−2);
`context/plans/completed/01_flashnext_container_tracker.md` § Notes/Deviations "Two writers, one worktree".

## A boot from the NFS copy crawls at 5–10 MB/s

**Trap.** [OBSERVED 2026-11-04] Boot 2 (`./run` with the pack at `models/vllm/jesdga/…`, NFS 4.2 over
RDMA from noir) printed `loading weights into cuda0 … 4.8/67.8 GiB  6 MB/s` and would have taken hours;
the owner stopped it at 7.4 GiB. The host was idle; six engine threads sat in `nfs_file_direct_read`. Two
causes stacked: Basalt reads weights with `O_DIRECT` (no switch), and the file on noir was shredded into
131,723 extents on spinning RAID. "NFS is slow" was half wrong — with the file cached on noir, direct
reads ran 749 MB/s.

**Guard.** Serve from local NVMe (`./run`'s default pack is `models.local/…`, 40 s boots). If a pack must
come from noir, defragment it there first (`btrfs filesystem defragment -f -t 128M <file>`, check
`filefrag`) — downloads land fragmented. Diagnose from the host with `ps -L -o tid,stat,wchan:32 -p <pid>`:
`nfs_file_direct_read` in state `D` is this pitfall.

**Grep:** `O_DIRECT` · `filefrag` · `wchan` · `models.local`

**Src:** `context/plans/completed/01_flashnext_container_tracker.md` (boot 2); `include/basalt/core/direct_upload.hpp:1-6` at `dc047a77`.

## Cross-repo git forensics: three commands that answer a different question than they look like

**Trap.** [OBSERVED 2026-11-05, Plan 02 T2] Three methods for "is this commit also upstream's" each
answered confidently while measuring something else; the first two built a false headline ("basalt and
Strata share history to ≈2026-10-04") in the flattering direction. **(1)** `GET /repos/<peer>/commits/<sha>`
returning `200` for five of our pre-Oct-2026 commits is not a fork boundary: **GitHub resolves that route
against the object store, not the default branch** — "on their `main`" was never evaluated. **(2)** The
first Strata fetch (`--filter=tree:0`) showed **6,347** commit objects against `main`'s 5,637 and the 710
extras matched our clone exactly — they were objects my own `git log` / `cat-file` calls had
**lazy-fetched on demand**; a pristine clone of the same ref holds **5,637 and 0 shared SHAs with us**.
**In a promisor repository the object store is a log of what you asked for.** **(3)** `git log
<up>/<br>..<pin>`, `git cherry`, cross-repo `bisect` and blame continuity **run** on histories with no
common ancestor and print a confident wrong answer — with no merge-base `A..B` is just `B`, so our whole
history reads as new work — and it does not error. (`git diff A B` is the sound exception: tree-to-tree;
our two root commits diff to nothing under it.)

**Guard.**
- **Two repos, one scratch store:** fetch **both** (`git fetch --filter=tree:0 --no-tags <remote> main`),
  then `comm -12 <(git rev-list A|sort) <(git rev-list B|sort)` → ours `|A|=2194 |B|=5637 |A∩B|=0`, and
  the store self-certifies (`in-pack` 7834 = 2194 + 5637 + 3 tags, nothing else). Print `|A|`, `|B|`,
  `|A∩B|` **together**: an empty intersection is also what a *failed* `rev-list` gives, so the marginals
  are the proof. **`FETCH_HEAD` is overwritten by the next fetch** — name the ref after each one.
- **`git merge-base` between unrelated histories exits 1 and prints NOTHING** (`mb=$(…)` is silently
  empty), and **`merge-base --independent` is not an unrelatedness test** — `main` vs a branch that
  plainly shares ancestry returns 2 exactly like two unrelated repos.
- **Clone instead of asking** (seconds, MBs, no quota): `git init p && git -C p remote add origin
  <peer-url> && git -C p fetch --filter=tree:0 --no-tags origin <branch>`, then **verify pristine before
  believing a count**:
  `in-pack` `0` → 5,637, no `objects/info/alternates`, single-branch refspec. For one specific object use
  the promisor as the oracle with a **FULL 40-hex OID** (`cat-file -e <sha>^{commit}`); the miss triggers
  a real `upload-pack` and `fatal: remote error: upload-pack: not our ref <sha>` is object-level,
  quota-free — scoped to *"not reachable from any ref this remote advertises"*, not "never in the store".
- **Match content, not hashes, when histories were rewritten:** join on `(tree, author-date,
  committer-date, subject)`, files on blob SHA, `git patch-id --stable` — ours share **0** SHAs and **844**
  content-key pairs. Check key uniqueness *before* quoting an intersection.
- **Name the repo a version string belongs to** (both publish `v0.1.0`/`v0.2.1` at **different commits**;
  Strata 95 tags to basalt's 3), and **anchor to a named snapshot hash + date** because upstream
  force-pushes (ours: Software Heritage's 2026-10-05 visit, whose `refs/tags/v0.1.0` is not today's).
- **Enumerate refs in any freeze:** 2,299 commit objects vs 2,194 from the pin, the gap being
  `refs/remotes/origin/multimodel` — **a repo can publish a whole branch outside the audited ref**.
  `git for-each-ref`, plus present-but-unreachable = 0 (`cat-file --batch-all-objects` set-minus
  `rev-list --all`).

**Grep:** `--filter=tree:0` · `merge-base` · `in-pack` · `patch-id` · `for-each-ref` · `..`

**Src:** observed Plan 02 T2 against `Niko1221/Strata` and `jesdga95/basalt`;
`context/audit/wave0/t2-lineage.txt` § 0, § 1, § 2 (2×2 remote matrix), § 4, **§ 11b**;
`context/audit/manifest/index.md` § *What "basalt ← Strata" actually means*; this § items 10–11; mirrored for
reuse at `../../../context/pitfalls.md` § *Cross-repo git forensics* (keep the shape there, the numbers
here).

## Provenance that cannot contradict its own claim

**Trap.** [OBSERVED 2026-11-05] Plan 02 carried a **P0 finding for days** on a wrong version number,
surviving *two* reviews — one of them about whether the version claim was right. Claim: "`stb_image.h` is
2.21 (2019-02-25), and the missing `STB_IMAGE_RELEASE` macro is the version evidence." The file is
**2.30**. **(1)** A line range was the version oracle: `sed -n '60,62p'` printed `2.23 / 2.22 / 2.21` —
the *middle* of a descending `RECENT REVISION HISTORY` block whose top entry is `2.30` nine lines above.
**A range silently relocates when the premise is wrong**; a pattern matches or the claim dies loudly. The
tell was in the quoted output: **a 2.21 copy cannot list 2.23 below the ceiling.** **(2)** The evidence
macro was invented: `STB_IMAGE_RELEASE` exists in no stb release, so its 0-hit grep is as consistent with
2.09 as 2.30 — **zero information** (the only version-ish macro is the unrelated `STBI_VERSION 1`).
**(3)** Two of five "discriminating" greps used identifiers that **exist in no release**
(`stbi__vertical_flip_pixels`, `stbi__jpeg_decode_block_to_dc_values`; real: `stbi__vertical_flip` 7 hits,
`stbi__jpeg_decode_block_prog_dc` 3) — five agreeing greps looked convergent and were one wrong premise
echoed. Shape: **no number in that chain could contradict the claim attached to it** — this audit's
recurring defect, same as the false-zero `grep boundary` and the all-404 sweep.

**Guard.**
- **Quote a pattern whose match *is* the claim:** `grep -n 'stb_image - v'` → `1:/* stb_image - v2.30 …`,
  never `sed -n '60,62p'`; an offset-dependent claim is about a coordinate, and coordinates move. Prefer
  **code markers to comment banners** — `#define STBI_MAX_DIMENSIONS (1 << 24)` at `:796`,
  `stbi__pnm_is16` at `:7624` are introduced-at-2.27 facts a re-labelled banner cannot fake.
- **Negative claims are the expensive ones:** name the identifier and **prove it is real** against the
  file's own vocabulary, not memory of upstream (`psize == 0` `:5564`/`:5601`, `extra_read` `:5419`,
  `IDAT section larger than 2^30` `:5182` all return hits, and would have collapsed the CVE table first).
- **Ask whether the quoted output contradicts the sentence above it** — it hides in descending lists,
  version tables, sorted histograms. Never write a bare version without the command that produced it.
- **In an evidence file a machine-format token appears exactly once, in its asserted form** (`KEY=value`,
  64-hex, `PATH:LINE`): **grep returns lines and cannot tell the value you assert from the one you quote
  to refute it** — one wrong claim about a degraded census run was carried in *five* places across *four*
  files. Quote a refuted claim **with typographic marks, refuted on the adjacent line** (the shape a
  checker can test: the paste checker fails any bare `exits 0`), and describe wrong figures in prose
  rather than as `KEY=value`.

**Grep:** `sed -n` · `STB_IMAGE_RELEASE` · `grep -c` · `v2.30`

**Src:** `context/plans/02_basalt_vendor_audit.md` § *Pre-Identified Bug/Issue* **1**; tracker § Notes/Deviations
(two stb entries, second correcting first); same shape across `wave0/t3-census-controls.txt`,
`t3-tree-contamination.txt` and a recipe elided to `…` that four documents cited while nothing in the tree
could fail on it (`wave0/t3-negctl-rechecks.txt` § *Re-fire*; `context/audit/scripts/t3-make-mutant.py`).

## An apostrophe in a comment inside a single-quoted `awk '…'` body is still inside the quotes

**Trap.** [OBSERVED 2026-11-05] A ~40-line `awk` program in `t3-census.sh` is a single-quoted argument; an
English comment **inside the body** — `# rows that do not have a site row` — closed the quote early and
bash resumed parsing the rest as shell. Two `bash -n` cycles nine minutes apart in the same block (lines
1056, 1120) and one wrong diagnosis. The offending byte was in a *comment* — nothing about `#` makes a
byte inert to the shell quoting it — and `shellcheck`'s **`SC1011: This apostrophe terminated the single
quoted string`** landed on prose three screens away, which reads like punctuation noise and is the exact
defect. `bash -n` was precise (`syntax error near unexpected token 'obs'`) and **failed loudly both
times** — which is why this is *not* a § *Shell idioms* instance. The worse cousin is a hazard only, never
observed: an apostrophe that re-splits the body into tokens that are still valid `awk` parses clean and
computes something different.

**Guard.** No apostrophes inside a quoted `awk`/`sed` body, **comments included** — `do not`, `it is`,
`cannot`; same for `case` arms and here-doc-free `-c` programs. **`SC1011` on a comment is the tell**, not
noise to suppress. `bash -n` is free and already gate step 1a: run it after *every* edit to a quoted
multi-line program, not once before the commit. For prose-heavy `awk`, stop inlining — `awk -f prog.awk`,
or a quoted here-doc into `awk -f /dev/stdin`.

**Grep:** `awk '` · `SC1011` · `bash -n`

**Src:** both instances, `context/audit/scripts/t3-census.sh`, 2026-11-05.

## A control run's blast radius — the baseline its tag wipes, and the stray files its command block leaves

**Trap.** [OBSERVED 2026-11-05] The census harness keeps `/tmp/opencode/basalt-audit/t3/run-$TAG` and
**`rm -rf`s it as its first action** — and `run-real` is not mere output: it holds the freeze artifacts
every cross-run assertion reads as its *expected* leg (**three** `.sha256` tokens — `generator`,
`predicate`, `inputs` — **plus `universe.txt`, a sorted path list, not a hash**; 232 files in 11
directories, 209 top-level). Two ways to destroy it, both by running a **control**: **tag reuse** —
`SENTINEL_TOTAL_AT_OR_ABOVE_FLOOR` is gated on `[ "$TAG" = real ]`, so firing it means a mutated copy
under `tag=real`; it ran, failed correctly, and replaced `run-real/generator.sha256` with the **mutant's**
hash (`b75fd2f0…`), which eight later controls then printed as the expected leg of
`GENERATOR_SHA_SAME_AS_REAL` (their FAILs stayed valid; every *baseline* in the window was poisoned); and
**dying after the wipe** — a batch launched from the wrong cwd resolved `MAN`/`PROJ` to `/tmp/…` and died
at `MANIFEST_DIR_NOT_FOUND`, **exit 2**, *after* the `rm -rf`, so the next batch printed
`TREE_DIFF_VS_REAL=unavailable (run-real/universe.txt missing…)`, which reads like dead code and was my
own artifact from four minutes earlier. Shape: **the destructive statement precedes the validation that
would have caught the mis-launch.**

**Guard.**
- **Controls get their own tag namespace, always**; a `real`-gated assertion runs **last, in a batch whose
  output is discarded**, or not at all with the gap disclosed. This is a *procedure*, not a prohibition —
  reading it as one cost a control (the controls file said `SENTINEL_TOTAL_AT_OR_ABOVE_FLOOR` "cannot be
  re-fired"; `wave0/t3-negctl-rechecks.txt` § C2 re-fired it with `run-real` copied aside).
- **A restore check that hashes absolute paths cannot pass:** `find … | xargs sha256sum | sha256sum` built
  from two *different* directories differs whatever the bytes do — it reported `*** RESTORE MISMATCH ***`
  on a restore `diff -r` called clean. `cd` into each side and hash relative paths; prefer an
  *independent* second method over a second run of the same mistake.
- **A restore is a re-production, not an edit:** the claim is *"a pristine re-run reproduces the pasted
  evidence byte-for-byte"* — `diff` new stdout against the paste, excluding lines that are timestamps by
  construction, pasting those two lines side by side so the exclusion is checkable. **Verify the whole
  token set**: `generator` + `sites` were checked, `predicate` + `inputs` had been overwritten too.
- **A mis-launched control is a broken control, not a fired one** — exit 2 means nothing was asserted, and
  it may still have written or deleted something.
- **A command block in an evidence file is a command someone will run, so its output paths are part of
  that file's blast radius:** a header saying `2> stderr.txt` was executed and left a 0-byte untracked file
  in the project root — in `vendor/basalt` that is invisible to `git diff` *and* counted by the freeze
  check's file leg. Write pasted redirects as absolute `/tmp/opencode/…` paths, and if a stray appears say
  it **fired**, not "would have".

**Grep:** `rm -rf` · `run-$TAG` · `--name-only` · `2>` · `sha256`

**Src:** `context/audit/wave0/t3-census-controls.txt` §§ 3–4. Related: § *Shell idioms…* items 3 and 25 (same
root; this adds the destructive half). The "four freeze tokens" wording here was corrected 2026-11-05 at
round-4 — conflating the path list with the tokens is exactly what lets someone check three hashes and
report the artifact set as verified.

## An LC_ALL naming a locale the host does not have falls open to C — no warning, no error

**Trap.** [OBSERVED 2026-11-05] `locale -a` here lists exactly `C`, `C.utf8`, `POSIX`, yet
`LC_ALL=en_US.UTF-8 sort` produced `B a b` — **identical to `LC_ALL=C`** — with no warning, error or exit
code: glibc resolves an unknown locale name to C and keeps going. Casualty: the census assertion
`SORT_DEFAULT_ORDER_EQ_C`, whose purpose is to catch a non-C collation in the environment and whose
realistic trigger **cannot be produced on this box** — so it is green for a reason unrelated to what it
guards, and **a genuine regression passes**: drop `LC_ALL=C` from a `sort` line and the output is
unchanged, so it still prints PASS.

**Guard.**
- **`locale -a` before you arm a locale-dependent tripwire** — if the trigger locale is absent the check
  is decorative, and says so only to whoever thought to ask.
- **When the realistic trigger can't be produced here: prove liveness another way, label what the proof
  covers, record which arm is unarmed** (script comment *and* evidence file). Liveness here came from
  reverse-C input (`actual=bBa`), which proves the comparator is **live** but **not** that it is
  collation-sensitive — the locale arm stays unproven. And the measurement only established "output is
  C-identical": it could **not** separate *locale absent → glibc falls back* from *present with an
  identical collation*; `locale -a` is the sole evidence for the first reading.
- **Don't install a locale to make a check exercisable** — you change the environment it polices and
  leave a tripwire only your box can fire.
- **Pin collation inside the script** (`export LC_ALL=C` at the top, as `t3-census.sh` / `t3-inputs.sh`
  do). Seeded hazard, **not observed**: a `comm -23` / `sort -u` pair sorted under *different* collations
  can silently agree or silently disagree.

**Grep:** `LC_ALL` · `locale -a` · `comm -23` · `sort -c` · `C.utf8`

**Src:** `printf 'a\nB\nb\n' | LC_ALL=en_US.UTF-8 sort` → `B a b`; comparator control =
`t3-census-controls.txt` **C7** (its `bBa` is reverse-C, never locale output); `t3-census.sh`
`SORT_DEFAULT_ORDER_EQ_C`. Host-shaped half at `../../../context/pitfalls.md`.

## A dated fact scoped to a shell I could not enter was declared "stale" — the scope caveat was on the adjacent line

**Trap.** [OBSERVED 2026-11-05] Grep found `python3 3.14.10` in three files (`RULES.md:456`, tracker
`:37`, `context/distilled.md:162`); my shell reports `3.12.16`. I told the human — in prose, one full turn — that
**a false fact was sitting in the project's memory**, and started writing the correction. Then I read the
±10 lines: the heading is `## This host: one sm_120 card, 125 GiB RAM [LIVE 2026-11-04]` and **one line
below** the number sits *"Tool versions here are the 2026-11-04 shell's view, not a host constant …
Re-check in the shell that will use the answer."* Nothing was stale; **the false claim had already been
published**, and that ordering — assert first, read second — is the bug. Its sibling proves the caveat is
load-bearing: the record says `docker 29.8.1` (2026-11-04), `docker --version` here reads **29.9.2** —
and **my fresh number is not a *better* number**, it is today's shell's view exactly as theirs was, so
the "correction" would have swapped one environment's snapshot for another's and destroyed the older one.
**This error is silent**: overwriting a scoped measurement with a value true of *your* shell makes the
file read *more* accurate, and no later check or reader fails on it. Same five minutes: a reviewer
asserted `3.14.10` was "implausible as a released CPython patch" hence a transcription slip — python.org
says **released 2026-10-25**; plausible, confident, wrong. And the one genuine defect among the three
hits was `RULES.md:456`, the only copy with **no date and no environment named**, in a *Prerequisites*
line — the sole copy asserting universality (§ *A gate prerequisite…*).

**Guard.**
- **Before naming a recorded fact stale, read the ±10 lines, not the hit line.** A date tag, environment
  name or "not a constant" clause on an adjacent line changes what the number claims, so it decides
  whether your measurement can contradict it.
- **A grep for a value returns lines; scope is not on the line** — a hit list is *candidate sites*, never
  a list of facts to correct. Mirror of `context/plans/RULES.md` § *Rule E — Evidence over assertion*, item 7
  ("grep for the claim, not the phrasing") applied to **values**: grep finds copies, **scope decides
  whether a copy is wrong**.
- **Correction test — which environment does the record claim?** If not yours, you may only **append a
  newer dated observation with its own scope**: keep `29.8.1 [LIVE 2026-11-04]`, add `29.9.2 measured
  2026-11-05, agent container`, leave both. Fix the **unscoped** copy; leave the scoped ones alone. If a
  number can't be re-derived, record *that* ("environment not named; not re-derivable") rather than
  assigning one.
- **Say the finding, then verify it, then say it again.** Publishing first is the expensive half; the
  retraction is cheap.

**Grep:** `stale` · `host constant` · `[LIVE` · `python3` · `--version`

**Src:** `context/distilled.md` **`:158-166`** — heading § *This host: one sm_120 card, 125 GiB RAM*
`[LIVE 2026-11-04]`, the number at `:162`, its own caveat at `:163-166` (left untouched; today's python
reading appended there). Cited by line because that file has **two** headings beginning
`This host: one sm_120 card` (`:136`, `:158`), so a §-name citation identifies neither; the unqualified
copy that *was* wrong for this shell is `RULES.md` § *Prerequisites*, corrected 2026-11-05 to a measured
value with the environment named; the refuted inference — python.org download history, retrieved
2026-11-05; the rule this generalises into, `context/AGENTS.md` § *Provenance*. Observed Plan 02 T3
while preparing `wave0/t3-sentinels-real.txt`. **Instrument vs
reader:** § *Provenance that cannot contradict its own claim* is the evidence defect; this is the reader
defect — the provenance system worked and I overrode it. Kept separate on purpose.

## A commit is the index, not the file I was looking at

**Trap.** [OBSERVED 2026-11-05] Tokens: `git status`'s `XY` (a leading space — ` M` — means
*worktree-only*, index clean), `git diff` vs `git diff --cached`, `git show <commit>:<path>`, and the
project-specific blade: **gate steps 1a/1b/2b enumerate `git ls-files`, so `shellcheck ok` is a claim
about the index, never about the bytes that landed.** A `wave0` evidence file was `git add`ed, edited
**four times** to replace four truncated hashes, and never re-staged: the commit shipped the **pre-edit**
content, and ` M context/audit/wave0/t3-census-controls.txt` was the entire available signal. Worse than
stale bytes: **the commit message asserted a property of the committed file that was only true of the
worktree** — "all ten are now byte-equal to their transcripts, verified block by block" — while every one
of those checks had run as `grep … <path>` against the worktree. Staging is a **snapshot**, and every
later green check on the path says nothing about what is committed; a review cycle that ends in edits is
exactly the sequence that produces this.

**Guard.**
- **`git add` is the last thing before `git commit`, not the first.** After the final edit re-stage, then
  `git diff --stat` must print **nothing** for every path the commit claims. Sequence the two states into
  one: lint → `git add` → gate → paste the numbers → commit (1b/2b read the index, so `AUDIT_B=5` is only
  true *after* the add).
- **Verify claims against the commit object** — `git show HEAD:<path>` / `git cat-file blob HEAD:<path>`.
  A sentence about "the committed X" must name `HEAD:` or it measured the wrong file.
- **After committing, `git status --porcelain` is empty** — a leftover ` M` inside the commit you just
  made is proof it holds different bytes.
- **Fix forward:** new commit + a dated transcription note *inside* the file saying which commit shipped
  the stale form. Never amend. And `git commit <path>` commits **worktree** content for that path while
  taking the rest from the index — stage and commit with no pathspec.

**Grep:** `git add` · `git status --porcelain` · ` M ` · `git diff --cached` · `HEAD:`

**Src:** `dd94598` shipped the stale copy (its message carries the over-claim); the correction is the next
commit and inserts the note into `wave0/t3-census-controls.txt`. Family: § *Shell idioms…* (input isn't
the thing under test), § *Cross-repo git forensics…* (wrong *object*), § *A shared worktree…* (one index,
two authors). This is the **index vs worktree** axis — the one that bites a solo session.

## A number measured on mutated copies got recorded as a property of the tag they ran under

**Trap.** [OBSERVED 2026-11-05] `t3-census.sh` prints `SENTINEL_TOTAL`: **34** on the `real` run, **28**
in the controls evidence, and I wrote it down as *"`SENTINEL_TOTAL=28` on `negctl`"* — in the evidence
file **and** the tracker's canary row, which is the file a future session seeds a floor constant from.
Every one of those 28s came from a **mutated copy of the generator**: of thirteen mutant runs on disk,
twelve ran under `tag=negctl` and one under `tag=real`, and no unmutated non-`real` run had then been
made — so the label attributes to the *tag* what belongs to *ten different edited programs that happened
to share that tag on the command line*. Nothing catches it: the **label was real**, and **28 < 34 has a
correct causal explanation about the tag** (a block of assertions is `[ "$TAG" = real ]`-gated) — a
confound with a right explanation passes both author and reviewer. The script itself said "**measure this
line on the first canary run**" before any per-tag floor existed; the prose walked past it.
`SENT_TOTAL_FLOOR_REAL` had already been guessed once (as **31**) and printed a green over the wrong
quantity.

**Guard.**
- **Before "X is N on tag T", name the artifact that produced N** — the program, by hash. Many differing
  artifacts means no measurement of *T*, only N measurements of N mutants. **A control run measures the
  mutation, not the environment**; anything else it prints is a mutant property, labelled at its own
  block, never a section-level fact.
- **State the population, then the count** ("thirteen mutant runs, twelve under `negctl`, one under
  `real`") and **name it by the field you read** (`TAG=`), not the one you were reasoning about
  (`GENERATOR_SHA256=`) — conflating them wrote "13 non-real runs" for 12. Where a value will seed a
  threshold, put its provenance **next to the number**, not next to the rule.
- **Sort same-identifier values by *kind of cause* before naming producers.** `SENTINEL_TOTAL=` appears
  with **five** values — 34, 28, 27, 23, 31 — and **three kinds of cause**: **program identity** (34 vs
  27, two cuts of the generator), **tag** (34 vs 28, real-gated assertions), and **run state** (one more
  assertion is real-gated *and* data-conditional, so a post-fill run of the same program on the same tag
  prints a different total). **A run total is identified by (program, tag, state) — three coordinates**;
  I published a partition naming two, correct only for the state I happened to have measured. All five
  values come from the **census**; the PoC harness never prints this identifier — it prints `POC_VERDICT` —
  so an earlier draft of this entry, which proposed renaming *the PoC's* counter, proposed a fix with no
  target. Annotate at the line rather than renaming a committed script — renaming orphans its own evidence.
- **Before "no clean measurement of X exists", check nobody merely failed to make one.** That clause was
  true when written and false hours later: the run had never been *forbidden*, only never *attempted*
  (`tag=negctl` writes its own namespace). Read the cleanup line and ask what the run would destroy; if
  the answer is "nothing I care about", make the measurement — an unmade measurement is a to-do, and
  writing it as a limitation understates your evidence *and* overstates the gap.
- **Warrant, in the order a reader should reach it:** (i) the partition is **source-checkable, no scratch
  needed** — match each `chk` to its enclosing tag conditional: 15 of 16 names sit under
  `if [ "$TAG" = real ]` / `!= real`, and the 16th (`TREE_FREE_OF_BUILD_ARTIFACTS`, `:222`) is gated by
  the **one-line idiom** `[ "$TAG" != real ] || chk …`, which a grep for `if [ "$TAG"` misses; (ii) the
  **totals need a run**, and now have a clean one — an unmutated `tag=negctl` invocation of the frozen
  generator prints `SENTINEL_TOTAL=28 SENTINEL_FAILS=0`, exit 0, and nine single-mutation runs of that cut
  print an identical 28-name set; (iii) the `/tmp` `TAG=` / `GENERATOR_SHA256=` lines are scratch —
  re-make a mutant rather than reading them.

**Grep:** `SENTINEL_TOTAL` · `FLOOR` · `on <tag>` · `under tag=` · `baseline`

**Src:** `context/audit/wave0/t3-census-controls.txt` § 2 ("What the `28` is actually worth", retitled
2026-11-05 when the clean run above falsified its claim) + the tracker's canary Evidence row;
`context/audit/scripts/t3-census.sh`'s `SENT_TOTAL_FLOOR_$TAG` comment.

## "You can reproduce this from the file alone" — a re-derivation claim that never names which cut of the program it starts from

**Trap.** [OBSERVED 2026-11-05] Ten evidence blocks record a `GENERATOR_SHA=` — the hash of the *mutated
census script* behind that block's stdout. The hashes were typed from truncated output, so I wrote an
invariant to rescue them: every mutant is *the frozen base plus exactly one exact-string replacement
applied exactly once*, the `mutate:` pairs printed in the file are the recipe, so
`sha256(frozen base + substitution)` reproduces any of the ten. **False the day it was written, and false
in the direction that mattered**: it promised a later reader verification they cannot perform. Measured
the same session: every recipe's left-hand side **does** occur exactly once in the frozen script (10/10 —
transcribed and located correctly), but re-deriving from the frozen base reproduces **0 of the 9**
re-derivable tokens (the tenth is a deliberate `deadbeef`). The base was not the frozen script:
`t3-census.sh` carries **mtime 06:35:10**, the last control ran at **06:35:08**, the pasted `real` run
started **06:35:14** — the generator was rewritten *between* the controls and the run they are cited
beside, and the ten mutants came from a **pre-freeze cut whose bytes exist nowhere** (one blob ever
committed; mutant copies deleted by design). One clean case is disproof by itself: a control whose
mutation is a one-token change on a unique standalone assignment line — had its base been the frozen
script, its hash would have followed arithmetically; it does not. The sentence named a **recipe**
precisely and a **base** loosely: "the frozen generator" felt obvious because the file's `Subject:` line
names it, but the Subject names the program that made the *paste*, and the controls predate the paste.

**Guard.**
- **A reproduction claim has two ingredients and the vague one is where it dies:** recipe *and* base. Base
  = "the version in the commit" → name the commit and give the blob oid; base = "the file as of that run"
  → say how a reader gets those bytes; if they can't, **it is not a reproduction claim**, so say what it is.
- **Test your own invariant with the cheapest case that could refute it** — a fifteen-line script here,
  cheaper than the paragraph it invalidated. The recurring failure is a claim written as apparatus
  *around* evidence and never run against it.
- **Watch for a base that moved silently:** an `mtime` later than the runs it documents is the whole
  finding and is one `stat` away; cross-check the transcripts' own start stamps, not memory. *Every batch
  of runs has a base, and batches do not share bases just because they share a filename.*
- **Prefer identification that survives its base:** the assertion name, verdict and argv are reproducible
  from the output; a program hash is not. Cite something comparable without the program alongside it.
- **Say "witnessed", not "re-derivable", when the bytes are gone** — the recipe, the assertion that fired,
  its verdict and argv are enough to re-make the mutation and watch it fail, which is the purpose of a
  control; they do not reproduce a hash, and a reader told otherwise blames themselves for failing.
  (Salvage: two differently-named runs carrying the **same** program hash confirm these hashes index
  programs, not runs — the property the claim assumed.)

**Grep:** `re-deriv` · `reproduce` · `from the file alone` · `mutate:` · `mtime`

**Src:** `context/audit/wave0/t3-census-controls.txt` § *Method* rule 4; the measurement (10/10 unique, 0/9
matches, `stat` vs the thirteen transcripts' `started=`); in-tree residue = the two-cut digest table in
`wave0/t3-gate-and-checks.txt` § 2a, where a block that only **moved** (`ba991abf…`) sits beside one that
**changed** (`2ed67035…` → `7a3e901a…`).

## A census keyed on a path set cannot see a modified file — and the gate that looks like a content check is not one

**Trap.** [OBSERVED 2026-11-05, found by reading the harness before planting the canaries rather than by
being burned] `t3-census.sh`'s universe is **one path per line**, hashed (`:225`, self-described: "a hash of the
sorted path set, NOT git's tree hash: the tree hash covers contents and this one does not"), and every
tree-level assertion inherits that blindness: `UNIVERSE_SHA_MATCHES_PIN` hashes the **path** list (and is
a `DISP` on non-`real` tags); `TREE_DIFF_NO_REMOVED_PATHS` / `TREE_DIFF_ADDITIONS_ARE_PLANTED` `comm` two
**path** lists (`:260-261`), blind to any path present on both sides whatever its bytes; the stray test at
`:271` filters `tree-added.txt` with `awk '!/CANARY\|SENTINEL\|canary/'`, so it sees everything except
what was not **added**. Scope it to *the tree
layer* — an earlier draft said "at all" and was corrected at round-4: `LOC_TOTAL` (`:248`) is a `wc -l`
over every path, compared at `:252`/`:253` inside the `:251` real-gate. Exact statement: invisible to the
tree layer on **every** tag; invisible to *everything* on a non-`real` tag; caught on `real` **iff the
line count moves**; **a line-count-preserving byte edit is invisible everywhere**. Neither remaining leg
is a content check — the `ge 1` denominators (`:743-744`) surface a body edit only if it zeroes a count,
and the manifest already publishes a **per-file sha256 in column 3** that **no `chk` in the generator
re-derives**. Two deceptions: a name that reads like a content check (`TREE_DIFF_ADDITIONS_ARE_PLANTED`
validates which paths *exist*), and a display that reads like a comparison — `DISP
UNIVERSE_SHA_MATCHES_PIN … (canary tree: hash compared to run-real, not to the pin)` when **nothing
compares it** (`DISP` never increments `SENTINEL_TOTAL`; the parenthetical says the opposite, and skimmers
read the parenthetical).

**Guard.**
- **Ask what a scan's universe is keyed on — paths, contents, symbols — before trusting "differs only
  by".** A path-keyed universe plus a path-keyed diff is a claim about *filenames* however phrased; if
  content matters the key must be per-file content hashes (`find … -print0 | xargs -0 sha256sum | sort`),
  which is a different assertion.
- **Design fixtures to the harness's key, not the taxonomy:** under a path-keyed harness, plant **new
  files** so they land in `tree-added.txt` where an assertion can see them, keep their *directories*
  ordinary (nothing should find them by location), put the token in the **filename**. A purely in-place
  edit cannot: state it as a fixture limit rather than planting a modification and crediting the harness,
  which prints `TREE_DIFF_VS_REAL=+0/-0` as if nothing happened.
- **A "not controlled" list belongs here, before the fixture** — controls § 2 for "there is no modified-path
  leg at all", the tracker's canary row for the fixture constraint. Writing it after the canaries are
  scored means they were designed against an unchecked assumption.
- **Where a prose qualifier inside a machine-format line contradicts the format's convention, fix the prose
  or add the missing check — never rely on the reader noticing.** `DISP` + "compared to run-real" is the
  shape; the fix is a `chk` or a caption saying "printed, not compared".

**Grep:** `universe` · `path set` · `comm -13` · `tree-added` · `ADDITIONS` · `DISP` · `sort -u`

**Src:** `context/audit/wave0/t3-census-controls.txt` § 2; `context/audit/scripts/t3-census.sh` `:225`, `:235`
(`EXPECTED_LOC`), `:238-247` (`DISP`), `:259-295`, `:271`. Related: § *Shell idioms…* (same defect one
level up — the *choice of subject*) and § *A number measured on mutated copies…* (evidence about a
different thing than the claim).

## A hash retyped from scrollback came out as 64 valid hex characters that were not the hash

**Trap.** [OBSERVED 2026-11-05] Ten `GENERATOR_SHA256` values sat in scrollback and the evidence file was
**composed by hand**: **four of the ten hashes were wrong.** C2 read `83b57240…9296afd2…` against a truth
of `83b57240…92afd2…` — 66 hex characters, not 64 (length
catches that one). C8 `…4df4e004dc72` vs truth `…4df4e00bf808`, C9 `…671493857ba5` vs `…6714938fe00b`,
C10 `…3209bb5d2f28` vs `…3209bb5e69fd` — **52 correct characters copied from memory, then a plausible
fabricated tail to reach 64**: well-formed, they grep as hashes and sort. In the same file a `FAILED:`
transcript line had its two names silently **reordered**, making it a sentence I wrote rather than a line
the program printed — invisible to the *prefix match* used to spot-check it. Shape: **an evidence file
whose own header says "raw output — never hand-edit" was hand-edited into existence**, with every number
still *about* the right runs, which is why it reads clean.

**Guard.**
- **Emit the artifact; don't type it.** Prose gets typed; transcript lines are copied out of saved output
  by the machine. No emitter ⇒ the artifact is not regenerable, which is a defect of the artifact.
- **Verify a pasted line by exact membership** (`quoted_line in output.split("\n")`) — not by eye, not by
  prefix (prefix is what let the reordered line through). Truncations carry a `…` marker **produced by the
  script**, so an accidental short copy cannot pass as a deliberate one.
- **A sha is verified by equality with the run's own output, whole.** "64 hex characters" is a *format*
  check; three of four bad hashes passed it — a **75 % false-negative rate** on the thing it looked like
  it verified.
- **If a value was not saved to disk, re-run and save it, or label the claim witnessed-only** — exit codes
  were never captured in that batch, and retyping them is how this entry happened.
- **Ship the checker *with* the evidence** — re-extract every claim from the raw outputs, exit non-zero on
  drift. The one that caught this was written after the commit; write it before.

**Grep:** `GENERATOR_SHA256` · `as typed` · `split("\n")` · `startswith`

**Src:** `context/audit/wave0/t3-negctl-rechecks.txt` (regenerated from `/tmp/opencode/recheck2/`); caught by
`verify_paste.py`'s membership check, one commit late. Related: § *A commit is the index…* (a description
of a state, not of the bytes) and `t3-census-controls.txt` § 4's "matches, **as typed**" vs "identical" —
the vocabulary distinction this entry argues for.

## A checker's self-test re-ran the checker unguarded — a fork bomb, and the checker had never once run

**Trap.** [OBSERVED 2026-11-05] `context/audit/scripts/t3-paste-check.py` ended in
`sys.exit(run_selftest())` with **no `if SELFTEST:` around it**. The self-test re-executes the file once
per corruption case (21 + the clean run, 8 at a time); each child hit the same line and did the same, and
the fan-out grew until the host ran out of memory — between the last orphaned write (00:18 PST) and the
reboot (03:05 PST), killing the session that wrote it with its round-4 work uncommitted. Found only when
the guard went in and the file reached its end for the first time: **every line below the call was dead
code** (no invocation had ever reached families A–L, so "the checker ships with it" described a checker
with **no completed run**); **`--tree-only` was not tree-only** (family I re-digested batch transcripts and
read the `/tmp` snapshot, printing `PROBLEMS=51`, all `MISSING FILE /tmp/opencode/…`, against a docstring
promising "needs nothing but repo bytes"); and **the self-test could pass by construction** — it scored a
case on *any* line containing the needle, so the three `k-*` needles matched family K's **section header**
and `L-key-removed` matched the applier's `VACUOUS` refusal before L ran: four of nine tree-only cases were
"FIRED" on lines their own check never printed.

**Guard.**
- **A script that re-executes `__file__` must make re-entry impossible, not unlikely:** guard the call
  site *and* mark children (`T3_PASTE_CHECK_SELFTEST_CHILD=1`) so a child refuses even if the guard is
  lost again. First run of any new self-test under `ulimit -u <current + small>` + `timeout`, watching the
  process count.
- **Code after an unconditional `sys.exit` is dead and nothing warns you.** A checker that has never
  printed its last line (`PROBLEMS=`) has never run, whatever the evidence beside it says.
- **"Repo bytes only" is a claim — test it on a machine without the corpus** (here, accidentally, by a
  reboot that wiped `/tmp`).
- **Score a self-test on failure lines only** (`BAD …`, `CANNOT CONTINUE`), with needles only the owning
  check prints; a needle that also matches a header or another check's message is a control that cannot
  miss — the defect this series keeps finding.

**Grep:** `sys.exit` · `run_selftest` · `__file__` · `PROBLEMS=` · `ulimit`

**Src:** `context/audit/scripts/t3-paste-check.py` (`run_selftest()` docstring, the `if SELFTEST:` call site,
`TREE_NEEDLE`), adopted after the OOM; the tree-only run `PROBLEMS=0`, self-test `fired 9/9`,
`NOT RUN: 13 of 22` in the Plan 02 tracker Evidence. Related: § *A hash retyped from scrollback…* —
shipping a checker is not enough; it has to have *run*, with its record in the evidence.

## A census that drops wide-only hits cannot see a hard-coded URL

**Trap.** [OBSERVED 2026-11-05] The primitive census has 15 families (sockets, DNS, exec, `dlopen`, file
writes, credentials, input capture…) and **none matches a URL literal or an encoded string**; its wide
patterns *do* see them, but a wide hit becomes a site only when the same file also has a narrow hit.
Planted canaries proved it: a raw `socket()`/`connect()` became a site, while a base64 blob decoding to an
`https://` URL and a literal `https://` in a header landed in `*.wide.tsv` and were dropped
(`column6 = none`). A file whose only suspicious content is a beacon URL would never reach a reader.

**Guard.** **Predict canary outcomes from the detector's family list before planting** — the prediction
(1–2 missed) came from `families.tsv` and the run confirmed it in one go. **A wide hit never promoted is a
silent drop**: give "seen by a wide pattern but not a site" its own output, or add a narrow family.
Closing it does not need a frozen generator edited — a separate one-command sweep with its own canary
control is cheaper and keeps the census's identity stable.

**Grep:** `families.tsv` · `column6` · `\.wide\.tsv`

**Src:** `context/audit/census.md` § Canaries 1–3, § *The URL/encoded sweep*; `context/audit/wave0/t3-census-rerun.txt`.
