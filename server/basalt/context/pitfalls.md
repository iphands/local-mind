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
the socket is `root:rtkit 660` and the session is `uid=1000 groups=1000,27`, not in group `docker`
(gid 991). The real error — `permission denied while trying to connect to the docker API at
unix:///var/run/docker.sock` — went to **stderr**, and `grep` on stdout turned the failure into a
confident-looking empty result. It surfaced only when `PREFLIGHT_ONLY=1 ./build` exited 1 and named
the permission error. The same class produced a second over-claim: `git cat-file -e <blob>` "proving
every blob is local" was run **without `GIT_NO_LAZY_FETCH=1`**, so it may have succeeded by fetching
from origin mid-check — "was already local" and "fetched on demand" are indistinguishable in that
output. Both Key Facts were downgraded in the plan with their limits stated.

## How to avoid
**An empty result from a command that was never allowed to run is not a measurement.** Three habits:
(1) end every measurement command with an unconditional sentinel and read it before the output —
`; echo "— done rc=$?"`; (2) never trust a pipeline's emptiness when the tool at its head can fail
on permissions — check `2>&1` once, or the exit status of the *first* command, not the pipeline's
last; (3) for git in a partial clone (`promisor=true` / `blob:none`, which is how `./build` clones
`vendor/basalt`), put `GIT_NO_LAZY_FETCH=1` in front of anything that asks whether an object is
already here, and remember `git cat-file -e` proves object **existence**, never that the working file
matches it — only `git hash-object -- <path>` compared to the blob id does. In an audit whose whole
purpose is proving absence, this is the failure that matters most.

## Sources
- basalt: `context/plans/02_basalt_vendor_audit.md` (Key Facts — the two downgraded rows)
- basalt: `context/plans/02_basalt_vendor_audit_tracker.md` § Notes / Deviations, 2026-11-05

---

# A gate prerequisite recorded in RULES.md can silently stop being true

## Problem
[OBSERVED 2026-11-05] `context/plans/RULES.md` § Project gate step 2 states *"`shellcheck` 0.11.0 is
installed (`/usr/local/bin/shellcheck`, 2026-11-04)"* and that a missing tool is **fail, not skip**.
On 2026-11-05 there was no `shellcheck` at that path, on `PATH`, or in `~/.local/bin`, `~/bin`,
`~/.cargo/bin`, `~/.bun/bin` or `~/.pyenv/shims`. Step 2 therefore failed for a markdown-only commit
that could not possibly affect it — which, left unexplained, reads to the next session as *"my change
broke the gate."* The same session hit the docker half: gate steps 3–6 are unrunnable because the
agent is not in group `docker`. Two of the project's six gate steps were red for reasons no file said.

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
