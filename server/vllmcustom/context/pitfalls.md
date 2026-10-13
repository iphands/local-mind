# Pitfalls & Gotchas — vllmcustom

Every bug, gotcha and trap — **especially anything that took more than one attempt to fix.** Read
before new work. Tag entries that actually bit `[OBSERVED YYYY-MM-DD]`; untagged entries are
hazards seeded from reading, not incidents.

Cross-cutting entries also go up to `../../../context/pitfalls.md`; project-specific ones stay here.
(The parent context is empty as of 2026-10-13 — create it only once a lesson is genuinely not about
this project.)

# Extensionless scripts are Python, and `bash -n` passes them silently [OBSERVED 2026-10-13]

## Problem
`bench/bench`, `bench/make-test-convo`, `scripts/decode-rate` and `scripts/qwen-conc-report` have no
extension and are Python. A file set built by glob or by "everything executable" therefore fails in
**both** directions: `bash -n bench/bench` dies with
`syntax error near unexpected token \`0,'\` (a false failure that looks like real breakage), while
`bash -n` on a `.py` file that *is* bash-free exits 0 and reports a clean bill of health it never
checked (a false pass — this one is the dangerous direction, because the gate goes green).

## How to avoid
Classify by **shebang on line 1**, never by extension, glob or mode. Gate step 1 of
`context/plans/RULES.md` does exactly that. Exclude `*.pth` — those are import shims
(`container/patches/*/*.pth`), not lintable source. And always print the count: a file list that
comes back empty makes every loop in the gate pass.

## Sources
- vllmcustom: `bench/bench`, `scripts/decode-rate` (shebang `#!/usr/bin/env python3`)
- vllmcustom: `container/patches/*.pth`

# Docker reaches nothing without sudo, and it fails as "no images" [OBSERVED 2026-10-13]

## Problem
`docker images` as the user prints
`permission denied while trying to connect to the docker API at unix:///var/run/docker.sock`.
Worse, several call sites swallow it: `require_image` in `common/_run-lib.sh` ends in
`docker image inspect … || { echo "Image … not found. Run ./container/build first."; exit 1; }`,
so an access failure is reported as a **missing image** and sends you off to rebuild an image that
is sitting right there.

## How to avoid
Prefix every docker call from an agent shell with `sudo -n`. When a script claims an image is
missing, confirm with `sudo -n docker images` before believing it — the error text is about the
socket, not the tag.

## Sources
- vllmcustom: `common/_run-lib.sh` (`require_image`), `container/archs`, `container/push`

# `PREFLIGHT_ONLY=1 ./container/build` force-checks-out trees the owner edits by hand [REPO]

## Problem
The documented "checks the pins in seconds" command first runs `sync_src` on all four trees under
`$BUILD_DIR/src`, each ending in `git checkout --force` plus a recursive submodule update. The
README explicitly invites working in those trees ("Patch or `git checkout` in place to iterate"),
so the routine-looking check destroys uncommitted hand edits — outside the repo, so `git status`
never warned you.

## How to avoid
For a routine pin check use the read-only form (gate step 4): export `SRC_DIR` and the version vars
from `container/versions.env` and call `./container/preflight` directly — it reads
`git show "$VLLM_REF:requirements/cuda.txt"`, which needs no checkout. Run the `container/build`
form only as part of a pin change, and only after this prints nothing
(`--untracked-files=no` because flashinfer legitimately carries an untracked `3rdparty/nccl/`):

```bash
for t in pytorch flashinfer vllm torchaudio; do
  git -C /mnt/noir/scratch/ai/vllm/build/src/$t status --porcelain --untracked-files=no \
    | sed "s/^/DIRTY $t: /"
done
```

## Sources
- vllmcustom: `container/build` (`sync_src`), `container/preflight`
- vllmcustom: `README.md` § *Caching — why builds aren't from scratch every time*

# Shared launcher state breaks silently, not loudly [REPO, seeded from reading]

## Problem
`common/_run-lib.sh` documents three rules whose violation produces **no error**: appending anything
after `env_extra_args` / `vllm_extra_args` makes the caller's `ENV_EXTRA` / `EXTRA_ARGS` override
stop working with no message; calling a core builder after appending throws the additions away; and
a bare (non-`--`) token appended to `VLLM_ARGS` is swallowed as an extra `--served-model-name`
alias rather than read as a positional. Every one of those yields a server that starts and is
configured differently from what the launcher appears to say.

## How to avoid
Follow the call order printed at the top of that file. The cheap proof is the launcher's own echo:
`vllm_launch` prints the complete `docker … serve …` line before running it, so diff that line
against the same launcher's output on `HEAD` after any plumbing change. The strongest version of
this check is a `DRY_RUN=1` short-circuit in `vllm_launch` and a golden-file diff — proposed as the
first plan, not yet true.

## Sources
- vllmcustom: `common/_run-lib.sh` (header comment, `vllm_launch`)

---

# `| head` in a `then` branch eats your own error message and changes the exit code [OBSERVED 2026-10-13]

## Problem
The docker reachability probe in `common/_run-lib.sh` printed the daemon's own output with
`sed 's/^/##   it said: /' <<<"$_DOCKER_PROBE" | head -3`. Three things are wrong with that line, and
two of them are invisible:

1. `head -3` exits after three lines and closes the pipe → `sed` is killed by `SIGPIPE` (status 141) →
   `pipefail` makes the *pipeline* fail → **errexit is live in a `then` branch** (`if` suppresses
   errexit for the *condition* only, never the body) → the script dies at that line, so every hint
   line below it never prints, and the launcher exits **141 instead of 1**. A bench harness reads 141
   as "the launcher crashed", not "the launcher refused".
2. It only triggers when the captured text is longer than three lines — i.e. on the *chatty* errors,
   which are exactly the ones worth reading. `sudo`'s own chained refusal is three lines by itself.
3. `docker version` writes its whole 8-line `Client:` block to stdout **before** it fails on the
   socket, so with `2>&1` the first three lines are always `Client:` / `Version:` / `API version:`
   and the actual diagnosis (`Cannot connect to the Docker daemon…`) is last. `head` reads the wrong
   end, in the one case the message exists for.

## How to avoid
In a diagnostic path, never put an **early-exiting consumer** (`head`, `head -c`, anything that quits
before EOF) downstream of a command inside a pipeline that runs under `set -eo pipefail`. Either
capture, then print (`_t=$(tail -3 <<<"$x"; ); printf '%s\n' "$_t"`) or let the tool exit itself
(`sed -e '3q'`), so nothing is ever signalled. Prefer **`tail` over `head` for error text** — the
diagnosis is at the end. And assert the **exit code** in verification, not just the text: a stub
`DOCKER` harness with `probe_sudo` / `probe_daemon` modes is how this was caught, and the same
harness proves both modes now return 1.

That harness is worth keeping in mind generally: `DOCKER` is a space-split string, so
`DOCKER=/tmp/stub/docker-stub.sh` routes every docker call in every launcher to a script — all four
PLE branches, the probe and the sudo refusal, with no daemon, no GPU and no model boot.

## Sources
- vllmcustom: `common/_run-lib.sh` (the probe), caught in review before commit, 2026-10-13
- vllmcustom: `context/plans/01_uid_parity_tracker.md` § Evidence, "T2 branch harness"

---

# A probe whose own setup is the thing it measures [OBSERVED 2026-10-13]

## Problem
Two Plan 01 T4 probes each produced a confident, wrong verdict, and neither failure looked like a
broken probe:

1. `docker run --rm uidprobe id` — the image's `ENTRYPOINT` is `["vllm","serve"]`
   (`container/Dockerfile:588`), so `id` was parsed as a **model tag** and vLLM died on
   `RepositoryNotFoundError: 401 … huggingface.co/id/resolve/main/config.json`. A mistake in the
   probe was reported as a network/authentication fault, and the natural (wrong) conclusion was
   "uid 1000 cannot run this image".
2. `ls -A ~/.cache` run **after** `import flashinfer`, in the same shell — it listed the directory
   that very import had just created. `context/plans/01_uid_parity.md` reads a line there as
   *"the image ships cache content, move the mount point"*, so the false positive was wired directly
   to a launcher change plus a Rule B.6 README rewrite. The probe was observing its own
   instrumentation.

Both are one bug: **the intervention happened before the observation.**

## How to avoid
- **Arguing with a container? Neutralize its `ENTRYPOINT` first.** Any `docker run` that passes a
  command must pass `--entrypoint` (`--entrypoint id`, `--entrypoint bash … -c`). Otherwise your
  arguments are that program's stdin. Note the corollary: probing *without* `--entrypoint` is a
  valid test of the real entrypoint — but only when that is what you meant to test.
- **For an existence question, use the most privileged identity available, nothing mounted, no
  payload run.** Ask as the image's default root: if root sees nothing there, uid 1000 certainly
  sees nothing, and no code you ran can have created it. Reserve the real identity for *writability*
  questions. Ordering the snapshot before the action also works, but asking the stronger question
  makes the ordering moot.
- **Before running a probe, write down what a wrong answer would look like.** If the wrong answer
  resembles an unrelated subsystem — a 401, an "image not found", a slow boot — the probe is not
  safe to run; rewrite it first. This is the same failure class the `docker version` probe in
  `common/_run-lib.sh` exists to kill: a dead socket that reports as a missing image.

## Sources
- vllmcustom: `context/plans/01_uid_parity.md` § T4, corrected in place 2026-10-13
- vllmcustom: `container/Dockerfile:588` (`ENTRYPOINT ["vllm","serve"]`)
- vllmcustom: `context/plans/01_uid_parity_tracker.md` § Notes / Deviations, "T4 was run with the plan's own Verify block"

---

# `${var@a}` answers with attribute flags, not a type letter [OBSERVED 2026-10-13]

## Problem
`container/build` and `common/_run-lib.sh` both guard against `DOCKER` being passed as an array
instead of a string, with `${DOCKER@a} == a`. That test is wrong, and the obvious fix is wrong in
the opposite direction. Measured on bash 5.2:

| how `DOCKER` was set | `${DOCKER@a}` | `== a` | `!= ''` |
|---|---|---|---|
| `export DOCKER="sudo -n docker"` (i.e. `DOCKER=docker ./container/build`) | `x` | passes | **rejects** |
| `DOCKER="docker"` shell-local | *(empty)* | passes | **rejects** |
| `declare -a DOCKER=(…)` | `a` | rejects | rejects |
| `declare -ax DOCKER=(…)` | `ax` | **misses** | rejects |
| `declare -Ax DOCKER=([k]=v)` | `ax` | **misses** | rejects |

`@a` is a **flag set**, so an exported variable always reports at least `x` whatever it holds.
Widening the test to "is it non-empty" — the natural reading of "`@a` tells you it's an array", and
the direction a review suggested here — rejects `DOCKER=docker` arriving from the environment,
which is the *only* way anyone ever sets it: the guard fires on every legitimate invocation and the
script becomes unusable. `== a` meanwhile passes every string, exported or not, but misses an
array once it is exported.

## How to avoid
- Test for the flag **inside** the set: `[[ ${DOCKER@a} == *[aA]* ]]`. Nothing else.
- Read `@a` as "what is true of this variable" (`a` array, `A` assoc, `x` exported, `r` readonly,
  `n` nameref, `i` integer), not as "what type is this variable". Any `-n`/`==` test on it is
  suspect.
- **Measure before you "fix" a guard on someone's say-so.** This went into a commit as `!= ''` and
  was only caught because the verification step triggered the failure path (`DOCKER=docker
  ./container/build`) instead of assuming the happy path. A review suggestion about semantics is a
  hypothesis, and a one-line `bash -c` settles it.
- Residual, accepted: `declare -i`/`-n` still slip past `*[aA]*`. Neither is reachable from the
  environment (neither survives export as that type), so the guard stays a plain "is it an array".
- Know the limit of the test itself: bash flattens an array to its first element across an `exec`
  boundary, so a child process can never see an array. This guard only bites on a same-shell array
  (a sourced script) — defensive, not a live exploit seam.

## Sources
- vllmcustom: measured matrix 2026-10-13, bash 5.2; recorded in
  `context/plans/01_uid_parity_tracker.md` § Evidence, "`${var@a}` truth table"
- vllmcustom: `common/_run-lib.sh` (the guard as shipped in Plan 01 T1), `container/build` (T5)

---

# A writability guard is itself two bugs waiting [OBSERVED 2026-10-13]

## Problem
Plan 01 T5 added "prove every path is writable before writing anything" to `container/build`. The
first version shipped two defects that only appear in the exact failure the guard exists for, so
neither shows up in a happy-path test:

1. **The path walk has a fixed point.** `probe=${probe%/*}` walks to the parent, but `%/*` on a
   component with no `/` returns it **unchanged**. `BUILD_DIR=build ./container/build` (relative,
   not yet created) therefore never shortens, never reaches `/`, and hangs forever with no output —
   a silently wedged terminal in place of the fast refusal the block exists to give. It is not a
   single-word-only bug: *any* relative path hangs once it reaches its last slash-less component.
2. **The diagnosis and the remedy named different paths.** The check tests the nearest existing
   *ancestor* (that is where ownership actually hides when the leaf doesn't exist yet), but the
   printed one-time fix named the *leaf*. So a legacy root-owned `/mnt/noir/scratch/ai/vllm/build`
   with no `src/` yet printed `sudo chown -R 1000:1000 -- …/build/src` — a path that does not
   exist. `chown: cannot access`. Every other line of the message was right, and the one line the
   user pastes was wrong.

Both are the same shape as the `@a` entry above: **a guard written from a mental model of an
operation instead of from its edge cases.**

## How to avoid
- **Absolute-ize before reducing a path**: `[[ $p == /* ]] || p=$PWD/$p`. One line, and it makes the
  printed path unambiguous as a bonus. Any loop that shrinks a string must prove it always
  shrinks — `${x%/*}`, `${x%/*}`-alikes, and `${x%.*}` all have a no-match fixed point.
- **The remedy must name the path the check actually tested.** If your guard substitutes an
  ancestor/parent/fallback on the way to its verdict, the advice has to follow the substitution.
  Chowning the parent is also the *semantically* correct fix here — `mkdir -p` then recreates the
  leaf with the right ownership.
- **Execute every remedy string you print, at least once, in the state that prints it.** T5's
  verification is now "parse the printed line, `eval` it, assert exit 0, assert the next run
  passes" — not "read the message and nod". A fix that cannot run is not a fix.
- **Prefer a non-destructive remedy for a path you did not create.** `%q` quotes a *token*, never a
  *scope*: `sudo rm -rf -- %q` over a user-settable `CTX_DIR` turns a mistyped
  `CTX_DIR=/mnt/noir/scratch/ai/vllm` into one pasteable command that deletes the build root
  including the source trees, on a host where sudo is `(ALL) NOPASSWD: ALL`. So: `chown` for the
  user-settable root, `rm` only for directories whose names the script chose.
- **Don't assert a cause you did not observe.** "left over from a build run under sudo" was printed
  for any unwritable path; when the owner uid is not 0 the real cause is a mode or a mistyped
  override, and the wrong diagnosis sends the reader off to fix sudoers. Print the uid, then branch
  the explanation on it.
- Drop `-n` from a sudo you *print* but keep it in a sudo you *run*: the flag exists so an
  in-script sudo can never hang, and a one-time fix is a human at a keyboard, who should get a
  password prompt on a host with no NOPASSWD rule rather than a failure.

## Sources
- vllmcustom: `container/build` § "refuse a root run, then prove every path and the socket"
  (Plan 01 T5, 2026-10-13); both defects found in review before commit and reproduced after the fix
- vllmcustom: `context/plans/01_uid_parity_tracker.md` § Evidence, "T5 guard matrix"
- vllmcustom: README § Caching ("a fix you cannot run is not a fix")

---
