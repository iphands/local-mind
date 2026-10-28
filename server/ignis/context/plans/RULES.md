<!-- hands-plan:v1 2026-10-27 -->
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
| A pin in `versions.env` (the Ignis image tag or digest, a weights repo or revision) | **Yes.** An Ignis bump can change a built-in profile's defaults, rename a flag or move the artifact format (`-v2.ninfer` today); a weights bump is a 68 GiB re-download the owner has to run. |
| A launcher (`qwen38fn/run`, the 27B launcher), a shared `lib/` helper, or a new launcher | **Yes.** A wrong flag or profile is a server that boots wrong or silently slower, and nothing fails until a bench says so. |
| The container's exposure: the published port, the bind address, `--security-opt` / `--privileged` / capabilities / `--ipc` / memlock, or the served model names | **Yes.** `:8700` and the aliases are what `llama-proxy` routes to; loosening the sandbox widens a server bound to `0.0.0.0`. |
| Anything that mutates the host outside git: `nvidia-smi -pl/-lgc`, downloading, moving or deleting an artifact in the shared models tree, chowning a cache | **Yes.** Unlintable and uncommitted — the plan is the only record that it happened. |

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
(required — this project's product is a number, see P1), **Notes / Deviations**,
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

There is **no compiler here and no CI**. This project is bash launchers, a pin file and a README
that drive an inference engine it does not contain (Ignis ships as an upstream image; the Rust
and CUDA live there). A launcher typo is a server that never boots; a flag Ignis renamed is a
boot that refuses or a server that silently runs a slower default. So the per-commit gate is
mechanical, needs **no GPU, no model load and no image pull** (decision 2026-10-27: nothing in the
per-commit gate may need a model loaded), and runs before every commit, from the project root
(`server/ignis`).

Every step reports **pass / fail / skip**, and **skip is not pass**: no task is `done` on a run
where an applicable step was skipped.

**1. Syntax + counts.** Files are classified by **shebang first, extension second** — `bash -n`
happily passes a Python file, and the launchers have no extension. Pass: no `FAIL` line and the
counts below match the expected numbers. **Expected counts: `3 b`** (Plan 01 T1:
`lib/common.sh`; T2: `qwen38fn/run`, `scripts/launcher-dryrun`). Bump the expected numbers in this paragraph when a file is added
or removed on purpose — a count that silently drops is the failure this step exists for.

```bash
# 1a — syntax, over tracked files only (in-memory compile: no __pycache__ written)
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
# 1b — the counts, so an empty file list can never read green
git ls-files -z . | while IFS= read -r -d '' f; do case "$(head -n1 "$f" 2>/dev/null)" in
  \#!*bash*|\#!*/sh) echo b; continue ;; \#!*python*) echo p; continue ;; esac
  case "$f" in *.sh) echo b ;; *.py) echo p ;; *.json) echo j ;; esac; done | sort | uniq -c
```

**2. shellcheck.** Pass: exit 0 and no output over the same bash set. `shellcheck` 0.11.0 is
installed (`/usr/local/bin/shellcheck`, 2026-10-27); if it is missing the step is **fail**, not
skip — the bash set is the whole product here.

```bash
git ls-files -z . | while IFS= read -r -d '' f; do
  case "$(head -n1 "$f" 2>/dev/null)" in
    \#!*bash*|\#!*/sh) printf '%s\0' "$f" ;;
    *) case "$f" in *.sh) printf '%s\0' "$f" ;; esac ;;
  esac
done | xargs -0 -r shellcheck -x && echo 'shellcheck ok'
```

**3. Pin file stays plain.** `versions.env` is `source`d by the launchers, so it holds plain
`KEY=value` lines only — no quotes, no spaces, no trailing comments. Pass: `env ok`, or the stated
`SKIP` before the file exists.

```bash
if [ -f versions.env ]; then
  grep -nvE '^[A-Z][A-Z0-9_]*=[^[:space:]]*$|^[[:space:]]*#|^$' versions.env \
    && echo 'FAIL(env)' || echo 'env ok'
else echo 'SKIP(3: no versions.env yet)'; fi
```

**4. Launcher dry-run** (Plan 01 T2). `scripts/launcher-dryrun` runs every launcher against a
**stub `docker`** and a **stub `nvidia-smi`** in a `mktemp` sandbox (empty fake artifacts, a
sandbox cache dir), asserts the `docker run` argv each one would execute, and removes the sandbox:
no daemon call, no GPU, no image, nothing left in the tree. Pass: exit 0 and the last line is
`dryrun ok`; any `FAIL` line fails the step. Per launcher it asserts the shared flags (`--rm --name
ignis --user 1000:1000 -e HOME=/cache --gpus device=0 -p 0.0.0.0:8700:8000`, the `:ro` models
mount, the pinned image by digest, `--profile rtxpro6000 --model-id cosmo-6000 --download-enabled
false --switch-allow-implicit false --model-idle-unload 0 --model-max-context 262144`, the cache
paths, no `--init`/`--memory`), the launcher's artifact, the knobs (`SPEC_TOKENS`, `LANES`,
`CONTEXT`, `PORT`, `GPU`, `DETACH`, `EXTRA_ARGS` last), the refusals (`DOCKER` naming sudo as a
word and as a path, a busy `:8700`, the RTX 4060, a missing artifact — with the `hf download`
hint), and `PRINT_CONFIG=1` (`config print` with the same field flags, no port, no GPU, accepted
while `:8700` is busy). Mutation-checked at Plan 01 T2 (2026-10-27): dropping
`--switch-allow-implicit false`, the busy refusal or the GPU check, or moving `EXTRA_ARGS` off the
end, each turns the run red.

```bash
./scripts/launcher-dryrun
```

**Runtime tier — NOT part of the per-commit gate.** Pulling the image, downloading weights,
booting a model, a smoke request and a bench happen only when a plan task names them, and **the
owner runs them** (decision 2026-10-27). When one runs: assert the server on `:8700` is the one
under test first (`docker ps --filter publish=8700 --format '{{.Names}} {{.Image}}'`), and record
numbers per P1.

**What this gate cannot see.** Steps 1–4 catch syntax and the shape of the command line, not
behavior. They cannot see a flag the pinned image no longer accepts, a profile that picked the
wrong card, an expert cache that spilled to host RAM, an artifact that is the wrong file, or a
server that is slow. A plan that adds or changes a flag cites the pinned upstream source or docs
for it (`path:line` at the pinned tag) — a read, not a boot. Never mark a launcher task `done` on
steps 1–4 alone if its behavior is runtime: say in the tracker that the runtime tier did not run.

**Prerequisites.** `shellcheck`, `python3`, `git`. `docker` reachable as yourself (no `sudo`;
the socket is `root:docker 660`, 2026-10-27) is needed for the runtime tier only. GPU and weights:
runtime tier only.

**Baseline at init (2026-10-27, branch `dev`, empty project):** step 1 **pass** vacuously (no
tracked files, no counts printed); step 2 **pass** vacuously (`shellcheck ok` over an empty set);
step 3 `SKIP(3: no versions.env yet)` — not applicable yet. Step 4 does not exist yet. The
baseline generated **no files**.

### Rule B — Commit at every task boundary (or more often) <!-- hp:commit -->

**YOU MUST COMMIT BEFORE MARKING ANY TASK COMPLETE.** If you haven't committed, you haven't
finished.

1. Commit at the end of **every** task. Smaller intermediate commits are welcome. **Never wait
   for the plan to finish.**
2. One task per commit — do not batch tasks unless they are truly inseparable.
3. Message format: `[ignis][P<n>][T<n>][<topic>] <imperative summary>` — the sub-project scope
   (plan numbers repeat across the monorepo: vllmcustom and ninfer each have a Plan 1), plan number
   (not zero-padded), task number, short area tag, then the summary. Keep the whole subject under
   72 characters. Outside a plan: `ignis: <summary>`. Plan bookkeeping replaces the task tag:
   `[ignis][P<n>][plan] add <title>`, `[ignis][P<n>][revise] …`,
   `[ignis][P<n>][close] <title>: <outcome>`. Examples:
   - `[ignis][P1][T2][run] add qwen38fn/run for Flash-Next on :8700`
   - `[ignis][P1][close] launchers: shipped, rtxpro6000 profile`
4. **Tracker row in the same commit.** The row update — and any `context/` update the task
   produced — lands with the work. Stage explicit paths: `git add <paths>`, never `git add -A`.
5. The full project gate (Rule A) passes before every commit.
6. A change in observable behavior updates `README.md` in the same commit.
7. **Never push** — the human pushes after review. **No co-author trailers** unless asked.
8. Every task in a plan carries its `**Commit**:` line, so the reminder is baked in.

#### Project addendum

- **Never offer to push either.** `git push` in every form — `--dry-run` included — is the
  owner's, and so is pushing an *image* to a registry. Report the commit, never the remote: say
  "I did not run `git push`". "Not pushed" is a claim about a remote you cannot see.

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

#### Project addendum

The monorepo is shared: sibling sub-projects (`../ninfer`, `../vllmcustom`) are often mid-work in
the same checkout. Their uncommitted files are **someone else's work** — never stage, stash,
restore or clean them; stage only `server/ignis/…` paths.

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
gets cited. Upstream's published numbers (Ignis's README and findings, measured on *their* cards)
are `[DOC]` claims to beat or reproduce, never our result.

1. **N ≥ 3 runs.** Report mean and spread. One run is an anecdote.
2. **A delta inside the spread is "within noise"** — write those words. Never report it as a win,
   a regression, or a trend.
3. **Name the source**: input/capture, build (git SHA or version string), environment and
   settings. It goes in the tracker's Evidence table. For this project that means the image
   reference **with its digest**, the Ignis version the server reports, the artifact file and its
   Hugging Face revision, the profile in effect, and the launcher's echoed `docker run` line.
4. **Change one thing at a time.** A different build *and* different settings in one comparison
   measures nothing.
5. **Check the environment first** (throttling, background load, warm vs cold caches, another
   server still holding the GPU). A run under a bad state is re-measured, not caveated.
6. **Inconclusive is a valid, recordable outcome.** Record it. Do not round it up.

### P2 — Capture hygiene

1. Captures, traces and raw result files go in `captures/` and are **gitignored** — they are
   large and regenerable.
2. Name them so they are identifiable six months later: `<workload>_<YYYY-MM-DD>_<what-varies>.<ext>`.
3. The tracker's Evidence table says which capture backs which number.
4. **Restore the system after a session** — `nvidia-smi -pl/-lgc`, launcher env knobs, debug
   flags, the vLLM daily driver stopped for the bench. The next session's baseline depends on a
   known state.

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
- Code blocks always carry a language (` ```bash `, ` ```json `, ` ```text `).
- Cross-reference plans as "Plan N" or "Plan N T2"; cite sources as `path/file:line`.
- Dense, no fluff — but never so compressed a detail is lost.

---

## Templates & History <!-- hp:templates -->

- New plan: copy `NN_example.md` → `NN_name.md` and `NN_example_tracker.md` →
  `NN_name_tracker.md` (drop their first-line `hands-plan` stamp), take the number from SERIES,
  fill every section, add the SERIES row, and bump **Next free plan number**.
- For real examples of the live format, browse `context/plans/completed/` — newest first.
