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
- basalt: `context/plans/01_flashnext_container.md` § Gate status (the `ss` incident, 2026-11-05)
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
- basalt: `context/plans/01_flashnext_container.md` § Gate status (added 2026-11-05 for exactly this)

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
- basalt: `context/plans/01_flashnext_container.md` § T6 (the probes as specified), § Gate status
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
- basalt: `context/plans/01_flashnext_container_tracker.md` § Notes / Deviations, "Two writers, one
  worktree", 2026-11-05
