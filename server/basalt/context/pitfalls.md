# Pitfalls & Gotchas — basalt

Every bug, gotcha and trap — **especially anything that took more than one attempt to fix.** Read
before new work. Tag entries that actually bit `[OBSERVED YYYY-MM-DD]`; untagged entries are
hazards seeded from reading, not incidents.

Cross-cutting entries also go up to `../../../context/pitfalls.md`; project-specific ones stay here.

<!-- Entry template (~200 words per half; quote the symptom verbatim):

# <Pitfall title>

## Problem
[OBSERVED YYYY-MM-DD] What happened, the exact error/output, why it was confusing, what was
tried first and why that failed.

## How to avoid
The fix, the rule that prevents it, and how to detect it early next time.

## Sources
- <project/sub-project>: `<short_filename>` (`<function_name>`)
-->

---

# The "NInfer" QUASAR 27B looks like a Basalt model, and is not

## Problem
Seeded from reading at init (2026-11-04), not yet an incident. The first request for this project
named `models/vllm/jesdga/Qwen3.8-27B-QUASAR-DFlash2-nvfp4-NInfer` for `./run`. The HF author
(`jesdga`) is also Basalt's author and publishes Basalt packs, so the artifact reads as Basalt's.
It is `qwen3_8_27b_quasar.ninfer`: a dense Qwen3.8-27B file for NInfer. Basalt loads only
Qwen3.8-Flash-Next `.basalt` packs; handed the `.ninfer` file it would fail at load, after the
container started.

## How to avoid
`./run` checks the pack before starting anything: the path ends in `.basalt` and exists, else it
dies naming `../ninfer` for `.ninfer` files. Recorded as out of scope in `plans/SERIES.md`.

## Sources
- basalt: `context/distilled.md` (Basalt loads only Qwen3.8-Flash-Next …)
- the artifact's `README.md` (`library_name: ninfer`)

---

# The image boots the mock engine but not a real pack: `serve/` imports `tools/model_file.py`

## Problem
[OBSERVED 2026-11-04] The first image (`27770926a3f6`) shipped `bin/`, `serve/`, `data/` and the
venv, passed every gate step and served the mock engine — then the first real `./run` died right
after `[server] legend`:
`File "/opt/basalt/serve/engine/modelfile.py", line 14 … from model_file import ModelFile` →
`ModuleNotFoundError: No module named 'model_file'`. `modelfile.py` does
`sys.path.insert(0, ROOT / "tools")` and imports by bare name, so a grep for `from tools` /
`import tools` over `serve/` (the check made while planning) found nothing, and the mock engine
never imports `serve.engine.modelfile` (it has no model file). Upstream's `docs/building.md` does
say an install or container image ships `serve/` **and `tools/`** — recorded in `distilled.md` at
init and still left out of the Dockerfile.

## How to avoid
The image ships all of `tools/` (448 KB). Gate step 6 imports every non-test `serve.*` module in the
image, so a missing file fails before a boot; on the broken image it printed exactly
`FAIL(imports) serve.engine.modelfile: ModuleNotFoundError: No module named 'model_file'`. When
upstream's docs list what an install ships, copy that list — don't derive it from imports.

## Sources
- basalt: `Dockerfile` (runtime-stage `COPY`s); `serve/engine/modelfile.py:12-14` at `dc047a77`
- basalt: `context/plans/RULES.md` § Project gate, step 6

---

# Empty output from a failed command was recorded as a measurement

## Problem
[OBSERVED 2026-11-05] While planning Plan 02 (the `vendor/basalt` security audit) I needed to know
whether an image existed for the pin, ran `docker images --format … | grep -i basalt | head`, got
**no output**, and wrote into the plan's Key Facts table: *"No built image exists for the pin yet —
`docker images | grep -i basalt` → empty."* Docker was not reachable from the agent session at all:
`/run/docker.sock` is mode `660` with numeric owner `0:995`, and the session is `uid=1000` with
supplementary groups `1000,27` — no group the session holds owns it. The real error — `permission
denied while trying to connect to the docker API at unix:///var/run/docker.sock` — went to **stderr**,
and `grep` on stdout turned the failure into a confident-looking empty result. It surfaced only when
`PREFLIGHT_ONLY=1 ./build` exited 1 and named the permission error. The same class produced a second
over-claim: `git cat-file -e <blob>` "proving every blob is local" was run **without
`GIT_NO_LAZY_FETCH=1`**, so it may have succeeded by fetching from origin mid-check — "was already
local" and "fetched on demand" are indistinguishable in that output. Both Key Facts were downgraded in
the plan with their limits stated.

**Second incident, same class, same day (2026-11-05) — and it nearly cost a GPU window.** Resuming
Plan 01 T6, the preflight was `ss -ltnp 2>/dev/null | grep -E ':8700' || echo ":8700 free (no
listener)"`. It printed *":8700 free"*, was reported to the owner as verified, and the boot block was
drafted on that basis. **`ss` is not installed in the agent container** (`command -v ss` → nothing):
the missing binary wrote its error to stderr, `grep` read empty stdin and exited 1, and `||` supplied
the reassuring default. The port was not free — `curl http://cosmo.lan:8700/v1/models` returned **200**
with `{"id":"cosmo-6000","status":{"value":"loaded"}}`: a Basalt server was live and carrying the
owner's real traffic, with 77 GiB of the 97.9 GiB card committed. Two probes were then aimed at it
before the arithmetic was done — a second, separate mistake, recorded under *A live daily-driver
server is not a test bench*.

## How to avoid
**An empty result from a command that was never allowed to run is not a measurement.** Three habits:
(1) end every measurement command with an unconditional sentinel and read it before the output —
`; echo "— done rc=$?"`; (2) never trust a pipeline's emptiness when the tool at its head can fail
on permissions — check `2>&1` once, or the exit status of the *first* command, not the pipeline's
last; (3) for git in a partial clone (`promisor=true` / `blob:none`, which is how `./build` clones
`vendor/basalt`), put `GIT_NO_LAZY_FETCH=1` in front of anything that asks whether an object is
already here, and remember `git cat-file -e` proves object **existence**, never that the working file
matches it — only `git hash-object -- <path>` compared to the blob id does. In an audit whose whole
purpose is proving absence, this is the failure that matters most. **(4) A probe must be able to answer
UNKNOWN.** `cmd | grep x || echo "not running"` collapses three states — *found*, *not found*,
*tool absent* — into two, and `pipefail` cannot save you because `||` is exactly where the failure was
swallowed. So `command -v <tool>` **before** anything consumes its answer, and never let the `||` branch
carry a reassuring default. **(5) Use the oracle the project already ships** instead of hand-rolling a
port check: `DRY_RUN=1 ./run` runs `refuse_busy` and prints
`== (DRY_RUN) a real run would refuse: :8700 or the GPU is busy` (`run:92-97`); where a generic check is
genuinely needed, `curl -o /dev/null -w '%{http_code}'` is better than `ss` because a live answer *is*
the answer, and it cannot silently not exist. **(6) An environment assertion needs the right
environment.** Socket ownership, gid→name mappings, "tool X is installed" and "port Y is free" are facts
about *the shell that ran the command*: label them "agent sandbox view, host unknown" unless the owner's
shell produced them.

## Sources
- basalt: `context/plans/02_basalt_vendor_audit.md` (Key Facts — the two downgraded rows)
- basalt: `context/plans/02_basalt_vendor_audit_tracker.md` § Notes / Deviations, 2026-11-05
- basalt: `context/plans/completed/01_flashnext_container.md` § Gate status (the `ss` incident, 2026-11-05)
- **Corrected in place 2026-11-05:** this entry originally stated the docker socket *is* `root:rtkit 660`
  and that the session was "not in group `docker` (gid 991)". That named the **agent container's**
  `/etc/group`, where gid 995 resolves to `rtkit` and `docker` is 991; the host socket's group was never
  measured and is still unknown. What is known is the numeric fact (`660`, `0:995`) and the refusal.

---

# A gate prerequisite recorded in RULES.md can silently stop being true

## Problem
[OBSERVED 2026-11-05] `context/plans/RULES.md` § Project gate step 2 states *"`shellcheck` 0.11.0 is
installed (`/usr/local/bin/shellcheck`, 2026-11-04)"* and that a missing tool is **fail, not skip**.
On 2026-11-05 there was no `shellcheck` at that path, on `PATH`, or in `~/.local/bin`, `~/bin`,
`~/.cargo/bin`, `~/.bun/bin` or `~/.pyenv/shims`. Step 2 therefore failed for a markdown-only commit
that could not possibly affect it — which, left unexplained, reads to the next session as *"my change
broke the gate."* The same session hit the docker half: gate steps 3–6 are unrunnable because the
agent cannot reach the docker socket (sandbox view — see the entry above, *Corrected in place*). Two of
the project's six gate steps were red for reasons no file said.

## How to avoid
Fix the stale prerequisite **in place, dated** (house style for wrong knowledge), and carry gate
state where a session will look: a per-step **Gate status** block in the plan — step, last run date,
pass/fail/blocked, who *can* run it — rather than a paragraph in the tracker's Notes. State plainly
which steps are owner-only, and have commit messages name the steps they could not run. Never let a
red environmental step quietly become a "close enough" pass: if the owner chooses to proceed with a
degraded gate, record it as *"gate knowingly degraded, owner decision, dated"*. Verify a restored tool
with a **negative control** before trusting its clean output (Plan 02 T0 ran it against a deliberately
bad script and required SC2115 to fire).

## Sources
- basalt: `context/plans/RULES.md` § Project gate step 2, § Prerequisites
- basalt: `context/plans/02_basalt_vendor_audit.md` § Gate status
- basalt: `context/plans/completed/01_flashnext_container.md` § Gate status (added 2026-11-05 for exactly this)

---

# A live daily-driver server is not a test bench

## Problem
[OBSERVED 2026-11-05] Believing `:8700` was free (see *Empty output from a failed command…*), an agent
session aimed Plan 01 T6's prescribed probes at the server that was actually running — the owner's
**live daily driver**, serving their real coding traffic — and then read its `/metrics` and
`storage/cache/basalt/<hash>/engine.log` as if they were the product of a boot it had directed. Three
things went wrong at once:
- **A chat probe costs the owner a slot.** `GET /v1/models` and `/metrics` are free; `POST
  /v1/chat/completions` is admitted into one of `batch_slots` (4) and allocates KV — and on this box KV
  is elastic *against the expert cache* ("the other 13.73 GiB of KV are the expert cache's last 5334
  slots until a request needs them", `engine.log`). So the probe can move the very number being
  measured. Only the responses that never reach the scheduler were harmless: the bogus-model `404` and
  the `Host:`-check `403`.
- **Its telemetry is not our Evidence.** `/metrics` `requests[]` and the log's prompt lines are the
  owner's conversation data — token counts, prompt sizes, timing. And a server the owner started has
  unconfirmed provenance (args, image, pin), so it cannot stand in for a task that specifies *"the
  owner runs `./run` and pastes the start block"*.
- **There is no room to work around it.** One card: `expert_cache_primary_mib 64806` wanted against
  `vram_free_mib 24435` free (`nvidia-smi`: 77,117/97,887 MiB used). `--single` shrinks KV slots, not
  the expert cache, so it needs the same ~63 GiB and **cannot** boot alongside. Anything a plan wants to
  start requires the owner to stop what they are using — a stop-the-world window that is theirs to
  schedule. (`./run` would have refused anyway — `refuse_busy`, `run:96` — so the risk was a wrong
  claim and a stolen slot, never a clobbered server.)

## How to avoid
Against a server you did not start: **read geometry, never submit work** — `GET /v1/models` and
`/metrics` yes, any `POST` no; and never `read` the engine log's request lines: run a filter that can
only emit aggregates, and record the filter plus its numbers, not the lines (same rule as `audit/wave<N>/`
holding raw output "never into a tracker", and `captures/` being gitignored). Tag live-box readings
`[LIVE <date>]` **with "owner's server, launch provenance unconfirmed"** and keep them out of a task's
Evidence column — they corroborate, they do not verify. Bucket every fact before writing it: *directed
boot* → Evidence; *live observation* → context/distilled with its caveat; *reachability* (`200` from
`cosmo.lan:8700`) → a fact about the network. When the only way forward is to stop the owner's work,
say what the window costs (here: two weight loads at ~40 s each, plus a period where their clients get
1 slot not 4), put the clock in their hands, and state that declining is a legitimate answer that keeps
the task `in-progress` with the reason recorded.

## Sources
- basalt: `run:92-97` (`refuse_busy`, and that `DRY_RUN` reports rather than refuses); `run:96`
- basalt: `context/plans/completed/01_flashnext_container.md` § T6 (the probes as specified), § Gate status
- live server's own lines: `/metrics` (`batch_slots 4`, `expert_cache_primary_mib 64806`, `vram_free_mib
  24435`), `storage/cache/basalt/aefd5d98dc650ed4/engine.log` (elastic-KV / expert-cache loan lines)

---

# A shared worktree has no such thing as "my uncommitted edits are private"

## Problem
*Correction (host agent session, 2026-11-04 PST): the other writer was the **host agent session**, not
the owner — `b308244` is its commit (`0b1efb6`, `aec717a`). Read "the host session" for "the owner"
below; the lesson is unchanged.*

[OBSERVED 2026-11-05] The agent was editing `context/plans/01_flashnext_container.md` and the Plan 01
tracker while the owner worked in the **same worktree** on the host. He ran a commit; it collected the
agent's in-flight files with its own. `b308244`, whose subject is *"record memlock unlimited inside the
container (plan Q2 resolved)"*, thereby carried 30 lines of the agent's `## Gate status` block and two
tracker edits, under his subject line and his authorship. Three consequences, all real:
1. Work lands under a message that does not describe it and an author who did not write it — blame and
   every future `git log -S` now point at the wrong task.
2. **Two authors edited the same bullet within the same minute and it merged into a self-contradicting
   paragraph on disk**: the owner's `RESOLVED … every expert sits in VRAM anyway` with the agent's
   `— still open: …` glued to the end of the same sentence. Nothing git does flags this; the file simply
   reads as nonsense to the next session.
3. The agent's mental model — "that edit is uncommitted, it's still on the bench" — becomes false without
   notice. It is in HEAD, and **Rule B2 then removes the escape hatch**: `--amend`, `rebase`,
   `reset --hard`, `revert`, and `stash` / `checkout -- <path>` / `restore` / `clean` *on work you did
   not make* are all banned. There is no safe untangling by undo. Only forward commits.

A fourth, quieter one: an authoritative-looking document (a Gate status table) landed in history
half-reconciled — the same failure as a stale gate prerequisite, which "reads to the next session as
*my change broke the gate*".

## How to avoid
- **The writer with more bandwidth keeps the tree clean.** An agent must not end a turn with knowledge
  files dirty, especially not across a window when the owner is live: stage **explicit paths** as each
  logical unit finishes (`git add <paths>`, never `-A` / `-a` — `RULES.md` § Rule B.4 already says this)
  to shrink the in-flight window to minutes.
- **Name the dirty files in every handoff**: end the message with
  `dirty: context/pitfalls.md, context/distilled.md (mine, in progress)`. It is one line and it is the
  only signal the other writer can act on without reading a diff.
- **Re-read before you write a hot shared file.** `git log -1 --format='%h %ad %s'` and `git status`
  first; then read the *current* text of the region you are about to edit rather than trusting what you
  last wrote. A self-contradicting sentence is the tell that someone else was there.
- **When a collision is discovered: HEAD is ground truth. Fix forward, with dates.** Never undo the other
  writer's commit, never `restore` their work out of the tree. Keep *their* verdict and measurement
  verbatim; delete only your own mechanical half; put your disagreement in as a separately attributed
  note rather than by editing their sentence — softening another author's assertion by hand is overreach,
  and silently deleting a clause that contradicts its own head clause is worse.
- **A `RESOLVED` headline may claim only what its own row proves.** The owner's row stated
  `VmLck 0` / `VmPin 0` — "CUDA-pinned host memory is not counted there" — and the bullet above it then
  asserted residency from it. The absence of a warning proves the negative case only.
- **Skip the machinery**: lock files, per-agent worktrees and hooks that refuse the owner's commits all
  cost more than the collision — he has to be in this worktree for GPU and docker work, and a hook that
  blocks him at 02:22 gets routed around inside a week.

## Sources
- basalt: `context/plans/RULES.md` § Rule B2 (append-only; the banned-command table), § Rule B.4
  (`git add <paths>`, never `-A`)
- basalt: `git show b308244` (its subject vs its `--stat`: `01_flashnext_container.md` +30/−2)
- basalt: `context/plans/completed/01_flashnext_container_tracker.md` § Notes / Deviations, "Two writers, one
  worktree", 2026-11-05

---

# A boot from the NFS copy crawls at 5–10 MB/s

## Problem
[OBSERVED 2026-11-04] Boot 2 (`./run` with the pack at `models/vllm/jesdga/…`, NFS 4.2 over RDMA from
noir) printed `loading weights into cuda0 … 4.8/67.8 GiB  6 MB/s` and would have taken hours; the
owner stopped it at 7.4 GiB. The host was idle; six engine threads sat in `nfs_file_direct_read`.
Two causes stacked: Basalt reads weights with `O_DIRECT` (no switch — `distilled.md`), and the file
on noir was shredded into 131,723 extents on spinning RAID. The first guess — "NFS is slow" — was
half wrong: with the file cached on noir, direct reads ran 749 MB/s.

## How to avoid
Serve from local NVMe: `./run`'s default pack is `models.local/…` (40 s boots). If a pack must come
from noir, defragment it there first (`btrfs filesystem defragment -f -t 128M <file>`, check
`filefrag`), since downloads land fragmented. Diagnose a slow load from the host with
`ps -L -o tid,stat,wchan:32 -p <engine pid>` — `nfs_file_direct_read` in state `D` is this pitfall.

## Sources
- basalt: `context/plans/completed/01_flashnext_container_tracker.md` (boot 2 rows, Notes)
- basalt: `include/basalt/core/direct_upload.hpp:1-6` at `dc047a77`

---

# Shell idioms that report green on the thing they did not check

*The count is deliberately not in the heading — it grows, and every earlier cross-reference then goes
stale. Two already have: `audit/wave0/t1-freeze-and-manifest.txt` cites this § as "Four shell idioms…"
(that artifact is a stamped transcript of a past run and is **not** edited retroactively — this line is
the reconciliation), and the follow-up commit grew it to seven. Twenty-one entries as of 2026-11-05; items
10–17 are from Plan 02 T2, and items 18–21 are from Plan 02 T3's security harness — all from the same
week and the same failure family: *the check that reports green because the thing it checks was never
reached, or was looking somewhere else.* Items 20–21 are the two that a passing audit rested on while
wrong: a freeze check blind to an ignored directory, and a file listing that contained itself. Items
11–13 are the same three traps that produced item 9's family.*

## Problem
[OBSERVED 2026-11-05] During Plan 02 T1 (freeze `vendor/basalt`, manifest 862 files), its coverage
follow-up the same day, and **T2 (pin provenance + author census)**, sixteen commands came back *passing*
while the measurement had not happened, and one transcript line recorded output that no command ever
produced (item 17 — the mirror image: silence was not the problem, invention was). Each looked like evidence:

1. **`IFS= read -r h p` over a space-separated `git … --format='%(objectname) %(path)'`.** Setting
   `IFS=` disables whitespace splitting, so the whole line lands in `$h` and `$p` is empty. Every one
   of 862 iterations compared against nothing: `fatal: could not open '' for reading` × 862 and
   `MISMATCH_COUNT=862`. An earlier run of the same loop written as plain `read -r h p` had correctly
   reported 0 mismatches, so the *correct* answer had already been on screen and the wrong one read as
   a regression. Only the printed count exposed it.
2. **A dot-prefixed file invisible to a glob.** The manifest split wrote `.github.tsv` (after the
   directory). `awk 'END{print NR+0}' manifest/*.tsv` said **859**, not 862 — three rows silently
   dropped, and `wc -l manifest/*.tsv` never lists the file either. Any count through `*.tsv` is
   blind to it forever.
3. **`diff -r a b` over two empty directories → "DETERMINISTIC".** The generator had run from the
   wrong cwd, printed `fatal: not a git repository`, and produced nothing; the comparison of the two
   empty outputs succeeded. A *comparing* command inherits the emptiness of what it compares.
4. **`sha256sum -c` on a sidecar of `cut -f1-4` hashes.** `-c` re-hashes **whole files**, so all 12
   TSVs reported `FAILED` — the recorded value was never what `-c` recomputes. The design was sound,
   the verifier was not, and nothing in the tool said so.

Same family, smaller: `— rc=$?` after a `while … done` reports the loop's last test, and after
`cmd | tail -1` reports `tail`'s — so `— rc=0` beside `permission denied` is exactly what a failed
docker step looks like in this convention. Two variants of that are worth naming because they were
*committed* before being caught: `awk 'NF!=9{print …}' *.tsv; echo "— fields rc=$? (silent = ok)"`
(awk exits **0** after printing its warnings, so the `ok` fires on garbage) and
`for f in *.tsv; do … || echo "UNSORTED $f"; done; echo "order ok"` (prints `order ok` whether or not
`UNSORTED` lines appeared).

5. **An assertion that prints `FAIL` and still exits 0.** The partition check was a bare
   `awk '… END{printf "PARTITION EXPECTED=%s ACTUAL=%d %s\n", …}'` — the verdict was a *string*
   computed inside awk, so a typo'd tier reported `ACTUAL=861 FAIL` and the script went on to print
   `ALL_SENTINELS_PASS=1`. A verdict computed in a sublanguage cannot set the shell's exit status: the
   value must come back (`part=$(awk '…END{print A+B+U+0}')`) and go through the same `check` helper
   that sets the fail flag.
6. **A guard written to catch a bug it cannot see.** `M=(*.tsv)` **never matches a dot-prefixed file**,
   so `check TSV_FILES 12 "${#M[@]}"` could only ever catch a 13th *visible* TSV — while its comment
   claimed to cover the `.github.tsv` failure in item 2, which had already happened once. Cross-prove
   the glob against `find . -maxdepth 1 -name '*.tsv' -type f | wc -l`: `find` is dot-aware and
   `-type f` additionally rejects a symlinked-in substitute (both sentinels see the same 12 otherwise).
7. **`uniq -c | awk 'END{print sum}'` deletes the histogram.** Piping a histogram into a summing awk
   without `{print}` leaves only the total on stdout — the new sentinel survives and the evidence the
   command existed to show is gone. Use `awk '{print; s+=$1} END{print "SUM=" s+0 " (expect 862)"}'`.
8. **`2>/dev/null` turns a bad flag into an empty column that still prints.** [OBSERVED 2026-11-05,
   T2] The author census asked each file's history breadth as
   `git rev-list --count --follow <pin> -- <path>`. **`rev-list` has no `--follow`**: git wrote its
   usage block to a stderr the script had redirected to `/dev/null` and wrote *nothing* to stdout, so
   the `commits` column came back **empty for all 75 rows** while every row still printed and the
   census declared itself validated. It surfaced only because the same column was visibly blank while
   inspecting a different file set. Same session, same mechanism: a `git blame` probe wrapped in
   `2>/dev/null` printed `0` for a path that does not exist. Never suppress stderr inside a check —
   append it to a log and **size it**: `ERR_BYTES=$(wc -c < err.log)` asserted `=0` as part of
   `VALIDATION` (`context/audit/scripts/t2-census.sh` does exactly this). Correct history form:
   `git log --follow --format=%H <pin> -- <path> | wc -l`.
9. **A comparison over two key sets that never overlap reports a perfect score.** [OBSERVED
   2026-11-05, T2] The provenance comparator builds `{path: blob}` from GitHub's tree object and the
   same shape from `git ls-tree -r`. One swapped pair of awk fields made the *local* keys blob SHAs, so
   the intersection was empty and it printed `ONLY_IN_API=862 ONLY_IN_LOCAL=861 BLOB_SHA_MISMATCH=0` —
   a **zero over nothing**, one step from being pasted as "862/862 verified". Every `comm` / `join` /
   `diff <(sort a) <(sort b)` / set-minus has this hole, including `audit/index.md`'s own check 6
   (`comm -3 … | wc -l`, expect 0 — two empty inputs sail through it, guarded only indirectly by check
   2). Fix: print the cardinalities and assert all four — `SET_A=$(grep -c . a) SET_B=$(grep -c . b)
   INTER=$(comm -12 a b | grep -c .) DIFF=$(comm -3 a b | grep -c .)` → `862 862 862 0`. Then make the
   comparator **prove it can fail**: the identical comparison against the pin's first parent returned
   `BLOB_SHA_MISMATCH=1`, resolving the single changed file out of 862. A mismatch-counter that has
   never been observed non-zero is not a check.
   Same species: an **end-anchored pattern against a headerless TSV line** when the field you mean is
   not the last one. `cut -f1,2 manifest/*.tsv | grep -vE '\.(py|cpp|hpp)$'` matches *nothing*, because
   each line ends in the LOC digits — a "which Tier A files are not source" classification came back
   listing all 75. Index the field instead (`awk -F'\t' '$1 ~ /\.[ch](pp)?$/'`); on a headerless file
   there is no way to eyeball which column is last.
10. **A uniform negative across an entire endpoint family is a broken probe, not a finding.**
    [OBSERVED 2026-11-05, T2] Every `GET …/api/1/revision/<sha>/get/` returned `404` — nine SHAs, all
    `404`, including a value the archive demonstrably holds. That is not "the archive has nothing", it is
    "the route is wrong": Software Heritage's revision endpoint is `/api/1/revision/<sha>/`, **with no
    `/get/` suffix**. With the correct form and controls (their tip → `200`, zeros → `404`) the same
    investigation inverted: the archive holds the ancestor repo in 9 visits (8 `full`, 1 `failed`) *and our own root
    commit*. The damage was already done — "Software Heritage has never archived this project" had been
    drafted for the record off the broken run. Rule: **a probe family that never returns a positive has
    measured itself, not the world.** Put a known-positive beside any new endpoint before believing a
    single negative from it, and treat an *all-negative* sweep as more suspicious than a mixed one. The
    same signature appeared twice more in the same task, both as a **uniform zero**: 367 blamed commits
    none of which "carried an AI trailer" (item 12) and "0 blame boundary lines" (item 13).
11. **Abbreviated SHAs — three separate incidents in one session.** [OBSERVED 2026-11-05, T2] Local
    `git` resolves an 8-hex prefix, so probes *appear* to work while sending a different request shape
    than a full OID does. Where it bit: (a) seven "is this commit upstream?" API probes went out as
    8-hex, so their `422`s were partly request-form artefacts and could not be read as non-resolution —
    the control that *did* discriminate was a full-40-hex nonsense SHA; (b) a **partial clone refuses
    abbreviated OIDs outright** (`Not a valid object name 09ae16000f84…`), which looks identical to a
    legitimate negative — promisor probes need full OIDs, full stop; (c) a hand-typed root SHA used in a
    `startswith` comparison was 34 hex chars, not 40, so it matched where `==` would not have and briefly
    "proved" our root was a snapshot branch-tip. Fix: assert length before use
    (`[ ${#sha} -eq 40 ] || continue`) and generate probe inputs from `--format='%H'`, never from
    anything a human or a `--short` touched.
12. **`git log --format='%H%x00%B'` piped through a `split("\0")` yields `BODY` + `NEXT_SHA` chunks, not
    records.** [OBSERVED 2026-11-05, T2] The NUL was the *field* separator, so splitting on it glued each
    SHA to the *previous* commit's body; the "SHA set" that came out contained commit subjects, its
    intersection with 367 blamed commits was **0**, and that zero nearly entered the record as "no AI
    trailers in the audited lines". Correct form: `git log -z --format='%H\t%B'` (NUL *between* records,
    tab inside them), or unit separators like `--format='%H\x1f%ae'`. General shape: **when you pick both
    the record separator and the field separator, name which is which in the code**, and validate the
    parse (`malformed_headers=0`, sampled values must look like 40 hex) before the set feeds a comparison.
13. **Grep for a `git` porcelain *header* must be anchored — porcelain echoes your source lines.**
    [OBSERVED 2026-11-05, T2] Counting blame boundary lines with `git blame --line-porcelain | grep -c
    boundary` also matches the word inside the blamed **code**: `a prefetch window starts on a page
    boundary` in `memory.cpp`, `boundary` in `CMakeLists.txt`. The figure published from an unanchored
    variant of that expression was **0**; anchored truth is **560 boundary lines across 7 files**, the
    same population as the 560 lines blamed on the root commit (blame cannot walk past a parentless
    commit). Use `grep -cE '^boundary( [0-9a-f]{40})?$'`. Same hazard for every porcelain keyword
    (`parent`, `author`, `summary`, `previous`): the format prints arbitrary user text between headers.
14. **`tail -n +2` on a headerless TSV deletes a data row — and this one was in the *check* of the row
    count.** [OBSERVED 2026-11-05, T2, twice in one session] `context/audit/wave0/t2-author-census.tsv`
    is headerless by manifest convention, so `tail -n +2` discarded `CMakeLists.txt`, the largest row in
    it. Every figure derived through it was short by one file: a "80 OK + 6 skipped = 86 ≠ 87"
    *arithmetic anomaly* that took three commands to explain, and a derivation sentence ("87 − 1 vendored
    = 86") that nearly shipped as the documented denominator. The measurement itself was fine because it
    had used a field-filter (`$13=="OK"`), which is why the anomaly was visible at all. Rule: **know
    whether an artifact has a header before you skip a line, and prefer an explicit predicate over a
    positional skip.** If a header is genuinely present, print it (`head -1`) in the same transcript so
    the reader can see what was skipped.
15. **A pathspec-scoped `git grep` cannot support a "does not exist anywhere" claim.** [OBSERVED
    2026-11-05, T2] To decide whether a shipped binary (`data/profiles/qwen4exp-48x512.prof`) had a
    generator, I ran `git grep -l -I -E "\.prof…|open\(.*prof|\.save\(.*prof" <pin> -- '*.py' '*.sh'`, got
    empty, and wrote "no writer/generator for `.prof` exists anywhere in the repo at this commit". **False.**
    The writer is C++ — `basalt::core::write_expert_profile` (`src/core/expert_cache.cpp:99`), called from
    `src/program/serve/tier_profile.cpp:16` under `--expert-profile-save` — and `docs/model-file.md:87-89`
    documents producing one, in the same file I had cited to argue the opposite. The empty result measured
    my pathspec, not the repository. Rule: **state the pathset in the claim, or drop the claim.** For a
    negative existence claim either search the whole tree (`git grep -I <pat> <pin>`, no pathspec, then
    subtract what you deliberately excluded and say why) or downgrade to "no *offline, deterministic*
    generator in `tools/`, `tests/`, `.github/`". Search for the *thing*, not for a guess at its spelling:
    searching for the format's writer by symbol name finds what searching for `.prof` in filenames misses.
    **Corollary, same trap computing a number instead of a name [OBSERVED 2026-11-05, T2]:** a path in a
    `git diff --numstat -- <paths>` list that matches nothing contributes **nothing and says nothing**, so a
    hand-assembled path list silently under-counts — my § 7 diff-shape figure was 18 lines short because one
    path omitted an `audit/` segment. Any sum taken over an explicit path list must be reconciled against the
    same sum with **no** pathspec (`954 + 1861 + 349 = 3164` against `git show --numstat` of the commit), which
    is a partition check, not a second opinion. **Its sibling, same section one commit later:** the fix for
    that under-count corrected the number it had *looked at* and left a second stale figure standing three
    lines below it, because a value-search for the wrong number cannot find a number you do not know about.
    Sweep by asking what reproduces each numeral; `RULES.md` Rule E.6. Two naming notes from the same incident,
    both about that file: it is **`context/audit/coverage_ledger.md`** — the mispath that caused the under-count
    dropped the `audit/` segment — and its tier roll-ups are **derived**, so `context/audit/manifest/index.md`
    is the owner of the grid and the thing to cite for a tier count; the ledger is the commentary beside it.
16. **A date partition is not defined until you name its basis — `--date=iso-strict` compared as a string
    partitions by the author's local calendar date.** [OBSERVED 2026-11-05, T2] `git log --format='%ad'
    --date=iso-strict | awk '$1 < "2026-10-05"'` looks like a UTC cut and is not: the field carries the
    **author's own UTC offset**, so lexicographic comparison buckets by local wall-clock day. Our
    pre/post-fork split published as **882 / 1,312**; on `%at` epochs at `2026-10-05T00:00:00Z` the same
    history is **861 / 1,333** — 21 commits authored on Oct 4 local are Oct 5 in UTC. Both are internally
    consistent, and the two figures disagreed with each other in three documents before it was caught.
    Rule: partition on **`%at` / `%ct` epochs** (or `--date=iso-strict` parsed to a tz-aware datetime),
    never on a formatted date string, and **print the cutoff as an instant with its offset** so the basis
    travels with the number. Corollary, same family: `--date=short` **inside** a `--format=` string is not
    an option, it is literal text — the flag silently does nothing and the field format changes under you.
17. **A transcript line in a stamped artifact is a claim that a command ran — paste it or don't write
    it.** [OBSERVED 2026-11-05, T2] `audit/wave0/t2-gate-and-checks.txt` recorded gate step 1c as
    `SKIP(1c: no versions.env yet)` annotated "← SKIP, not pass. Pre-existing state, unchanged by T2."
    **No run ever printed that line.** `versions.env` has been tracked since `1eafda7` (2026-11-05T01:20Z,
    Plan 01 T1), so the `else` branch that emits that string is unreachable in this project, and T1's own
    stamped gate block records `env ok` for the same step. The sentence had been carried in from what the
    step *would* print in a repo without pins — a prediction of output, typed into a transcript. Cost had it
    shipped: RULES.md § *The gate* makes "**skip is not pass**" load-bearing for steps 3–6, which genuinely
    are owner-only; spending that marker on a line that never ran trains the next reader to discount the
    real SKIPs. The same error appeared one level up, in the tracker note written to
    explain it: "step 1c **changed state** under this task, and it is a pass, not a regression" — an
    unobserved *before* state, invented to reconcile a fabricated observation with T1's genuine one.
    Corollary from the same minute: my chat report of the gate said `env ok` while the artifact said `SKIP`,
    and neither could be checked against the other — only a fresh run settled it. Rule: **every `text` block
    in a `wave*/` artifact is pasted bytes, or it is labelled** (`expected` / `template` / `not run`); when a
    transcript asserts a *precondition* ("no X yet", "X absent"), check X's tracked status before writing the
    line; and when two records of one step disagree, re-run the step rather than adjudicate from memory.
18. **A locator you have to maintain is not a locator.** [OBSERVED 2026-11-05, T2] To name the newest row of a
    scope-evidence table (`audit/wave0/t2-gate-and-checks.txt` § 8) whose own SHA does not exist yet, I built a
    `git log -S'<phrase unique to that row's note>'` pickaxe anchor and then wrote a paragraph declaring the
    anchor string **load-bearing**. One commit later the mechanism was false. Two independent measurements:
    **(1)** a § 8 heading phrase used as an anchor resolved to exactly one commit when the warning was typed and
    to **two** one commit later — not because anything was renamed, but because the later commit deleted an
    unrelated command that happened to quote the phrase, and `git log -S` fires on any change in **occurrence
    count**; **(2)** separately, a § 6a heading renamed mid-plan (`t2-boundary-authorship.txt`) left its old
    phrase resolving to a 2-commit set (`6dbb312` in, `d388d17` out). Worse, the paragraph documenting the
    hazard **quoted the phrase it was counting**, so reading and re-copying the file kept degrading the
    instrument. What came out of it: **an anchor rots through ordinary prose edits, not only renames**, so prefer
    the identifier you already have as soon as it exists; **never quote verbatim the literal whose count or
    match-set a check reports** — that is editing the world to fix the instrument; and a locator whose string has
    to be kept in step with the thing it locates is not a locator, it is a second copy of it. The replacement is
    a convention, not a mechanism: *the commit that adds a table row fills in the SHA of the row above*, so
    exactly one row is ever nameless and nobody has to construct a command to name their own commit. Governance
    corollary, from the reviewer who caught it: **append-only history protects claims about the world, not
    scaffolding about the document** — a false assertion (`1,843`, the invented `SKIP` line, the flat SHA bans
    above) can only be answered by a correction commit, but an **instrument** shown to be unsound is deleted, not
    annotated in perpetuity; mistaking the second for the first is how an artifact accretes defensive prose until
    it buries its own payload.

19. **A pattern anchored to the token after a delimiter silently misses order inversions — and an
    under-count that flatters your own conclusion is invisible to every downstream check.**
    [MEASURED 2026-11-05, T3] `audit/wave0/t2-provenance.txt` § 10 counted AI co-authorship trailers with
    `co-authored-by: (claude|openai|chatgpt|copilot)` and published **1,881 of 2,194**. That figure
    reproduces exactly, so nothing was miscalculated; what was wrong is that the alternation matches the
    token **immediately after the colon**. `Co-Authored-By: OpenAI Codex <codex@openai.com>` matches.
    `Co-Authored-By: Codex <noreply@openai.com>` — same family, order inverted — does not. Anchoring on
    the **domain** instead of the name gives **1,907 (86.9 %)** and **19,682 of 20,111 lines (97.9 %)**
    against the published 85.7 % / 94.6 %, and it exposed five tool families the pattern could never see
    (DeepSeek, opencode, Antigravity, a Qwen-named co-author, a 'Hermes orchestrator'). The general
    shape: whenever a classifier tests `key: (value|…)`, it is really testing *which word comes first*,
    so **write the pattern against the field that identifies the thing** (the domain, the path, the
    type) rather than the label a human happened to type first, and **run at least one order-inverted
    sample through it before publishing a count**. Two corollaries. (a) Verify the subset relation, not
    just the new number: computing `|old − new| = 0` is what proved the wider net was a strict superset
    rather than a different measurement trading one omission set for another. (b) The direction of this
    error *strengthened* this audit's own headline finding, which is precisely why it survived 11 commits
    and a sign-off — a count that argues against you gets re-derived, and a count that argues for you
    gets cited. So when a number agrees with your conclusion, re-derive the classifier, not the figure.
20. **A default value in an aggregation silently manufactures evidence — an absent API field scored as
    `0` reads exactly like a measured zero.** [MEASURED 2026-11-05, T3] Aggregating GitHub's
    `/pulls?state=all` response, a check written as `(p.get("comments") or 0) > 0` printed
    `with issue comments>0: 0/15` next to an empty `merged_by` map. Both looked like findings. They were
    not: the **list** endpoint does not return `comments`, `review_comments` or `merged_by` at all
    (verified with `key in obj` → `present=False` for all three), so `or 0` turned three *absent* fields
    into three confident zeros — and a zero is precisely the thing that reads as a result. Same species
    as item 15's phantom path, and the `merged_by: null` that reappeared later in the *same* task's
    `commits/<sha>/pulls` output. Three defences, all cheap: (a) **test field presence, not just value**
    — print `key in obj` for every field an aggregate reads, once, before believing any count built from
    it; (b) prefer the **dedicated endpoint** over a list projection when the list omits the field
    (`/pulls/N/reviews` is authoritative for review counts; the list is not); (c) when a field is absent,
    write the sentence as "the endpoint does not report X", never as "X is 0" — the first is a fact about
    an API, the second is a claim about the world. Corollary for **negative controls**: a control that
    "fires" on an absent field proves nothing, because the default produced the expected answer without
    any observation taking place.
21. **Take a denominator from the producer's schema, not from a guess about which column means what.**
    [MEASURED 2026-11-05, T3] To re-derive a published blame figure over "the 80 census files", the
    filter was written against `t2-author-census.tsv` **field 8**, which turned out to hold a top-author
    *name* (`Jesus Gulfo`); the status marker `BINARY_SKIPPED` lives in **field 13**. The filter returned
    **86** files, and 86 is close enough to 80 that the resulting percentages would have been quoted
    against § 10's 80 as if the populations matched. Two lessons. (a) **Print the column layout before
    writing the filter** — `head -1 | awk '{for(i=1;i<=NF;i++) print i, $i}'` plus one known-special row
    costs seconds and is the only thing that catches a wrong-but-plausible field index; a probe loop
    that only checks fields 2-10 will report "nothing holds this marker" for a marker in field 13, which
    is exactly what happened here. (b) **An error caught by unrelated luck still has to be recorded as an
    error**: this one surfaced only because `git blame --line-porcelain` then hit a non-UTF-8 byte in a
    binary and raised `UnicodeDecodeError`. Had that file been text, the 86-file denominator would have
    gone into a table silently. Never treat a lucky crash as a passing check.
## How to avoid
Never let absence-of-output or a trailing `rc` carry a verdict when a **number** can be printed:
`| wc -l | sed 's/^/PREFIX_DRIFT_COUNT=/'`, `awk 'END{print "ROWS=" NR+0}'`, and assert the expected
count *inside* the producer (`[ "$rows" = "862" ] || exit 5`) so the generator itself refuses to
produce a short artifact. Split `git` field records on a **tab** (`--format='%(objectname)\t%(path)'`
with `IFS=$'\t'`), never on a default-IFS space, and never with `IFS=`. Name artifact files so no glob
meant to count them can skip them — no leading dot, no extension collision — and keep a `find . -name
'.*tsv'` assertion in the check list. For a prefix/derived hash, write the recompute loop
(`cut -f1-4 | sha256sum` vs the recorded value) and keep it away from `sha256sum -c`. Finally: give
every new check a **negative control** before trusting its silence — T1's drift loop was only
believed after a tampered LOC digit made it report exactly one drift
(`context/audit/wave0/t1-freeze-and-manifest.txt`). Two refinements from the follow-up, both of which
came from controls that *failed*:

- **Control the checker, not just the data.** Nine tampers against a scratch copy of the manifest
  (tampered LOC, typo'd tier, short row, dot-prefixed TSV, symlinked TSV, trailing-space tier, stripped
  final newline) each had to exit non-zero; two of nine exposed a bug in the script instead. Record
  the transcript: `context/audit/wave0/t1-grid-controls.txt`.
- **Tie two zeros together instead of asserting two zeros.** `cat *.tsv | awk 'END{print NR}'` counts
  *records*, `wc -l` counts newline *bytes*; they can only differ if a file lost its final newline.
  Printing `RECORDS=862 NEWLINES=862` tells "a row went missing" apart from "a newline went missing",
  which `NO_FINAL_NEWLINE=0` and `ROWS=862` cannot. Print the **expected value as a literal**
  (`EXPECTED=862 ACTUAL=$rows`) and derive the actual, never the reverse: a literal `=ok` on an
  aggregation step fires green while the numbers are garbage.
- **Size stderr, and never let a check own a suppressed stream.** A check that hides stderr cannot
  tell "nothing to report" from "the command refused to run". Collect it and assert it empty
  (`ERR_BYTES=0`) — that one term is what would have caught item 8 on its first run.
- **Show a comparator non-zero at least once.** A `MISMATCH=0` from a comparison nobody has seen fail
  is decoration. Give every both-directions comparison a control input that must disagree: the pin's
  **first parent** for the tree check (it returned `BLOB_SHA_MISMATCH=1`, isolating one file —
  `serve/util/telemetry.py` — out of 862), a perturbed row for a row-level sentinel, an empty input for a row-count
  gate. And when a control does *not* fire, check what it was aimed at — the first attempt at the
  census sentinel perturbed row 3, which is a `BINARY_SKIPPED` row the check correctly ignores.
- **Paste, or label.** A `text` block in a `wave*/` artifact asserts *this ran, this came out*. Item 17's
  line was **predicted** output wearing a transcript's clothes — it survived because it was plausible and the
  precondition it asserted ("no `versions.env` yet") was not checkable from inside the artifact. So: write no
  transcript from memory; when one states a precondition, verify that precondition against the tree first;
  and when two records of one step disagree, **re-run the step** rather than adjudicate from recall — recall
  is what manufactured the fabricated "before" state in this task's own tracker note.
- **18. A lookup with an existence-gated early return can silently disable the probes that come after it.**
  [OBSERVED 2026-11-05] In `t3-model-file-poc.sh`, a probe that plants `~/.claude/settings.json` printed
  `wrote` while the file did not exist. Two probes whose `meta` bytes are identical resolve to **one**
  `cache_dir` (the key is the meta digest *as declared in the file* — that is finding F-2), so the second
  `extract_tokenizer()` takes `if (out / "vocab.json").exists(): return out`, writes **nothing**, raises
  **nothing**, and the caller scores a pass. The finding under test was switching off its own probes. The
  fix is structural, not careful: give every probe a unique cache key, and promote the collision to a named
  assertion (`F2_SAME_META_SHORTCIRCUITS_EXTRACTION=short-circuited`) so the mechanism is *proven live*
  rather than trusted. Generalise: whenever the code under test caches, assume later probes are no-ops until
  one assertion says otherwise.
- **19. A harness that reassigns `HOME` changes what `git` reports, not just where files land.**
  [OBSERVED 2026-11-05] The same harness overwrites `HOME` so absolute-path writes are sandboxed — correct,
  and the point of the guard. But `core.excludesFile` is `~/.gitignore`, so with the sandbox `HOME` git
  cannot see the operator's global ignore file and `git check-ignore` answered **untracked** for
  `.claude/settings.json`, a path that is genuinely **IGNORED** (`~/.gitignore:16 /.claude`) in the real
  environment. One command, two answers, because the *environment* moved and only `$HOME` was thought about.
  Since that column is the tamper-visibility evidence a 🔴 rating leans on: any probe that shells out to a
  tool with per-user config (`git`, `gh`, `ssh`, package managers) must **pin the config it means to
  measure** — here, `HOME="$REAL_HOME"` on each git call, plus `--no-optional-locks` so a read-only query
  cannot refresh the index of somebody else's repo.
- **20. `git diff --quiet <pin>` proves tracked contents, not an untouched tree — and inside a gitignored
  directory it proves almost nothing.** [OBSERVED 2026-11-05] `server/basalt/.gitignore` line 1 is
  `/vendor/`, so every file created inside `vendor/basalt` is untracked *and* ignored, and `git diff <pin>`
  — which compares tracked contents against a commit — cannot see one of them. This audit's own PoC
  harness ran with `cwd=vendor/basalt` and imported `serve.*` and `tools.model_file`, so CPython wrote
  **13 `__pycache__` files into the tree under audit**, and a string of `AUDIT TREE OK` lines was true and
  useless the whole time. It surfaced only as `t3-census.sh`'s universe hash: **875 files against a pinned
  862** (and LOC +999, because a LOC pass counts newline bytes and `.pyc` files are full of them). So a
  freeze check is **three legs**: `git diff` for contents, a **file count** against the frozen number, and
  `git status --porcelain --ignored` being **empty** — the last one names the offending paths, which a
  count cannot. And any tool that imports its subject sets `PYTHONDONTWRITEBYTECODE=1` *and* asserts
  afterward that it left nothing: the harness now exits **3** on `POC_TREE_BYTECODE>0`, because "the
  experiment modified its subject" is a different failure from "an assertion failed" and earns its own
  exit code. What did *not* save us: 32 named assertions all passed on the contaminated tree.
- **21. `ls` lists its own redirect target.** [OBSERVED 2026-11-05] `ls "$FIX" | sed … > "$FIX/fixlist.tmp"`
  creates `fixlist.tmp` before `ls` runs, so the listing contains the file being written. In
  `t3-census.sh` that inflated `FIXTURE_FILES` by one and handed `xargs` a path the following `mv` had
  just deleted. The worse sibling is writing the list *inside the corpus it describes*: on the next run
  the list is a corpus member, and here its absolute paths were greppable by the very family patterns
  being scored against it. Use `find … ! -name 'fixlist*'`, and keep derived lists outside what they
  enumerate.

## Sources
- basalt: `context/audit/manifest/index.md` (the checks, as corrected) and
  `context/audit/wave0/t1-freeze-and-manifest.txt` (the control output)
- basalt: `context/audit/scripts/ledger-grid.sh` + `context/audit/wave0/t1-grid-controls.txt`
  (items 5–7: the generator and the nine controls that caught them, 2026-11-05)
- basalt: `context/audit/scripts/t2-census.sh` + `context/audit/wave0/t2-provenance.txt` § 4
  (items 8–9: the `rev-list --follow` empty column and the disjoint-set zero, plus the five controls
  that now cover them — C1 parent-tree, C2 empty input, C3 sentinel, C4 empty field, C5 row count)
- basalt: `context/audit/wave0/t2-provenance.txt` §§ 6, 9, 10 +
  `context/audit/wave0/t2-lineage.txt` § 0 (items 10–13: the `/get/` endpoint sweep that returned 404
  for everything, the abbreviated-SHA probes, the `%H%x00%B` record-split that reported 0 AI-trailed
  commits out of 367, and the unanchored `boundary` grep that published 0 where the anchored count is
  560 — each refuted by its own control, all four *after* being drafted for the record, 2026-11-05)
- basalt: `context/plans/02_basalt_vendor_audit_tracker.md` (T1 Notes, 2026-11-05)
- related: `context/pitfalls.md` § *An empty result from a command that was never allowed to run*

# Cross-repo git forensics: three commands that answer a different question than they look like

## Problem
[OBSERVED 2026-11-05, Plan 02 T2] Establishing "is this commit also upstream's / where did this fork
come from" against a second repository. Three methods were run, all three produced confident answers,
and all three were measuring something else. The stakes: the first two built a headline ("basalt and
Strata share history to ≈2026-10-04") that was false, and it was false in the direction that would have
made a fork look better-attested than it is.

1. **Asking a peer repo's commit API for our SHAs.** `GET /repos/<peer>/commits/<sha>` returning `200`
   for five of our pre-Oct-2026 commits and `422` for others looked like a fork boundary. It is not one:
   **GitHub resolves that route against the repository's object store, not against its default branch**,
   so a `200` means "this object exists over there", including on refs nobody looks at. Combined with an
   upstream that has rewritten its mainline, the predicate "on their `main`" was never evaluated at all.
2. **Counting or probing objects inside a partial clone you have already queried.** The same session's
   first Strata fetch (`--filter=tree:0`) showed **6,347** commit objects against `main`'s 5,637, and the
   710 extras matched our clone exactly — "upstream still serves the deleted line!" They were objects my
   *own* `git log` and `cat-file` calls had **lazy-fetched on demand** from our side. A pristine clone of
   the same ref contains **5,637 and 0 shared SHAs with us**. In a promisor repository, the object store
   is a log of what you asked for.
3. **`git log <upstream>/<branch>..<our-pin>`, `git cherry`, cross-repo `bisect`, blame continuity.**
   These *run* on two repositories with no common ancestor and print a confident, wrong answer: with no
   merge-base, `A..B` is just `B`, so the command reports our entire history as new work and upstream's
   changes as none. It does not error. (`git diff A B` is the exception worth knowing: it is tree-to-tree
   and remains sound — our two root commits diff to nothing under it.)

## How to avoid
- **Best test when the question is about two repos: fetch BOTH into one scratch store** with the same
  `git fetch --filter=tree:0 --no-tags <remote> main`, then compare inside it —
  `comm -12 <(git rev-list A|sort) <(git rev-list B|sort)` answers "any shared commit object?" directly
  (ours: `|A|=2194  |B|=5637  |A∩B|=0`), and the store self-certifies: `in-pack` 7834 = 2194 + 5637 + 3
  tag objects, nothing else in it. Raw record: `audit/wave0/t2-lineage.txt` § 11b. Two traps inside the
  method itself: `FETCH_HEAD` is overwritten by the next fetch (name the ref after **each** one — first run
  here compared Strata against itself that way and had to be discarded), and **`git merge-base` between
  unrelated histories exits 1 and prints NOTHING**, so `mb=$(git merge-base A B)` is silently empty. The
  probe that means what it says is the `rev-list` **intersection**, with `|A|` and `|B|` printed next to
  `|A∩B|` (an empty intersection is also what a *failed* `rev-list` gives, so the marginals are the proof).
  **Do not use `merge-base --independent` as an unrelatedness test**: it answers "does either input reach
  the other", and `main` vs a feature branch that plainly shares ancestry returns 2 exactly like two
  unrelated repos do.
- **Clone instead of asking.** `git init p && git -C p remote add origin <peer-url> && git -C p fetch
  --filter=tree:0 --no-tags origin <branch>` is seconds and a few MB, needs no API quota, and gives a
  real object store. **Verify pristine before believing a count**: `in-pack` before/after
  (`0` → 5,637), no `objects/info/alternates`, single-branch refspec.
- **To ask whether a peer serves one specific object, use the promisor as the oracle with a FULL
  40-hex OID**: `git -C <pristine-partial-clone> cat-file -e <sha>^{commit}`. A miss triggers a real
  `upload-pack` request, and `fatal: remote error: upload-pack: not our ref <sha>` is an object-level,
  quota-free answer. Scope what it proves: *"not reachable from any ref this remote advertises"*, **not**
  "never in the store". Abbreviated OIDs fail locally with a lookalike error (see the item above).
- **Match content, not hashes, when histories were rewritten.** Join on `(tree, author-date,
  committer-date, subject)`, and for files on blob SHA; add `git patch-id --stable` for changes. Our two
  repositories share **0** SHAs and **844** content-key pairs — one history, two variants. Always print
  `|A|`, `|B|`, `|A∩B|` and the per-side duplicate-key count so the join's shape is visible, and check
  uniqueness *before* quoting the intersection.
- **Say which repository a version string belongs to.** Basalt and Strata both publish `v0.1.0`/`v0.2.1`
  pointing at **different commits**, and Strata publishes 95 tags to basalt's 3. An unqualified `v0.2.1`
  in any plan or note is ambiguous.
- **Expect published history to move.** An upstream that force-pushes makes "diff against upstream" an
  unstable operation; anchor comparisons to a named snapshot hash and a date (ours: Software Heritage's
  2026-10-05 visit of Strata, whose recorded `refs/tags/v0.1.0` is not the one there today).
- **And a related trap inside one's own clone:** commit-object counts can exceed `rev-list <pin>` when
  the repo publishes other refs. Ours: 2,299 objects vs 2,194 commits from the pin — the 105-object gap
  is `refs/remotes/origin/multimodel`, i.e. **an audited repo can publish a whole branch outside the
  audited ref**. Enumerate refs (`git for-each-ref`) as part of any freeze, and check
  present-but-unreachable is 0 (`cat-file --batch-all-objects` set-minus `rev-list --all`).

## Sources
- basalt: `context/audit/wave0/t2-lineage.txt` (§ 0 refuted hypotheses R1–R2, § 1 sound method, § 2 the
  2×2 remote-presence matrix, § 4 the content-key corpus, **§ 11b both histories in one object store — the
  direct `|A∩B|=0` measurement and the silent-`merge-base` record**)
- basalt: `context/audit/manifest/index.md` § *What "basalt ← Strata" actually means*
- basalt: `context/pitfalls.md` § *Shell idioms that report green…* items 10–11 (the all-404 endpoint
  sweep, the abbreviated SHAs)
- mirrored for reuse across the monorepo: `../../../context/pitfalls.md` § *Cross-repo git forensics*
- observed during Plan 02 T2, 2026-11-05, against `Niko1221/Strata` and `jesdga95/basalt`


---

# Provenance that cannot contradict its own claim

## Problem

**[OBSERVED 2026-11-05]** Plan 02 carried a **P0 finding for days** on a version number that was
wrong, and the wrongness survived *two* separate reviews — including one whose whole subject was
whether the version claim was right. The claim was "`stb_image.h` is 2.21 (2019-02-25), and the
missing `STB_IMAGE_RELEASE` macro is the version evidence." The file is **2.30**. Three failures
compounded, and none of them is "didn't check":

1. **A line range was used as a version oracle.** `sed -n '60,62p'` printed
   `2.23 / 2.22 / 2.21` — the *middle* of a descending `RECENT REVISION HISTORY` block whose top
   entry is `2.30` nine lines above. A **range silently relocates when the premise is wrong**; a
   pattern either matches or the claim dies loudly. The tell was structural and sitting in the
   quoted output: **a 2.21 copy cannot list 2.23 below the ceiling.**
2. **A fact was invented to fit the conclusion.** `STB_IMAGE_RELEASE` is not a macro in *any*
   stb_image release — stb records its version only in the banner comment and the changelog. A
   0-hit grep on it is exactly as consistent with 2.09 as with 2.30: **zero information.** The
   only version-ish macro in the file is the unrelated ABI constant `STBI_VERSION 1`.
3. **Negative results were drawn from invented identifiers.** Two of five "discriminating"
   greps — `stbi__vertical_flip_pixels`, `stbi__jpeg_decode_block_to_dc_values` — **exist in no
   stb release**. The real names are `stbi__vertical_flip` (7 hits) and
   `stbi__jpeg_decode_block_prog_dc` (3). A 0-hit grep on a paraphrased name "confirms" anything,
   and five greps that all agreed with 2.21 looked like convergent evidence. It was one wrong
   premise echoed.

The shape to notice: every number in that chain *could not contradict the claim attached to it*.
That is this audit's recurring defect, not this one bug — the false-zero `grep boundary` over
blame output and the all-404 endpoint sweep are the same shape.

## How to avoid

- **Identity claims quote a pattern whose match *is* the claim.** Never a bare line range.
  Right: `grep -n 'stb_image - v' src/vision/stb_image.h` → `1:/* stb_image - v2.30 …`. Wrong:
  `sed -n '60,62p'`. A claim that needs a hardcoded offset is a claim about a coordinate, not
  about the artifact, and coordinates move under you.
- **Negative claims are the expensive ones; show the token and the 0 hits.** Any
  "missing / absent / unfixed / because it is old" assertion must name the identifier it grepped
  for and prove the identifier is *real* — confirm against the file's own vocabulary, not from
  memory of upstream. `psize == 0` at `:5564`/`:5601`, `extra_read` at `:5419`, and the
  `IDAT section larger than 2^30` string at `:5182` each return hits and would have collapsed the
  whole CVE table before it entered a plan.
- **Ask whether the quoted output contradicts the sentence above it.** Half a line, every time.
  Descending lists, version tables and sorted histograms are where this hides: the artifact
  bounds itself.
- **Never write a bare version number without the command that produced it.** This entire error
  was one un-sourced line range, quoted confidently, twice.
- **Prefer code markers to comment banners** when a version matters. Comments can be edited;
  `#define STBI_MAX_DIMENSIONS (1 << 24)` at `:796` and `stbi__pnm_is16` at `:7624` are
  introduced-at-2.27 facts that no re-labelled banner can fake.

## Sources

- the corrected claim and its evidence: `context/plans/02_basalt_vendor_audit.md` § *Pre-Identified
  Bug/Issue* **1** (rewritten 2026-11-05, with a dated note preserving the falsified version so
  nobody re-derives it)
- the reasoning that caught it: `context/plans/02_basalt_vendor_audit_tracker.md` § Notes/Deviations,
  the two dated 2026-11-05 stb entries (the second corrects the first)
- related items in this file: § *Shell idioms that report green…* items 10–11 (the all-404
  endpoint sweep, abbreviated SHAs) and the blame false zero recorded during T2
- observed during Plan 02 T3, 2026-11-05, while correcting a P0 — `vendor/basalt` @ `dc047a7`

---

# An apostrophe in a comment inside a single-quoted `awk '…'` body is still inside the quotes

## Problem

**[OBSERVED 2026-11-05]** `context/audit/scripts/t3-census.sh` carries a ~40-line `awk` program
written as a single-quoted argument. Adding an English comment *inside the body* — `# rows that do
not have a site row` — closed the quote early, so bash resumed parsing the rest of the awk program as
shell. Cost: two `bash -n` cycles nine minutes apart in the **same block** (first at line 1056, then
at 1120) and one wrong diagnosis.

What makes it non-obvious is where it hid and what it pointed at:

- The offending character was in a **comment**. Prose inside a quoted program is still inside the
  quotes; nothing about `#` makes a byte inert to the shell that is quoting it.
- `shellcheck` reported **`SC1011: This apostrophe terminated the single quoted string`** against a
  line of *prose*, three screens away from the construct that broke. That reads like a false positive
  about punctuation and is not one — it is the exact defect, in a file that is otherwise clean.
- `bash -n` was precise (`syntax error near unexpected token 'obs'`) and both times it **failed
  loudly**. It never produced a plausible-but-wrong count, which is why this is not an instance of
  § *Shell idioms that report green on the thing they did not check*.

The worse cousin has **not** been observed here and is recorded as a hazard only: an apostrophe that
merely re-splits the body into tokens that are still valid `awk` (a comment word absorbed into a
concatenation, say) parses clean and computes something different. That variant would go green.

## How to avoid

- Grep anchors for this trap: `awk '`, `apostrophe`, `SC1011`, `bash -n`.
- **No apostrophes inside a quoted `awk`/`sed` body — comments included.** Write `do not`, `it is`,
  `cannot`. Same rule for `case` arms and here-doc-free `-c` programs.
- `shellcheck` `SC1011` landing on a comment **is the tell**, not noise to suppress.
- `bash -n` is free, takes a second, and is already gate step 1a — run it after *every* edit to a
  quoted multi-line program, not once before the commit. It caught both instances; nothing else did.
- For prose-heavy `awk`, stop inlining: write the program to a file and use `awk -f prog.awk`, or a
  quoted here-doc fed to `awk -f /dev/stdin`. Then apostrophes are just English.

## Sources

- both instances: `context/audit/scripts/t3-census.sh`, 2026-11-05, during the defect-4 labelling
  edit (the block that emits the three-state column-8 labels)
- related: § *Shell idioms that report green on the thing they did not check* — this is the same
  quoting family, but it failed closed, and recording which of the two it did is the useful part

---

# A control run under a real tag wipes the baseline every later compare reads

## Problem

**[OBSERVED 2026-11-05]** The census harness keeps a per-tag namespace,
`/tmp/opencode/basalt-audit/t3/run-$TAG`, and **`rm -rf`s it as its first action**. `run-real` is not
just an output directory: it holds the four freeze tokens (`generator`/`predicate`/`inputs`/`universe`)
that every cross-run assertion reads as its *expected* leg.

Two ways to destroy it, both by running a **control**:

1. **Tag reuse.** One assertion — `SENTINEL_TOTAL_AT_OR_ABOVE_FLOOR` — is gated on
   `[ "$TAG" = real ]`, so the only way to fire it is to run a mutated copy under `tag=real`. It ran,
   failed correctly, and in doing so replaced `run-real/generator.sha256` with the **mutant's** own
   hash (`b75fd2f0…`). Eight subsequent controls then printed that contaminated value as the
   "expected" side of `GENERATOR_SHA_SAME_AS_REAL`. The FAILs were still valid — a mutant differs from
   another mutant — but every *baseline* figure from that window is poisoned.
2. **Dying after the wipe.** An earlier batch launched its copies from the wrong cwd, so the script
   resolved `MAN`/`PROJ` to `/tmp/…` and died at `MANIFEST_DIR_NOT_FOUND`, **exit 2**. Because the
   `rm -rf` is the *first* statement and the failure is twenty lines later, a run that never produced
   an assertion still deleted `run-real`. The next batch printed
   `TREE_DIFF_VS_REAL=unavailable (run-real/universe.txt missing…)`, which reads like dead code in the
   harness and was in fact my own artifact from four minutes earlier.

The reusable shape: **the destructive statement precedes the validation that would have caught the
mis-launch.** A control is allowed to fail; a control that silently removes the reference for the
controls after it is not.

## How to avoid

- **Controls get their own tag namespace, always.** No control ever runs under the tag whose artifacts
  it compares against. A `real`-gated assertion is executed **last, in a batch whose output is
  discarded** — or not at all, with the gap disclosed.
- **A restore is a re-production, not an edit.** `run-$TAG` is regenerable by design, so the claim is
  never "I salvaged it" but *"a pristine re-run reproduces the pasted evidence byte-for-byte"*:
  `diff` the new stdout against the paste, excluding the lines that are timestamps by construction
  (and paste those two lines side by side, so the exclusion is checkable rather than asserted).
- **Verify the whole token set, not the two you happened to think of.** The first restore proof checked
  `generator` and `sites`; `predicate` and `inputs` — the expected legs of the *other* two canary
  compares — had been overwritten too. Enumerate the artifacts, then verify all of them.
- A mis-launched control is a **broken control, not a fired one** — exit 2 means nothing was asserted.
  Treat it as an incident: it may still have written, or deleted, something.
- Related failure already in this file, same root (wrong cwd, empty inputs, a check that inherits the
  emptiness): § *Shell idioms that report green on the thing they did not check* items 3 and 20. This
  entry adds only the destructive half.

## Sources

- the controls and the two incidents: `context/audit/wave0/t3-census-controls.txt` § 3 and § 4
  (including the byte-diff restore anchor and the four-token verification)
- observed Plan 02 T3, 2026-11-05, `vendor/basalt` @ `dc047a7`

---

# An LC_ALL naming a locale the host does not have falls open to C — no warning, no error

## Problem

**[OBSERVED 2026-11-05]** `locale -a` on this host lists exactly `C`, `C.utf8`, `POSIX`. Setting
`LC_ALL=en_US.UTF-8` and running `sort` produced `B a b` — **identical to `LC_ALL=C`** — with no
warning, no error, and no exit code. glibc resolves an unknown locale name to C and keeps going.

The first casualty was the census assertion `SORT_DEFAULT_ORDER_EQ_C`, whose whole purpose is to catch
a non-C collation being exported into the environment. Its realistic trigger **cannot be produced
here**: there is no non-C collator to export. So the chk is green for a reason unrelated to what it
guards, and — worse — **a genuine regression passes**: drop `LC_ALL=C` from a `sort` line and, on this
box, the output is unchanged and the assertion still prints PASS.

Two scope limits on this entry, both stated rather than smoothed over:

- What was measured is "the output is C-identical". This did **not** separate *locale unavailable, so
  glibc falls back to C* from *locale present with an identical collation sequence* — the observable
  here cannot distinguish them, and `locale -a` is the only evidence for the "unavailable" reading.
- The chk's liveness was proved by a different mutation: feeding the comparator reverse-C order
  (`actual=bBa`, which is neither C's `Bab` nor a plausible non-C `aBb`). That proves the **comparator
  is live and compares what it claims**; it does **not** prove the comparator is collation-sensitive.
  The locale arm remains unproven on this host — say so before treating the chk as coverage.

Seeded hazard, **not observed** here: a `comm -23` / `sort -u` pair whose two sides were sorted under
*different* collations can silently agree, or silently disagree, because each side is internally
consistent. Keep `LC_ALL=C` on **both** sides and inside the script, not only at the call site.

## How to avoid

- Grep anchors: `LC_ALL`, `locale -a`, `sort -c`, `comm -23`, `C.utf8`.
- **`locale -a` before you arm a locale-dependent tripwire.** If the trigger locale is not installed,
  the check is decorative and says so only to the person who thought to ask.
- When a chk's realistic trigger cannot be produced on this machine: prove liveness another way,
  **label that proof for what it covers** (comparator vs export), and record *which arm is unarmed* —
  in the script comment *and* in the evidence file. Do not let a PASS read as an armed check.
- Do not install a locale just to make a check exercisable: that changes the environment the check is
  supposed to police, and the next agent inherits a tripwire only your box can fire.
- Pin collation **inside** the script (`export LC_ALL=C` at the top, as `t3-census.sh` and
  `t3-inputs.sh` both do) rather than trusting the invocation, and note in the comment that on this
  host the export is load-bearing but *unverifiable by that assertion*.

## Sources

- measured: `locale -a`; `printf 'a\nB\nb\n' | LC_ALL=en_US.UTF-8 sort` → `B a b`; the fired comparator
  control is `t3-census-controls.txt` **C7**, whose `bBa` is reverse-C and must not be read as locale output
- the assertion: `context/audit/scripts/t3-census.sh` `SORT_DEFAULT_ORDER_EQ_C`
- the host-shaped half, pruned of this box's locale count, is copied to
  `../../../context/pitfalls.md` — glibc's silent fall-open is not a basalt fact
- observed Plan 02 T3, 2026-11-05

---

# A dated fact scoped to a shell I could not enter was declared "stale" — the scope caveat was on the adjacent line

## Problem

**[OBSERVED 2026-11-05]** Grep found `python3 3.14.10` in three files (`RULES.md:456`, this plan's
tracker `:37`, `distilled.md:162`). My shell reports `3.12.16`. I told the human — in prose, in the
conversation, one full turn — that **a false fact was sitting in the project's memory**, and started
writing the correction.

Then I read `±10` lines around the `distilled.md` hit. Its section heading is
`## This host: one sm_120 card, 125 GiB RAM [LIVE 2026-11-04]`, and **one line below** the number sits:

> **Tool versions here are the 2026-11-04 shell's view, not a host constant** (checked 2026-11-05 from
> the agent container, which reports docker client 29.9.2 …). Re-check in the shell that will use the
> answer.

The record was correct, dated, environment-scoped, and carried instructions for exactly the situation I
had walked into. Nothing was stale about it. The write was averted; **the false claim about the
project's memory had already been published**, and that ordering — assert first, read second — is the
bug, not the near-miss.

The same line's sibling number proves the caveat is load-bearing rather than decorative: it records
`docker 29.8.1` for 2026-11-04, and `docker --version` here reads **29.9.2**. But that cuts both ways,
and this is the part worth internalising: **my fresh number is not a *better* number.** It is today's
shell's view in precisely the way theirs was the 2026-11-04 shell's view. The "correction" I proposed
would have swapped one environment's snapshot for another's and destroyed the older one to do it.

Severity, stated plainly because it is the strongest argument: **this error is silent and leaves no
trace.** Overwriting a scoped measurement with a value that is true of *your* shell produces a file
that reads **more** accurate than before — no later check, control or reader ever fails on it. Leaving
a stale-looking number in place keeps the defect visible. Prefer the visible one.

Two related errors from the same five minutes, both about *inference* rather than reading:

- A reviewer proposed that `3.14.10` was "implausible as a released CPython patch" and that the string
  was therefore a transcription slip. Checked against python.org: **3.14.10 was released Oct. 25, 2026**
  (3.14 is in bugfix maintenance). The inference was plausible, confident, and wrong; the primary
  source is cheap. An unsupported plausibility argument would have manufactured evidence.
- The one genuine defect in the three hits was `RULES.md:456` — the only copy with **no date and no
  environment named**, sitting in a gate *Prerequisites* line, i.e. the sole copy that actually
  asserted universality. See § *A gate prerequisite recorded in RULES.md can silently stop being true*.

## How to avoid

- Grep anchors: `stale`, `outdated`, `correct`, `dated`, `[LIVE`, `host constant`, `scope`.
- **Before naming any recorded fact stale, read the enclosing scope — the `±10` lines, not the hit
  line.** A date tag, an environment name, or a "not a constant" clause on an adjacent line changes
  what the number claims, and therefore whether your measurement can contradict it.
- **A grep for a value returns lines; scope is not on the line.** A grep-hit list is *candidate sites*,
  never a list of facts to correct. This is the mirror of the doctrine in `plans/RULES.md` § *Rule E —
  Evidence over assertion*, item 7 ("grep for the claim, not the phrasing") applied to **values**
  rather than wording: grep finds copies, **scope decides whether a copy is wrong**.
- **Correction test:** ask which environment the record claims to describe. If it is not yours, you may
  only **append a newer dated observation with its own scope** — never replace. Supersede in place:
  keep `29.8.1 [LIVE 2026-11-04]`, add `29.9.2 measured 2026-11-05, agent container`, leave both.
- Fix the **unscoped** copy; leave the **scoped** ones alone. A dated, environment-tagged number makes
  no claim about your shell, so your shell cannot refute it. Only an unqualified one can be corrected.
- If you must keep an old number you cannot re-derive, record *that* as the datum — "environment not
  named in the original; not re-derivable" — rather than silently assigning it one.
- **Say the finding, then verify it, then say it again.** A confident claim about the project's memory
  that turns out to be wrong is worse than a late correct one. The retraction is the cheapest part of
  this entry; publishing was not.
- Cross-reference: § *Provenance that cannot contradict its own claim* is the **instrument** defect
  (the evidence could not contradict the claim attached to it). This is the **reader** defect — the
  provenance system worked perfectly and I overrode it anyway. Kept separate on purpose.

## Sources

- the correctly-scoped record I nearly destroyed: `context/distilled.md` **`:158-166`** — heading
  § *This host: one sm_120 card, 125 GiB RAM* `[LIVE 2026-11-04]`, the number at `:162`, its own caveat
  at `:163-166` (left untouched; today's python reading appended there as one datum). Cited by line
  because that file has **two** headings beginning `This host: one sm_120 card` (`:136`, `:158`), so a
  section-name citation to "§ *This host…*" does not identify one of them.
- the unqualified copy that *was* wrong for this shell: `context/plans/RULES.md` § *Prerequisites*,
  corrected 2026-11-05 to a measured value with the environment named
- the refuted inference: python.org download history, retrieved 2026-11-05
- the rule this generalises into: `context/AGENTS.md` § *Provenance* — a dated observation scoped to an
  environment you cannot enter is superseded, never replaced
- observed Plan 02 T3, 2026-11-05, while preparing `wave0/t3-sentinels-real.txt`
