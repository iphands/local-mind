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
