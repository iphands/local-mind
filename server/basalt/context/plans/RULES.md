<!-- hands-plan:v1 2026-11-04 -->
# Plans — Rules & Conventions

> **Read this file in full before writing a plan, a tracker, or any code for a plan.**
> **This file wins** over a root `AGENTS.md`/`CLAUDE.md`, a plan, or a habit that disagrees.
> Sections marked `<!-- hp:… -->` come from the `hands-plan` skill. The plan-gate rows and the
> project gate belong to this project; in the other skill sections, project-specific text lives
> under a `#### Project addendum` heading — an addendum may tighten a rule, never loosen it.

---

## When Does a Change Need a Plan? <!-- hp:plan-gate -->

| Change | Plan? |
|---|---|
| A typo, a comment, a doc fix, a constant tweak, a one-file obvious fix | **No.** Do it, verify (Rule A), commit. |
| A pin change in `versions.env` (the Basalt commit, the CUDA base image tag or digest) | **Yes.** A Basalt bump can rename or drop a flag (upstream keeps no shims) or change output numerics; the plan says what the release notes changed and re-boots. |
| A Dockerfile or build-flag change (`CMAKE_CUDA_ARCHITECTURES`, `-march`/AVX-512, `WERROR`, what ships beside `bin/`) | **Yes.** A build that compiles can still miss `data/profiles` or emit the wrong SASS; the plan names how the built image is proven. |
| A default in `./run` or the generated `basalt_config.json` (expert cache, prefill, KV format, spec depth, parallel slots, memory floors) | **Yes.** It changes what every boot does; the plan records the before/after it was chosen on. |
| A new model pack or launcher profile | **Yes.** A pack Basalt cannot load (e.g. a `.ninfer` file) must fail fast in `./run`, not in the engine. |

When in doubt, write the plan. It is cheap; an unplanned multi-file change is not.

---

## Files & Naming <!-- hp:naming -->

- `NN_name.md` — the plan. Number is **at least two digits** (`01`…`99`) and keeps growing past
  `99` (`100_…`). snake_case name. **Never renumber a file that exists.**
- `NN_N_name.md` — a sub-plan of plan `NN` (e.g. `03_1_storage_schema.md`).
- `NN_name_tracker.md` — the paired tracker. Every plan has one.
- `SERIES.md` — dependency chain, status of every plan, and the **next free plan number**.
- `NN_example.md` + `NN_example_tracker.md` — the canonical skeletons. Copy them; never edit
  them to write a plan.
- `<TOPIC>_RULES.md` — optional series-scoped rules (see *Series-Scoped Rules*).
- `completed/` and `abandoned/` — closed plans (Rule C). Created by the first close.

Take the next number from `SERIES.md` and confirm it is unused across the active directory,
`completed/` and `abandoned/`. If they disagree, use the larger and fix SERIES.

---

## Plan Format <!-- hp:plan-format -->

Copy `NN_example.md`. It must open with the title, this metadata block, and the mandatory
header — in this order:

```markdown
# Plan NN — [Title]

> **Status**: pending | in-progress | blocked | done | abandoned
> **Created**: YYYY-MM-DD
> **Revised**: YYYY-MM-DD — what changed (optional; add one every time the plan is re-scoped)
> **Depends on**: Plan N | N/A
> **Goal**: One-sentence deliverable.
> **Agent**: implementation agent | ralph-loop | sub-agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.
```

Required sections, in order. A required section is never deleted: if it genuinely does not apply,
write `N/A — <reason>` under its heading. Sections marked *optional* may be left out.

| Section | Must contain |
|---|---|
| `## TL;DR` | **What** (one sentence), numbered **Deliverables**, **Estimated effort**. |
| `## Scope` *(optional)* | What is in, what is out, and: if the work must cross the line, stop and record a blocker in the tracker instead of expanding scope. |
| `## Context` | Why the plan exists. `### Pre-Identified Bug/Issue` with the **command that reproduces it and its verbatim output** (for a feature: the command showing today's behavior) — never a paraphrase. `### Why [Approach]` naming the rejected alternative. `### Key Facts` as a table with a **How confirmed** column (command + date, `path:line`, or "unconfirmed — confirm in T1"). `### Rejected Claims` *(optional)*: things that look like bugs but are not — "do NOT re-fix", with the evidence. |
| `## Step-by-Step Tasks` | One `### TN: [title]` per task with **File**, **What to do**, **Before/After** (when the edit is known at planning time; otherwise name the target symbol and the evidence source), **Verify** (the command that exercises *this task's* change + expected output, then the project gate — mandatory), **Expected observation** (mandatory on measurement/investigation tasks: what would confirm, what would refute, what counts as noise — written *before* running anything), and **Commit** (the message, per Rule B). Large plans group tasks into **waves**, each with a stated exit gate. |
| `## Critical Files` | Table `File \| Change \| Priority` — `P0` blocking, `P1` important, `P2` nice-to-have. |
| `## Open Questions / Risks` | Numbered. Each names the risk or question and its *Mitigation* / *How we'll settle it*. **Never delete one.** Resolve it in place (~~strike~~ + "RESOLVED (T3): …") or mark it `deferred — <reason>` at the moment it is deferred. |
| `## Verification Checklist` | One checkbox per task, each a **testable assertion with an observable result** — not a restatement of the task. Closers: findings harvested (Rule D); plan + tracker moved and SERIES updated (Rule C). **Tick a box only against evidence you produced.** An untickable box stays unticked with a note saying why. |

---

## Tracker Format <!-- hp:tracker-format -->

Copy `NN_example_tracker.md`. Sections: **Overview** (percent complete as `N% (X/Y)`, start
date, where evidence lives), **Resume Instructions** (numbered — what to read, environment,
ordering constraints, commit format), **Open Unknowns** *(optional)*, **Progress**, **Evidence**
*(required when the plan produces numbers or live checks)*, **Notes / Deviations**,
**Follow-ups**.

- Task status values: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid`.
  `blocked`, `skipped` and `invalid` always carry a reason in the row's Notes cell.
- Record negative and inconclusive results ("tried X, no measurable effect, 3 runs"). They stop
  the next session from repeating the work.
- **Notes / Deviations** is where a plan premise that turned out wrong is written down. Be blunt.
- A row never records its own commit hash — the row lands in the same commit (Rule B). The
  commit format makes the commit findable instead.
- When the first task starts: plan `Status` → `in-progress`, the SERIES row → `in-progress` and
  listed under *Currently Active*, tracker `Start date` set — all in that task's commit.

---

## Per-Task Execution Rules

These apply to **every task** (T1, T2, …). They are not optional.

### Rule A — The verification gate <!-- hp:gate -->

A task is verified when you have **observed the change working**, not when it compiles and not
when the diff looks right. Reading the diff is not verification.

1. Run the project gate below after every task. It must exit 0 with zero errors and zero warnings.
   Fix warnings before marking anything done.
2. Know what the gate cannot see. Exercise the behavior the task changed — run it, hit it,
   render it, measure it — and record what you observed in the tracker.
3. Prove the thing you tested is the thing you built — that the new build actually loaded.
   A binary, image or driver that never ran "passes" every test.
4. Anything a human sees (UI, output, logs) must be **looked at**, not inferred from a passing
   build.
5. **Keep the gate current.** A task that adds behavior the gate should exercise (a flag, a mode,
   an endpoint) updates the project gate in the same commit.
6. **Never mark a task `done` on unverified work.**

#### Project gate <!-- hp:gate-project -->

There is **no compiler here and no CI**. This project is bash scripts, a Dockerfile, a pin file
and generated config that drive a C++/CUDA engine it does not contain
([jesdga95/basalt](https://github.com/jesdga95/basalt), built inside the image). A launcher typo
is a server that never boots; a flag Basalt renamed is a refusal at boot (upstream keeps no
aliases); a wrong default is a slower server that still answers. So the per-commit gate is
mechanical, needs **no GPU and no model load**, and runs before every commit, from the project
root (`server/basalt`).

Every step reports **pass / fail / skip**, and **skip is not pass**: no task is `done` on a run
where an applicable step was skipped.

**Named carve-out — owner-only steps (the only permitted skip).** Steps **3–6** are owner-run on this
host, per § *Prerequisites* (an agent session here is not in the `docker` group). A task may be marked
`done` with 3–6 skipped **only** when all four hold. Conditions (3) and (4) are **machine-checked and
pasted, never asserted**, because a condition an agent restates in prose is a condition it grades itself.

**(1) Name them.** The commit message and the tracker row name *which* steps were skipped and why, in the
established form: `Gate 1a/1b/1c/2 pass, 3–6 docker-blocked (owner)`.

**(2) No retroactive reclassification, across commits.** The step must already be listed owner-only in
§ *Prerequisites* in a commit **strictly earlier** than the first commit that invokes this carve-out for
that step — an edit made minutes earlier in the same session does not count, or the clause defeats itself.
This is verified **once per plan, not per commit**, because a per-commit date comparison across two
`git log -S` lists is the condition agents silently recall instead of run. The plan's § *Gate status* block
records the pair as data — `owner-only listing: <sha> · first invocation: <sha>` — and every later commit
cites that line. Re-derive only if the listing line itself is ever edited
(`git log --oneline -S'owner-run' -- context/plans/RULES.md` must still predate
`git log --oneline -S'docker-blocked'`).

**(3) Diff scope, pasted.** The commit must touch nothing any blocked step could exercise; the surface is
**whatever the printed `PATTERNS=` line names** — that line is the authority for what was checked, and the
prose here is only a gloss on it. Run it against the **staged** set, because the evidence has to be in the
commit's own message and an unsaved commit cannot be `git show`n:

```bash
# PRE-COMMIT twin — run staged, this is the block that goes in the evidence file
# Run from the project root with diff.relative unset/false (see below): the strip assumes
# monorepo-root-relative output.
PAT='^(run|build|Dockerfile|Dockerfile\.dockerignore|versions\.env|lib/|models|\.gitignore)'
W=/tmp/opencode/scope-$$-$(date +%s).txt      # per-run name: a fixed path lets two sessions overwrite
git diff --cached --name-only | sed -n 's|^server/basalt/||p' | grep -v '^$' > "$W"
echo "FILES=$(grep -c . "$W") SCOPE_HITS=$(grep -Ec "$PAT" "$W")"
echo "PATTERNS=$PAT"
rm -f "$W"                                     # throwaway; the evidence is the printed line, not the file
```
**Where it goes:** the three printed lines go into the active plan's `wave<N>/` file (raw output belongs
there, never in a tracker — `context/AGENTS.md`); the commit message and the tracker row quote only the
single `FILES=… SCOPE_HITS=…` line. After committing, re-run the same check against the **commit object**
(`git show --name-only --format= <sha>` in place of `git diff --cached --name-only`) and confirm the numbers
match — that pair is the guard against a staged set that differed from what landed, and this rule's own
first invocation did exactly that (`aab418e`: `FILES=3 SCOPE_HITS=0` staged, identical from the object).

**Required: `SCOPE_HITS=0` AND `FILES>=1`.** `FILES=0` means nothing was staged — or the producer was
mis-run — and is a **FAIL, not a pass**: `SCOPE_HITS=0` over an empty file list is all-green-from-absent-input,
the failure § 6a caught in its own checker. The floor is reachable by accident in three ordinary ways (running
this *after* committing, running it after `git commit -a`, running it after a `git reset`), and by one
configuration: **`git config diff.relative` true makes `git show --name-only` print cwd-relative paths, so the
`sed -n 's|^server/basalt/||p'` matches nothing and the block prints `FILES=0`** — verified, not reasoned:
forcing `diff.relative=true` turns `aab418e` from `FILES=3` into `FILES=0`. Run from the project root, or
check the config. Without the floor that misconfiguration would read as a green. Four things this producer had
to be rebuilt over before it was believed, recorded so nobody re-breaks them: `git show --name-only` prints
**monorepo-root-relative** paths, so an unanchored-then-stripped `^run` matches nothing and a real launcher
commit scores `SCOPE_HITS=0` — verified: `b2e5fa4` "[T3][run] add ./run" scored 0 until the
`sed -n 's|^server/basalt/||p'` was added, then 1, which is a **fails-open** on precisely the class the rule
exists to catch; `PATTERNS` must name **paths that exist here**, and an earlier draft listed
`serve/config.py`, not a tracked path in this project at all (`git ls-files | grep -c '^serve/config'` → 0)
— the phantom-path trap of `pitfalls.md` item 15 inside the scope rule itself; `FILES=` counts files **inside
`server/basalt/`** and can be one less than `git show --stat` (`9a56c8d` is 14 there, 13 here, the difference
being the monorepo's own `context/pitfalls.md`); and **`vendor/` is deliberately absent from `PATTERNS`
because it is untracked, ignored** — `server/basalt/.gitignore` line 1 is `/vendor/`, `git check-ignore -v
vendor/basalt` resolves to it, and the repo has **zero** mode-`160000` entries (`git ls-files -s | awk
'$1==160000' | wc -l` → 0), so no pin state can arrive as a `vendor/basalt` path. The pin itself is the
tracked `BASALT_SHA` line in `versions.env`, which `PATTERNS` does match. That omission is only safe while
line 1 of `.gitignore` says `/vendor/` — and **`.gitignore` is itself in `PATTERNS` for exactly that reason**:
the realistic way `PATTERNS` becomes incomplete is not a planned submodule conversion, it is someone editing
the ignore file, which is a scope-surface definition and therefore scope-relevant. `^\.gitignore` was added to
`PATTERNS` on 2026-11-05 after the gap was found; before it, `1eafda7` ("[P1][T1][pins] add Basalt dc047a…",
which edits `.gitignore` *and* pins) scored `SCOPE_HITS=3` where it should score `4`, and `5f8890d` scored
`1` where it should score `2` — so the addition has firing controls, it is not decorative. Keeping the reason
for the omission in prose still matters: an unexplained gap in a pattern list is what a later agent
"helpfully" narrows. **Contingency: if `vendor/basalt` is ever converted to a submodule or checked-in gitlink,
`PATTERNS` must gain `vendor/`** — and that conversion is itself a class-3/4 change, so it cannot be made
under this carve-out.
Verified under this `PATTERNS`: the seven Plan-02 T2 commits (`9a56c8d` `93b6bd5` `7822c1e` `6dbb312`
`0c7517d` `d388d17` `aab418e`) all score `SCOPE_HITS=0`; `b2e5fa4` and `4ca31ae` score `1`, `5f8890d` `2`,
`1eafda7` `4`.

**A commit that touches the surface cannot use the carve-out — and that is by design, so here is the
remedy.** Now that `\.gitignore` is on the surface, a commit editing it — even a scratch-hygiene line no
build step reads — scores `SCOPE_HITS>=1`, fails (3), and must be written `BLOCKED` with the task left
`in_progress`. It fails closed deliberately; the answer is not to argue the edit is harmless. Two legitimate
routes: **land it in a commit the owner runs gate 3–6 for** (the owner is in the `docker` group, so no
carve-out is needed at all), or **split it out** of the docs commit and hand that commit to the owner. The
reason the remedy is written down rather than left implied: an unexplained forced `BLOCKED` is what an agent
under deadline routes around, and smuggling one `.gitignore` line into an unrelated docs commit to recover a
green is exactly the outcome a published surface list exists to make impossible.

**(4) Behaviour class, selected and bound to (3).** Name which of the four classes the task changes —
**3** vendor management/build preflight, **4** image build, **5** boot and `./run` output and flags,
**6** `serve.*` imports inside the image — or `none`. **`none` is legal only when (3) printed
`SCOPE_HITS=0`**, which is what makes this condition checkable: it is a selection from a list RULES already
defines, tied to another condition's machine output, rather than a claim about what a hypothetical run
would have revealed. Rule A.6 ("never mark a task `done` on unverified work") is not waived by writing
`none`; it is the reason `none` requires the (3) evidence.

Anything outside these four conditions is a **fail**, not a skip: write the row as `BLOCKED` and leave the
task `in_progress`. The carve-out exists because steps 3–6 are genuinely unrunnable here, not because their
absence is harmless; that is why it is a list of conditions with paste-able evidence rather than a
permission. First invocation was `d388d17`, whose scope check is the `SCOPE_HITS=0` line above.

**1. Syntax + counts.** Files are classified by **shebang first, extension second** — `bash -n`
happily passes a Python file, and launchers have no extension. Pass: no `FAIL` line, the counts
match the expected numbers, `env ok` (or the stated `SKIP` before `versions.env` exists), and the
`INTERPRETER=` line is pasted with the output. That last one is **evidence, not a threshold**: no
version is *required*, but a gate whose python half ran under an interpreter nobody recorded cannot be
re-derived, and the version recorded here is the only place a moving toolchain stays visible.
**The line's presence is human-checked and deliberately unguarded** — no `FAIL` fires if a paste drops
it, because a guard that reddens a commit over a missing informational line trades a real risk (silent
degradation) for the worse one this series documents (a red that must be explained, so a red that gets
explained away). Rationale and the two other limits in `context/audit/wave0/t3-gate-and-checks.txt` § 6;
if the line is ever used as a *threshold*, it needs a guard first.
**Expected counts — two sets, split on 2026-11-05 at Plan 02 T3** (decision recorded below):
**`3 b` product** (`lib/common.sh` from Plan 01 T1, `build` T2, `run` T3 — no `.py`, no `.json`) and
**`5 b` audit helpers** (`context/audit/scripts/ledger-grid.sh`, `t2-census.sh`,
`t3-model-file-poc.sh`, `t3-census.sh`, `t3-inputs.sh`). Bump these when a file is added or removed on
purpose — a count that silently drops is the failure this step exists for. *(Moved `3 b` → `5 b` on
2026-11-05 with Plan 02 T3's census generator and input projection; both were `shellcheck -x` clean
**before** `git add`, which is the only reason 2b had anything to check — see the note at 2b. This line
is the **owner** of both numbers: cite it, do not restate it, per § *Rule E* item 7.)*

> **Citation form, and what this line actually owns.** Cite it as
> **`RULES.md` § *Project gate* → Expected counts**. "Expected counts" is a **bold prose label under
> the heading `#### Project gate`**, not a heading; a bare `§ *The gate*` or `§ *Expected counts*`
> resolves to nothing in this file, and both forms were in circulation until 2026-11-05. The same
> applies to **Prerequisites** below it. *(Not re-headed on purpose: promoting either to a heading
> renumbers a section Plan 01's completed files cite.)*
> **This line is the owner of the two COUNTS, not of set membership.** Membership is derived: 1b selects
> by class **and** path over `git ls-files`, and no assertion checks `AUDIT_B=5` — the enumeration
> above is illustrative, and 2b's printed file list is the machine check on who is in the set. A sixth
> audit script moves the live count against this number, and that is the drift 1b's identity guard
> exists to surface.

> **The helper-clause decision, 2026-11-05 (T3).** The clause above — "a third script added to this
> count should not be a third use of the clause: it should trigger the alternative… narrowing step 1a
> to the project's own launchers and linting audit helpers in their own task-local check" — **is now
> taken.** T3 added `t3-model-file-poc.sh`, which would have been the third use of the helper clause
> and would have pushed a single count from `5 b` to `6 b`. So: 1b now reports the **product** set and
> the **audit-helper** set separately, step **2** lints only the product set, and step **2b** lints the
> audit generators in their own check. Why this is the right fork rather than a convenience: step 2's
> stated purpose is that *"the bash set is the whole product here"* — `run` and `build` are what boot a
> GPU server, and letting coverage-figure generators share that gate had already started to blur it
> (`t2-census.sh` and `ledger-grid.sh` emit numbers other plans cite, which is a different failure
> class from a launcher typo). Two consequences stated plainly, so neither reads as a loosening:
> **(i) both sets must be clean** — 2b is pass/fail like any other step, and an audit generator with a
> `FAIL` blocks its task; **(ii) 1a is deliberately *not* narrowed** — it still syntax-checks every
> tracked file, because dropping coverage to tidy a count is the wrong trade; 1a's scope and 1b/2's
> scope differ on purpose. Another audit script just moves `AUDIT_B` by one against the number that
> owner line carries, no clause needed. *(De-numbered 2026-11-05: this sentence used to hard-code "from
> 3 to 4", which was a second authority over the count directly above it and went stale the moment the
> count moved to 5. The hypothetical is the point; the digits were not.)*
>
> What the split did **not** do is catch anything, and the first version of this paragraph said
> otherwise. Step 2 before the split linted *every* tracked bash file, so a tracked `t3-census.sh`
> would have failed it exactly the same way. What caught that script's **9 shellcheck findings**
> (`shellcheck -x` exit 1, measured 2026-11-05, since linted clean) was running the linter on a file
> that was **not tracked yet**. That is the actual lesson, and it is a rule now: **lint a new script
> before you `git add` it**, because an untracked file is invisible to every step of this gate — 1a
> syntax, 1b counts and 2/2b all enumerate `git ls-files`. `t2-census.sh` and `ledger-grid.sh`, the two
> generators already tracked, are clean (`shellcheck -x` exit 0 each).

```bash
# 0 — name the interpreter this gate just ran. A prerequisite stated as an exact version in prose
#     fails closed when the environment moves; one the gate ECHOES cannot go stale silently. The
#     python halves of the audit generators (t3-census.sh's S14/S15 procedures) and every `python3 -c`
#     compile in 1a below run under this interpreter, so "which python3" is part of the record.
_py=$(python3 -c 'import sys;print(sys.version.split()[0])' 2>/dev/null || true)
_at=$(command -v python3 2>/dev/null || true)
echo "INTERPRETER bash=${BASH_VERSION:-MISSING} python=${_py:-MISSING} at=${_at:-MISSING}"
# 1a — syntax, over tracked files only (in-memory compile: no __pycache__ written).
#      NOT narrowed: every tracked file, both sets. See the decision note above.
git ls-files -z . | while IFS= read -r -d '' f; do
  case "$(head -n1 "$f" 2>/dev/null)" in   # 2>: a tracked symlink to a directory
    \#!*bash*|\#!*/sh) bash -n "$f" || echo "FAIL(bash) $f" ;;
    \#!*python*) python3 -c 'import sys; compile(open(sys.argv[1]).read(), sys.argv[1], "exec")' "$f" \
                   || echo "FAIL(py) $f" ;;
    *) case "$f" in
         *.sh)   bash -n "$f" || echo "FAIL(bash) $f" ;;
         *.py)   python3 -c 'import sys; compile(open(sys.argv[1]).read(), sys.argv[1], "exec")' "$f" \
                   || echo "FAIL(py) $f" ;;
         *.json) python3 -m json.tool "$f" >/dev/null || echo "FAIL(json) $f" ;;
       esac ;;
  esac
done
# 1b — the counts, so an empty file list can never read green. Two named sets: the audit
#      generators are excluded from PRODUCT_B and counted separately, per the decision above.
classify() { git ls-files -z . | while IFS= read -r -d '' f; do case "$(head -n1 "$f" 2>/dev/null)" in
  \#!*bash*|\#!*/sh) printf 'b\t%s\n' "$f"; continue ;; \#!*python*) printf 'p\t%s\n' "$f"; continue ;; esac
  case "$f" in *.sh) printf 'b\t%s\n' "$f" ;; *.py) printf 'p\t%s\n' "$f" ;; *.json) printf 'j\t%s\n' "$f" ;; esac
  done; }
TSV=/tmp/opencode/bashset.$$.tsv; AB=/tmp/opencode/auditb.$$.txt
classify > "$TSV"
# Each set is SELECTED by (class AND path). Never derive one set by subtracting a class-blind filter
# from a class-aware total: `AUDIT_B` used to filter on the path alone, so a `context/audit/scripts/gen.py`
# would have added 1 to AUDIT_B and 0 to TOTAL_B — PRODUCT_B silently under-counts (it can go
# negative), and the *audit* half goes falsely green, since 2b's set shrinks while its number does not.
grep $'^b\t' "$TSV" | cut -f2 | grep '^context/audit/scripts/' > "$AB"
PRODUCT_B=$(grep $'^b\t' "$TSV" | cut -f2 | grep -vc '^context/audit/scripts/')
AUDIT_B=$(grep -c . "$AB")
echo "PRODUCT_B=$PRODUCT_B AUDIT_B=$AUDIT_B TOTAL_B=$((PRODUCT_B+AUDIT_B))"
# IDENTITY GUARD. `git ls-files` prints **cwd-relative** paths, so run this block from
# context/audit/ and the anchor above matches nothing: AUDIT_B=0 and PRODUCT_B=3 — the expected
# product number, printed over zero launchers. Reproduced 2026-11-05, not reasoned: from
# context/audit/ the paths are `scripts/t2-census.sh`, from the project root `context/audit/scripts/…`.
# No count can detect that; only naming the files can. This is a locally-owned sentinel — do not DRY
# it into a pointer at the expected-count line, because a sentinel replaced by a citation cannot fail.
for f in run build lib/common.sh; do
  awk -F'\t' -v w="$f" '$2==w{g=1} END{exit !g}' "$TSV" || echo "FAIL(1b) missing product bash: $f"
done
# enumeration: the printed sets are the sets the two numbers count, per Rule A.4 "look at it"
echo '  product b:'; grep $'^b\t' "$TSV" | cut -f2 | grep -v '^context/audit/scripts/' | LC_ALL=C sort | sed 's|^|    |'
echo '  audit b:';   LC_ALL=C sort "$AB" | sed 's|^|    |'
grep $'^[pj]\t' "$TSV" | cut -f1 | sort | uniq -c
echo "CLASSIFIED=$(grep -c . "$TSV")"
rm -f "$TSV" "$AB"
# 1c — versions.env stays plain KEY=value: it is `source`d by the scripts and passed to
#      `docker build --build-arg`; a quoted value or trailing comment breaks one of the readers
if [ -f versions.env ]; then
  grep -nvE '^[A-Z][A-Z0-9_]*=[^[:space:]]*$|^[[:space:]]*#|^$' versions.env \
    && echo 'FAIL(env)' || echo 'env ok'
else echo 'SKIP(1c: no versions.env yet)'; fi
```

**2. shellcheck — product set only.** Pass: exit 0 and no output over the same bash set.
`shellcheck` 0.11.0 is
installed **user-local at `~/.local/bin/shellcheck`** (Plan 02 T0, 2026-11-05, official release
tarball sha256 `8c3be12b…`, verified by `--version` and by a negative control that had to fire
SC2115 — a tool's clean output is worthless until it has been shown to report something). The
`/usr/local/bin/shellcheck` this line recorded on 2026-11-04 **was gone** by 2026-11-05; see
`context/pitfalls.md`, "A gate prerequisite recorded in RULES.md can silently stop being true".
If it is missing the step is **fail**, not skip — the bash set is the whole product here.

**2b. shellcheck — audit generators.** Added 2026-11-05 (T3) with the 1b split; pass on the same
terms as step 2. These scripts publish coverage and census figures that later plans cite, so a lint
finding in one is a *correctness* signal, not cosmetics. What motivated the step is `t3-census.sh`:
9 findings at `shellcheck -x` exit 1 when it was about to be committed, which is why new scripts are
linted *before* `git add` — see the decision note above. (An earlier draft of this line blamed
`t2-census.sh` for arriving unlinted; that was false, it and `ledger-grid.sh` are exit 0, and the
sentence contradicted the decision note two pages back. Corrected 2026-11-05.)

```bash
# 2 — product bash set: run, build, lib/common.sh (audit generators go to 2b)
git ls-files -z . | while IFS= read -r -d '' f; do
  case "$f" in context/audit/scripts/*) continue ;; esac
  case "$(head -n1 "$f" 2>/dev/null)" in
    \#!*bash*|\#!*/sh) printf '%s\0' "$f" ;;
    *) case "$f" in *.sh) printf '%s\0' "$f" ;; esac ;;
  esac
done | xargs -0 -r shellcheck -x && echo 'shellcheck ok (product)'
# 2b — audit generators. The list is named and its size asserted, because `git ls-files <path>` is
# cwd-dependent: run this from a directory where context/audit/scripts is not a path and it enumerates
# NOTHING, xargs -r runs nothing, and the step prints "ok" having linted zero files. A guard that only
# counts would not notice; naming the files does.
mapfile -d '' AUD < <(git ls-files -z context/audit/scripts)
printf '  audit file: %s\n' "${AUD[@]}"
if [ "${#AUD[@]}" -ge 1 ]; then
  printf '%s\0' "${AUD[@]}" | xargs -0 -r shellcheck -x && echo 'shellcheck ok (audit)'
else
  echo "FAIL(2b) enumerated 0 audit scripts — wrong cwd? (running this with an empty list would print \"ok\")"
fi
```

**Three copies of the classifier exist** — 1a's `case`, 1b's `classify`, and step 2's loop — because each
gate step is a self-contained block meant to be pasted on its own, and hoisting one function into a
shared preamble would make step 2 unrunnable without step 1 first. That is a deliberate duplication and
it will drift if nobody watches: **when you change how a file is classified, change all three in the
same commit**, and say so in the message. The alternative (a sourced helper under `context/audit/`)
puts gate machinery outside the tracked launchers this gate exists to protect.

**3. Build preflight** (Plan 01 T2). Manages `vendor/basalt` (clones it on first use — network;
refuses a dirty checkout, untracked files included; checks out `BASALT_SHA` detached), resolves
both base digests, runs `docker build --check` (lint; the context is not sent, nothing is built)
and prints the build command. Pass: exit 0 and the last line `preflight ok`. Missing docker or
network is **fail**, not skip.

```bash
PREFLIGHT_ONLY=1 ./build 2>&1 | tail -1    # → preflight ok
```

**4. `./run` dry run** (Plan 01 T3). `DRY_RUN=1` runs every check `./run` makes and prints the
docker command; it starts nothing and creates nothing (a busy `:8700` is reported, not refused —
vLLM is the daily driver). The image is the newest one built for `BASALT_SHA`; before one exists
the step uses the stub `stub:none` and prints `image: stub:none` — say so in the tracker when it
did. Pass: no `FAIL` line, last line `dry-run ok`, and `storage/` was not created by it.

```bash
img=$(bash -c 'source lib/common.sh; load_pins; image_for_pin'); export IMAGE_OVERRIDE=${img:-stub:none}
d=$(DRY_RUN=1 ./run 2>&1); s=$(DRY_RUN=1 ./run --single 2>&1)
need() { local where=$1 out=$2 w; shift 2; for w; do [[ $out == *"$w"* ]] || echo "FAIL(dry:$where) missing: $w"; done; }
both=('-p 0.0.0.0:8700:8700' '--gpus device=0' '--ulimit memlock=-1' ':/models:ro' '--host 0.0.0.0'
      '--port 8700' '--alias cosmo-6000' '--allowed-host cosmo.lan' '--max-context 262144' '--kv int8'
      '--vision dynamic' 'serve /models/Qwen3.8-FN-NVFP4-DQ6_K.basalt' '(DRY_RUN) nothing started')
need default "$d" "${both[@]}" '--parallel 4' '--slot-full-context'
need single "$s" "${both[@]}" '--parallel 1'
[[ $s == *--slot-full-context* ]] && echo 'FAIL(dry:single) has --slot-full-context'
n=$(DRY_RUN=1 ./run /tmp/x.ninfer 2>&1) && echo 'FAIL(dry:ninfer) exit 0'
[[ $n == *../ninfer* ]] || echo 'FAIL(dry:ninfer) no ../ninfer pointer'
DRY_RUN=1 ./run /nonexistent/x.basalt >/dev/null 2>&1 && echo 'FAIL(dry:missing) exit 0'
DRY_RUN=1 ./run README.md >/dev/null 2>&1 && echo 'FAIL(dry:not-a-pack) exit 0'
k=$(API_KEY=k3y DRY_RUN=1 ./run --single 2>&1); [[ $k == *k3y* ]] && echo 'FAIL(dry:api-key) printed'
echo "dry-run ok (image: $IMAGE_OVERRIDE)"; unset IMAGE_OVERRIDE
```

**5. Flags vs the built image** (Plan 01 T5). Every `--flag` the two dry runs pass to `serve` must
be one the built image accepts, decided by the image's own server code — the check Basalt makes at
startup: a server option from `serve.config.build_parser()`, or an engine flag found in
`bin/basalt` by `serve.config.engine_flags_of` (the server stops at start on any other name). No
GPU, `--network none`. Pass: one line `flags ok (…)`. No image for `BASALT_SHA` is **fail**.
Negative control (run it when this step changes): `EXTRA_ARGS=--no-such-flag` → `FAIL(flags)`.

```bash
img=$(bash -c 'source lib/common.sh; load_pins; image_for_pin')
if [[ -z $img ]]; then echo 'FAIL(flags) no image for BASALT_SHA — run ./build'; else
mapfile -t flags < <({ DRY_RUN=1 ./run; DRY_RUN=1 ./run --single; } 2>/dev/null \
  | grep -o ' serve /models/.*' | tr ' ' '\n' | grep -E '^--[a-z]' | sort -u)
docker run --rm --network none -w /opt/basalt --entrypoint /opt/basalt/.venv/bin/python "$img" -c '
import sys
from serve.config import build_parser, server_options, engine_flags_of
srv = set(server_options(build_parser())); eng = engine_flags_of("/opt/basalt/bin/basalt") or set()
fl = sys.argv[1:]; bad = [f for f in fl if f[2:] not in srv and f not in eng]
ns = sum(f[2:] in srv for f in fl)
print("\n".join("FAIL(flags) " + f for f in bad) if bad or not fl else
      f"flags ok ({len(fl) - ns} engine, {ns} server: {len(eng)} engine flags known)")
if not fl: print("FAIL(flags) ./run emitted no flags")' "${flags[@]}"
fi
```

**6. Server imports in the image** (Plan 01 T6). Imports every non-test `serve.*` module with the
image's venv, from `/opt/basalt`: a module that needs a file the image does not ship fails here
instead of at boot (`serve/engine/modelfile.py` imports `tools/model_file.py` through `sys.path` —
the 2026-11-04 boot failure, `context/pitfalls.md`). No GPU, `--network none`. Pass: one line
`imports ok (N serve modules)`. No image for `BASALT_SHA` is **fail**.

```bash
img=$(bash -c 'source lib/common.sh; load_pins; image_for_pin')
if [[ -z $img ]]; then echo 'FAIL(imports) no image for BASALT_SHA — run ./build'; else
docker run --rm --network none -w /opt/basalt --entrypoint /opt/basalt/.venv/bin/python "$img" -c '
import importlib, pkgutil, serve
names = [m.name for m in pkgutil.walk_packages(serve.__path__, "serve.") if ".tests" not in m.name]
bad = []
for n in names:
    try: importlib.import_module(n)
    except Exception as e: bad.append(f"FAIL(imports) {n}: {type(e).__name__}: {e}")
print("\n".join(bad) if bad else f"imports ok ({len(names)} serve modules)")'
fi
```

**Blind spots.** Steps 1–6 never boot. They cannot see a code path only a loaded model reaches, a
container that cannot reach the GPU or lock memory, a pack the engine refuses, or a port that is
already taken. Step 5 checks flag *names*, not values: a value Basalt rejects (`--kv fp8`) still
shows only at boot.

**Build actually loaded.** Behavior is verified on a boot the owner runs (SERIES *Standing
Constraints*): the boot log must name the Basalt commit and the pack `./run` was asked for —
`./run` prints the image tag and pack path before it starts, and the record in the tracker quotes
both. A boot of the wrong image or pack passes every check above.

**Look at it.** `./run` output, the boot log and one real `/v1/chat/completions` reply on `:8700`
are read, not inferred, before a launcher task is `done`.

**Prerequisites:** `bash`, `git`, `python3`, `shellcheck` 0.11.0 (`~/.local/bin`,
2026-11-05). **Every version here is a minimum, not a pin, and each is a reading of one named
shell** — no gate step enforces an exact version, and an unenforced exact prerequisite is what fails
closed later; **re-check in the shell that will use the answer**, which is what `distilled.md`
§ *This host: one sm_120 card, 125 GiB RAM* says and where the dated host inventory lives. Measured
2026-11-05 **in the agent's shell** (`python3 -VV`, `command -v python3`, `pyenv versions`):
`python3 3.12.16` at `~/.pyenv/versions/3.12.16/bin/python3`, pyenv `3.12.16` active via
`~/.pyenv/version`. Two other interpreters live on this box and answer different questions:
`/usr/bin/python3` is **3.15.0**, and the container image's is **3.12.3** (`distilled.md`
§ *Image build inputs: … the runtime base has no Python*). *(This line read `python3 (3.14.10 here)`
until 2026-11-05. It named no environment, so it asserted universality, and this shell contradicts it —
`3.14.10` is a real CPython release (python.org, 2026-10-25), so it may have been a true reading of an
unspecifiable shell; it is not re-derivable, so it is dropped from the requirement rather than argued
about. `pitfalls.md` § *A dated fact scoped to a shell I could not enter…* is the trap; the
`INTERPRETER=` echo added to step 1 is the structural cure.)*
Later steps add `docker` (**client** 29.9.2 measured here 2026-11-05 by `docker --version`; the
**daemon** is unverifiable from an agent session — socket owner-only — and `29.8.1` is the owner's
2026-11-04 reading), used as yourself — no `sudo`, which needs the
user in the `docker` group: **as of 2026-11-05 an agent session here is not** (`uid=1000
groups=1000,27`, socket `root:rtkit 660`), so steps 3–6 are **owner-run** — the agent writes the
command and records the pasted output, and a commit names the gate steps it could not run.

**Init baseline (2026-11-04):** step 1 pass over 0 tracked files (no counts printed), 1c
`SKIP(1c: no versions.env yet)` — not applicable until a task adds the pin file; step 2
`shellcheck ok` over an empty set. No files generated. *(Currency note 2026-11-05: this baseline predates
the `INTERPRETER=` echo now in step 1, so it carries no interpreter line; the same absence in any
pre-2026-11-05 capture means **not captured**, never **not required**.)*

### Rule B — Commit at every task boundary (or more often) <!-- hp:commit -->

**YOU MUST COMMIT BEFORE MARKING ANY TASK COMPLETE.** If you haven't committed, you haven't
finished.

1. Commit at the end of **every** task. Smaller intermediate commits are welcome. **Never wait
   for the plan to finish.**
2. One task per commit — do not batch tasks unless they are truly inseparable.
3. Message format: `[basalt][P<n>][T<n>][<topic>] <imperative summary>` — the sub-project scope
   (plan numbers repeat across `server/*` sub-projects), plan number (not zero-padded), task
   number, short area tag. Outside a plan: `basalt: <summary>`. Plan bookkeeping replaces the task
   tag: `[basalt][P<n>][plan] add <title>`, `[basalt][P<n>][revise] …`,
   `[basalt][P<n>][close] <title>: <outcome>`.
   - `[basalt][P1][T3][run] add ./run launcher for Flash-Next on :8700`
   - `[basalt][P1][T5][boot] record the owner boot of the DQ6_K pack`
4. **Tracker row in the same commit.** The row update — and any `context/` update the task
   produced — lands with the work. Stage explicit paths: `git add <paths>`, never `git add -A`.
5. The full project gate (Rule A) passes before every commit.
6. A change in observable behavior updates `README.md` in the same commit.
7. **Never push** — the human pushes after review. **No co-author trailers** unless asked.
8. Every task in a plan carries its `**Commit**:` line, so the reminder is baked in.

### Rule B2 — Git history is append-only <!-- hp:append-only -->

**Never rewrite a commit — not even the one you just made.** Banned unless the human explicitly
asks, in that moment:

| Banned | Why |
|---|---|
| `git commit --amend` | replaces a commit that may already be public |
| `git rebase` (any form) | rewrites every commit it touches |
| `git reset --hard`, any reset that drops a commit | discards history |
| `git push --force` / `--force-with-lease` | forces the rewrite onto everyone else |
| `git revert` | corrections are hand-written, not machine-generated |
| `git stash`, `git checkout -- <path>`, `git restore`, `git clean` on work you did not make | destroys someone else's uncommitted work |

1. A mistake in a commit — wrong content, wrong message, a claim that turned out false — is
   fixed by a **new commit** that says what was wrong and corrects it.
2. This holds **even when the commit looks unpushed.** Push state changes without you seeing it.
3. **Chain edit-then-commit with `&&`** so a failed edit can never be followed by a commit that
   claims it worked: `edit_files.sh && git commit -m "…"`.
4. **A commit message is a factual claim about the tree.** If it says a file was updated,
   re-read that file before writing the message.
5. **Append-only protects claims about the world, not scaffolding about the document.** A false
   assertion — a wrong figure, a transcript line a command never printed, a rule stated too flatly —
   can only be answered by a correction commit, because the assertion is the evidence. A broken
   **instrument** — a locator, a check, a scheme for naming rows — gets **deleted**, not annotated in
   perpetuity: keeping it with a warning attached leaves a thing nobody can trust and cannot remove.
   Delete the instrument and record what killed it as a measured case in `context/pitfalls.md`. Tell
   the two apart before you reach for a correction commit; mistaking the second for the first is how
   an artifact accretes defensive prose until it buries its own payload
   (`context/pitfalls.md` item 18).

> **Why this rule exists** (the incident behind it, in the project this skill came from,
> 2026-07-18): a scripted edit hit an assertion and wrote nothing, but the unchained `git commit`
> on the next line ran anyway — a commit whose message claimed updates it did not contain. It had
> already been pushed; "fixing" it with `--amend` diverged `main` from `origin/main` and forced a
> force-push. A follow-up commit would have cost nothing.


### Rule C — Plan lifecycle: `completed/` and `abandoned/` <!-- hp:lifecycle -->

1. When a plan and its tracker reach 100% (every row `done`, `skipped` or `invalid` with a
   reason; checklist ticked against evidence), move them **immediately**, in the same commit as
   the SERIES update, and fix links that pointed at the old path:
   ```bash
   mkdir -p context/plans/completed
   git mv context/plans/NN_name.md context/plans/completed/NN_name.md
   git mv context/plans/NN_name_tracker.md context/plans/completed/NN_name_tracker.md
   ```
2. **A partially complete plan does not move.**
3. A plan dropped before completion moves to `abandoned/` **with its reason recorded in
   SERIES.md**. A plan dropped without a stated reason gets re-attempted six months later by
   someone who doesn't know it failed.
4. Before starting a plan, every other `in-progress` plan must be listed in SERIES under
   *Currently Active*, or marked `blocked`/`deferred` there with a reason.
5. **A pending plan is a hypothesis, not a contract.** Before starting one, re-read it against
   what has landed since it was written; if anything it relies on changed, update it and add a
   `Revised` line.

### Rule D — Harvest the knowledge before you close the plan <!-- hp:harvest -->

A plan is not done when the code works. It is done when what you learned is on disk.

1. Record findings **as you go**, in the same commit as the task that found them. Where each
   kind of finding goes is defined in `context/AGENTS.md`.
2. Anything that took more than one attempt to understand or fix (not a formatter re-run)
   becomes a pitfall entry.
3. At close, check that every finding the tracker mentions actually landed.
4. **Never claim a finding is recorded unless the bytes are in the file.**

### Rule E — Evidence over assertion <!-- hp:evidence -->

1. **Reality is the oracle** — the running system, the upstream source, the measured output. A
   test that asserts our own values proves nothing about correctness.
2. **A plan premise is a claim.** When reality contradicts the plan: record the measurement, do
   what reality does, and mark the plan item `invalid` as written — in the tracker and in the
   commit body. Do not bend the code to the plan.
3. **Below the bar is a row, not a silence.** Anything left unfinished, unverified or
   disagreeing gets a Follow-ups row in the tracker, naming this plan or a later one.
4. "Inconclusive" is a valid, recordable outcome. Never round it up to a win.
5. **Cite the source of a fact by the exact command that showed it**, or by `path:line` for code. In prose
   this project keeps editing (plans, artifacts, knowledge pages) cite the **section name, never a line
   number** — a line citation rots on the next edit. The same holds for a `git log -S<anchor>` locator: it
   resolves only while the anchor string survives verbatim, and it can degrade with **no rename at all**,
   because `-S` fires on any change in the phrase's **occurrence count** — deleting an unrelated line that
   happened to quote the anchor is enough. So **prefer an identifier you already have** (a SHA, a path, a
   section name) over a locator, and never quote verbatim the literal whose count or match-set a check
   reports: that is editing the world to fix the instrument. Two measured cases, including one that broke
   one commit after being documented, are `context/pitfalls.md` item 18.
6. **A measurement taken over an explicit path list must carry its own total.** A path in
   `git diff --numstat -- <paths>` (or any `-- <paths>` form) that matches nothing contributes **nothing and
   reports nothing**, so a hand-assembled list silently under-counts: print the same sum with **no** pathspec
   beside it, or assert `paths given == paths matched`. And when a figure is retired, **sweep the section, do
   not grep the value** — ask of each numeral which command reproduces it at this commit, since the stale
   figure that survives a correction is usually one that was never wrong, only unreferenced. Both failure
   modes are recorded in `audit/wave0/t2-gate-and-checks.txt` § 7.
7. **A project-wide rule has one owner file.** Restate it elsewhere as a citation plus the named consequence
   for *that* file's reader; do not restate the prohibition itself. The reason is a failure mode, not tidiness:
   a shortened restatement tends to lose its qualifier, and **a ban that is too broad suppresses work instead
   of corrupting it** — it makes a later task skip a check it was required to run, so it fires no control,
   fails no test and never self-reports. (Observed 2026-11-05, T2: "never compare SHAs between these two
   repositories" was flat in two of three copies and contradicted the blob-SHA / `patch-id --stable` join that
   T2b and U12 require. Owner: `audit/index.md` § *If you are about to run something against upstream*, routed
   from `context/AGENTS.md`.) Corollary on hunting such copies: **grep for the claim, not the phrasing** — the
   third instance read "SHA comparison … is meaningless", which no `never compare` pattern would have found.
   The same applies to a **number a record copies**: cite the owner for an expected value rather than restating
   it, because a restated count becomes a second authority the moment the number legitimately moves, and an
   agent who sees it disagree with the live tool concludes drift rather than staleness. (Observed 2026-11-05:
   four places held `3 b` after the count moved to `5 b`: three live instructions, each fixed by citing the
   owner rather than restating the number, plus one stamped capture, annotated forward in its own notes section
   and never rewritten — rewriting it would retro-date the helpers into the task that captured it.)
   **Boundary: the owner is a named file**, so this applies only where the value's owner sits outside the file
   holding the copy. A check's own locally-owned expectation — § 6a's `PATHS=10`, a section's own `SCOPE_HITS`
   — stays a literal, and citation would delete it: a sentinel you replaced with a pointer can no longer fail,
   which is the opposite mistake and a worse one.

---

## Project Rules <!-- hp:project-rules -->

Project-specific rules, numbered **P1, P2, …**. They may tighten the core rules above, never
loosen them. This section belongs to the project; `hands-plan resync` never rewrites it.

- **P1 — One worktree, two writers: the agent keeps the tree clean.** The owner runs the GPU, docker
  and boot steps **in this same worktree**, so an agent's uncommitted files are always at risk of being
  swept into his commit (done to us on 2026-11-05 by `b308244` — *corrected 2026-11-04 PST: that
  commit was the host agent session's, not the owner's (`0b1efb6`); the rule holds for any second
  writer, agent or human*; see
  `context/pitfalls.md` § *A shared worktree has no such thing as "my uncommitted edits are private"*).
  Therefore: (a) an agent stages **explicit paths** the moment a logical unit finishes —
  `git add <paths>`, never `-A`/`-a` — and never ends a turn with `context/` or plan files dirty while
  the owner is live; (b) every handoff ends with a `dirty: <paths> (mine, in progress)` line naming
  whatever is still uncommitted; (c) before editing a plan, tracker or `context/` file, re-read the
  **current** text rather than trusting what the agent last wrote, because a collision is invisible in
  git and visible only as a sentence that contradicts itself; (d) after a collision, **fix forward** —
  HEAD is ground truth, the other writer's verdict and measurements stay verbatim, and disagreement is
  added as a dated, attributed note. Rule B2's bans (`--amend`, `revert`, `restore`, `stash`, `clean`
  on work you did not make) make every undo-based "fix" worse than the collision.

---

## Series-Scoped Rules <!-- hp:series-rules -->

When several plans share a decision rule (how to reuse code across components, a format both
sides must agree on, an extraction strategy), write it **once** as `context/plans/<TOPIC>_RULES.md`
naming the plan range it governs, link it from each of those plans' headers and from SERIES, and
retire it when the series closes.

Red flag in any plan: **"copy from X" / "port from X" without saying whether the code is
extracted to a shared home, kept where it is, or used only as a reference.** Fix the plan first.

---

## Content Style <!-- hp:style -->

- **Bold** for important terms; `code` for file names, identifiers and commands.
- Dates are ISO `YYYY-MM-DD`, taken from `date +%F` — never guessed.
- Code blocks always carry a language (` ```bash `, ` ```bash `).
- Cross-reference plans as "Plan N" or "Plan N T2"; cite sources as `path/file:line`.
- Dense, no fluff — but never so compressed a detail is lost.

---

## Templates & History <!-- hp:templates -->

- New plan: copy `NN_example.md` → `NN_name.md` and `NN_example_tracker.md` →
  `NN_name_tracker.md` (drop their first-line `hands-plan` stamp), take the number from SERIES,
  fill every section, add the SERIES row, and bump **Next free plan number**.
- For real examples of the live format, browse `context/plans/completed/` — newest first.
