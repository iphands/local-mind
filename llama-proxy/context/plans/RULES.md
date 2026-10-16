<!-- hands-plan:v1 2026-10-16 -->
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
| A new or changed stats line / output format | **Yes.** It changes human-readable output that clients may parse; verify by eye and by the gate. |
| A fix to a metrics calculation (prefill tps) | **Yes.** A wrong number is a silent failure; verify against a reproduction. |
| A new backend route or endpoint | **Yes.** It changes request routing; verify the new endpoint responds and existing ones are untouched. |
| A change to metrics export (InfluxDB) | **Yes.** It changes exported fields; verify against the exporter. |

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

After every task, in order:

1. **Compile:** `cargo check --workspace --features influxdb -- -D warnings` → exit 0, zero warnings.
   (`cargo check --workspace` on default features also passes at init.)
2. **Lint:** `cargo clippy --all-targets --features influxdb -- -D warnings` → exit 0.
3. **Format:** `cargo fmt --check` → clean (no diff).
4. **Test (both feature states):** `cargo test --workspace --lib` → exit 0, **and**
   `cargo test --workspace --lib --features influxdb` → exit 0. Not redundant: the exporter's
   `feature_off` tests compile ONLY in the first, and `wire_tests`/`queue_tests`/`url_gate_tests`/
   `feature_on` ONLY in the second. Step 2 *compiles* the feature-on set but never runs it, so a
   single test line leaves half the matrix unexercised — see `context/pitfalls.md`, Toolchain.

Baseline at init (2026-10-16): step 1 (check) PASSES; steps 2–4 (clippy, fmt, test) FAIL on
pre-existing issues, including a formatting diff in `src/stats/formatter.rs` — the file this
work touches. These failures are recorded here, not hidden; the first plan's opening task is to
make the gate pass (or record them as a standing constraint). The gate must be green before any
task is marked `done`.

Blind spots: the gate cannot see a wrong stat value or a changed prefill tps — those need the
behavioral check in each task's Verify (run the proxy, read the stats line).

### Rule B — Commit at every task boundary (or more often) <!-- hp:commit -->

**YOU MUST COMMIT BEFORE MARKING ANY TASK COMPLETE.** If you haven't committed, you haven't
finished.

1. Commit at the end of **every** task. Smaller intermediate commits are welcome. **Never wait
   for the plan to finish.**
2. One task per commit — do not batch tasks unless they are truly inseparable.
3. Message format:
   `[proxy][P<n>][T<n>][<topic>] <imperative summary>` — plan number (not zero-padded),
   task number, short area tag. Outside a plan: `[proxy] <summary>`. Plan bookkeeping replaces
   the task tag: `[proxy][P<n>][plan] add <title>`,
   `[proxy][P<n>][revise] …`, `[proxy][P<n>][close] <title>: <outcome>`.
   In both forms the leading `[proxy]` is the project scope tag.
   - `[proxy][P1][T1][stats] reorder and pad the stats line`
   - `[proxy] scaffold context/plans + context/ knowledge base (hands-plan v1)`
4. **Tracker row in the same commit.** The row update — and any `context/` update the task
   produced — lands with the work. Stage explicit paths: `git add <paths>`, never `git add -A`.
5. The full project gate (Rule A) passes before every commit.
6. A change in observable behavior updates `README.md` in the same commit.
7. **Never push** — the human pushes after review. **No co-author trailers** unless asked.
8. Every task in a plan carries its `**Commit**:` line, so the reminder is baked in.

#### Project addendum

The commit scope tag for this project is **`[proxy]`** — never `[llama-proxy]`, which is the
directory name, not a commit tag. `[proxy]` is not a rename: it is what this project has used since
the commit that created it (`[proxy] initial commit`, 2026-02-15) and what the hands-plan scaffold
itself used. `[llama-proxy]` appears only in the Plan 01/02 commits — that was the drift, and this
rule corrects it.

Every commit example in this file and in the `NN_example` skeletons uses `[proxy]`. If a
`hands-plan` re-sync re-stamps a different tag into the Rule B body, `[proxy]` wins and those
examples are stale: fix them, don't follow them.

Match history by tag, not by date: the Plan 01/02 subjects still read `[llama-proxy][P…]` and were
left untouched, history being append-only (Rule B2). So `git log --grep='\[proxy\]\[P1\]'` finds
nothing for Plan 01 — search it by summary text or SHA. The dated provenance blockquote under the
header of each `plans/completed/` plan is sanctioned: leave it, and don't strip it as a Plan Format
deviation.

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
5. Cite the source of a fact as `path:line` or the exact command that showed it.

---

## Project Rules <!-- hp:project-rules -->

Project-specific rules, numbered **P1, P2, …**. They may tighten the core rules above, never
loosen them. This section belongs to the project; `hands-plan resync` never rewrites it.

### P1 — Every number carries provenance and variance

This project's numbers are its product. An unsourced number is worse than no number, because it
gets cited.

1. **N ≥ 3 runs.** Report mean and spread. One run is an anecdote.
2. **A delta inside the spread is "within noise"** — write those words. Never report it as a win,
   a regression, or a trend.
3. **Name the source**: input/capture, build (git SHA or version string), environment and
   settings. It goes in the tracker's Evidence table.
4. **Change one thing at a time.** A different build *and* different settings in one comparison
   measures nothing.
5. **Check the environment first** (throttling, background load, warm vs cold caches). A run under
   a bad state is re-measured, not caveated.
6. **Inconclusive is a valid, recordable outcome.** Record it. Do not round it up.

### P2 — Capture hygiene

1. Captures, traces and raw result files go in `captures/` (or the project's equivalent) and are
   **gitignored** — they are large and regenerable.
2. Name them so they are identifiable six months later: `<workload>_<YYYY-MM-DD>_<what-varies>.<ext>`.
3. The tracker's Evidence table says which capture backs which number.
4. **Restore the system after a session** — sysctls, env vars, swapped drivers/binaries, debug
   flags. The next session's baseline depends on a known state.

### P3 — Vendor sources are read-only discovery space

`vendor/vllm` and `vendor/llama.cpp` (paths **relative to the `llama-proxy/` project root** — not
the `../vendor/…` that root `CLAUDE.md` still references, which does not exist) are upstream
checkouts dropped in to learn how the backends actually behave. They are **read-only**: never edit,
never `git add`, never commit.

**State, verified 2026-10-16 (Plan 02 T3, `55a6bf3`):** the tree is **gitignored** via `/vendor` in
`llama-proxy/.gitignore` (proof in the Plan 02 tracker row; `git ls-files vendor/` is `0`, so the
ignore hides nothing tracked). Being ignored changed what the outer git *sees*, not what you may *do*
— read-only still binds, and the trees are real checkouts (each keeps its own `.git`):

1. **Never `git add` it.** A plain `git add -A` now skips the tree, but that is not a safety net:
   `git add <path>` only *refuses* (re-exposing takes `-f`), and `git stash -a` sweeps it. Stage
   explicit paths (Rule B4), never into `vendor/`.
2. **Never `git clean -fdx` from the repo root** — `-x` deletes exactly the ignored tree and the outer
   repo's history cannot bring it back. Clean with `git clean -fdx -e vendor`.
3. **An edit is invisible to *this* repo, not to the checkout.** Each tree keeps its own `.git`, so
   there is always something to revert — just with the inner repo: `git -C vendor/<tree> status` /
   `git -C vendor/<tree> checkout -- .` (the outer `git checkout -- vendor/…` errors: pathspec
   unmatched). Never edit anyway — cite, don't patch.
4. **Cite them like any source**: `vendor/vllm/<path>:<line>`, naming the pinned build (Rule P1) via
   `git -C vendor/<tree> rev-parse HEAD`.
5. **A zero-hit search is not evidence of absence.** `--no-ignore` is now **required**, not optional:
   `rg --no-ignore --hidden -n '<symbol>' vendor/vllm/ -g '!**/build/**'`. `--no-ignore` covers the
   ignore files, `--hidden` covers dot-paths (`.cache/`, `.github/`), and the `-g` stops a stale local
   build faking a hit upstream never shipped — distrust a lone *hit* as much as a zero.
6. A backend-behavior fact learned here goes in `context/distilled.md` tagged `[REPO]` with its
   `path:line` (see `context/AGENTS.md`, "Where Findings Go").

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
- Code blocks always carry a language (`` ```rust ``, ` ```bash `).
- Cross-reference plans as "Plan N" or "Plan N T2"; cite sources as `path/file:line`.
- Dense, no fluff — but never so compressed a detail is lost.

---

## Templates & History <!-- hp:templates -->

- New plan: copy `NN_example.md` → `NN_name.md` and `NN_example_tracker.md` →
  `NN_name_tracker.md` (drop their first-line `hands-plan` stamp), take the number from SERIES,
  fill every section, add the SERIES row, and bump **Next free plan number**.
- For real examples of the live format, browse `context/plans/completed/` — newest first.
