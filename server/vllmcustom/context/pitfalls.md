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

# A socket you may not open is reported as "no images", and it fails that way silently [OBSERVED 2026-10-13, amended 2026-10-13]

## Problem
`docker images` as the user prints
`permission denied while trying to connect to the docker API at unix:///var/run/docker.sock`.
Worse, several call sites swallow it: `require_image` in `common/_run-lib.sh` ends in
`docker image inspect … || { echo "Image … not found. Run ./container/build first."; exit 1; }`,
so an access failure is reported as a **missing image** and sends you off to rebuild an image that
is sitting right there.

## How to avoid
Ask the socket before believing any "image not found": plain `docker images` — this project uses no
privilege helper, so there is no prefix to add. The trap is **closed in code**, not just documented:
the scripts route every call through `${DOCKER[@]}` and probe `docker version` before the first call
that could misreport, printing docker's own last lines first. Two history notes, because both were
true here at the time: under Plan 01 the default was `sudo -n docker` and *prefixing* a script was the
bug, since `env_reset` stripped `DOCKER` and the script silently took its own default; since Plan 02
there is no prefix to add and none to strip, so `permission denied` on any docker line means your
socket access has not landed. Read `stat -c '%U:%G %a' /var/run/docker.sock` before assuming a group.

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
- **Withdrawn by Plan 02 (2026-10-13): the `-n` rule.** This bullet used to read "drop `-n` from a sudo
  you *print* but keep it in a sudo you *run*", on the logic that the flag stops an in-script sudo from
  hanging while a human pasting a one-time fix should get a password prompt. There is no in-script sudo
  left to apply it to. The principle underneath it survives and generalises: **print only remedies the
  reader can actually run**, which is why `_writable` now offers `su -c`, a `mv` aside, or a relocation
  instead of a `chown` on a directory they do not own.

## Sources
- vllmcustom: `container/build` § "refuse a root run, then prove every path and the socket"
  (Plan 01 T5, 2026-10-13); both defects found in review before commit and reproduced after the fix
- vllmcustom: `context/plans/01_uid_parity_tracker.md` § Evidence, "T5 guard matrix"
- vllmcustom: README § Caching ("a fix you cannot run is not a fix")

---

# Mounting the cache at `/root/.cache` made `--user` look unworkable, and it was the mount [OBSERVED 2026-10-13]

## Problem
The launchers mounted the host cache directory onto **`/root/.cache`** while the image declared no
`USER`, so everything ran as root and the mount looked correct. Adding `--user 1000:1000` then looked
like it *broke* the serve: uid 1000 cannot write `/root/.cache`, so the HF metadata tree, the FlashInfer
JIT cache and vLLM's autotune cache all failed — hundreds of lines into a boot, with the flag blamed for
a mount problem. The flag was never the issue. It was pointed at a root-owned path.

## How to avoid
When a uid change "breaks" a container, ask what its writable paths were **mounted from**, not what the
process can do: a bind-mount target is a permission boundary the image cannot overrule. Mount at
`/home/<user>/.cache`, and set `HOME` explicitly (`-e HOME=…` or `ENV HOME`) — with a *numeric* `--user`,
docker does not consult `/etc/passwd`, so an unset `HOME` quietly becomes `/` and every `~/.cache` user
is writing to the filesystem root instead.

## Sources
- vllmcustom: `common/_run-lib.sh` (the `-v …:/home/ubuntu/.cache:rw` mount, `-e HOME=/home/ubuntu`),
  Plan 01 T1/T2/T3; `context/plans/01_uid_parity.md` § Context

---

# A root overwrite keeps the old owner, so "recent files" is not an ownership audit [OBSERVED 2026-10-13]

## Problem
Auditing for root-owned files by recency misses precisely the case worth catching. When a root process
writes **over** a file that uid 1000 already owns, no `chown` happens: the bytes are root's, the owner
field stays `1000`, and `find -newermt` / `ls -lt` present a file that looks perfectly healthy. Sort by
mtime and you will report a clean tree over a tree a root process has been editing all week.

## How to avoid
Audit the **owner**, not the clock: `find <tree> -user root -print`. Where you need to catch change after
a baseline, baseline the tuple `path + inode + owner`, not the timestamps — and know what each form
proves: a *new* file shows up in a `-user root` sweep, an overwrite-in-place does not, which is exactly
why gate step 7e asserts "no root-owned entry exists" rather than "nothing changed recently". If you must
detect overwrites, compare content hashes; there is no ownership signal in one.

## Sources
- vllmcustom: Plan 01 T7 review, 2026-10-13; `context/plans/RULES.md` § step 7e

---

# Running a launcher privileged poisons the cache it is about to mount, and the serve dies minutes later [OBSERVED 2026-10-13, amended 2026-10-13]

## Problem
Every launcher does `mkdir -p` on its cache leaf and then mounts it. Run it **privileged** — via `su`,
or from a root shell; no `sudo` is involved any more, and that changes nothing about this failure — and
that leaf is created **root-owned**, the container starts anyway (the mount itself succeeds), and the
failure lands minutes later as `EACCES` from inside vLLM's first cache write, in a message that names
neither the privilege, nor the ownership, nor the launcher. This is the trap the whole plan exists to
close: half a fix (pin the container uid, keep launching privileged) produces a *worse* bug than no fix
at all.

## How to avoid
Never run a launcher privileged. The launchers refuse uid 0, and so does `container/build` — now with
**no escape hatch**: `ALLOW_ROOT` was deleted in Plan 02, and exporting it prints a line saying it does
nothing. The refusal deliberately outlived the sudo removal, because `_writable` tests `-w`, which root
satisfies by definition, so nothing else in the tree can catch a privileged run before it
force-checks-out the four hand-editable trees. A poisoned tree is named by
`find "$CACHE_DIR" -not -user 1000` and fixed once by `su -c "chown -R 1000:1000 -- $CACHE_DIR"`; with
no root available there is **no privilege-free fix** — point `CACHE_DIR=` at a directory you own and
pay the cold boot.
Since Plan 01 T11 that path **is** the cache root, because the root is what gets mounted; the advice
here used to insist the fix stay "bounded to the offending directory, never to the cache root", which
belonged to the per-leaf mount and is withdrawn with it. Gate step 7e prints the command and its wording
scales — a handful of strays gets the narrow `dirname` remedy, hundreds of thousands gets one whole-root
`chown` and a note on what it will cost. One deliberate exception: `run-legacy` appends `--user 0:0` and
therefore uses its own `cache-legacy` root, so a root serve cannot leave root-owned files inside the tree
the other launchers depend on.

## Sources
- vllmcustom: `common/_run-lib.sh` (root refusal, `RUN_UID`), `container/build` root guard,
  `qwen3.8-flash-next/run-legacy`; Plan 01 T1/T2/T5

---

# `find … | grep -q .` is not a test under `pipefail`: a dirty tree prints PASS [OBSERVED 2026-10-13]

## Problem
`if find "$D" -user root -print -quit | grep -q .` under `set -o pipefail` prints the **opposite** of the
truth when the tree is dirty. `grep -q` exits on its first match and closes the pipe; `find` is still
inside `exit()`, mid-flush, takes `SIGPIPE` and dies at **141**; `pipefail` takes the rightmost non-zero
status, so the pipeline's status is 141, the `if` is false, and the step reports *clean*. The failure is
counter-intuitive in the worst direction: it is the **match** that kills the pipeline, so the check works
on every clean run and lies on every dirty one. This sat inside the gate's own uid assertion for four
green gate runs before a side-by-side test caught it.

## How to avoid
Do not pipe a producer whose exit status you care about into a consumer that exits early. Capture, then
test the capture: `_f=$(find … -print -quit)` and `[[ -n $_f ]]`, with any status you need taken as
`cmd || rc=$?` so 124/127/141 stay distinguishable. Same family as `| head` in a `then` branch above —
in every case the forgotten fact is that the *producer's* status is the one `pipefail` will surface.

## Sources
- vllmcustom: Plan 01 T7, 2026-10-13 — old and new forms run against one synthetic `root:root` file at
  depth 6, old `PASS`/exit 0 vs new `FAIL`/exit 1; `context/plans/RULES.md` § step 7e

---

# `container/build` runs `rsync` unguarded and dies 127 on a host that has no rsync [OBSERVED 2026-10-13]

## Problem
`container/build` stages each source tree into `$CTX_DIR` with `rsync -a --delete --exclude=.git`
(`container/build:362`) under `set -euo pipefail`. Where the binary is absent the build aborts with
**exit 127** *after* `mkdir -p` created the destination leaf — so the directory exists, empty, and
nothing was copied. `PREFLIGHT_ONLY=1` returns at `:344`, before the staging block, so the gate and every
cheap-check path step straight over the gap: eight tasks of green gate runs never executed line 362.
`repin.md:224` (`VLLM_REF=main ./container/build && ./container/push`) is blocked at its defaults by this.

## How to avoid
`CTX_DIR= ./container/build` skips staging entirely and passes the source trees as the contexts — same
cache keys, ~2 min more transfer over NFS. Do not add a silent `tar`/`cp` fallback: `--delete` is
load-bearing, because a stale copy must not survive a `checkout --force`, so any substitute has to
reproduce it or refuse outright. Probe for the binary *before* the `mkdir`, not after. Left unfixed by
owner decision (report only, Plan 01); fix it under Plan 02.

## Sources
- vllmcustom: `container/build:344`, `:362` — `command -v rsync` empty in `$PATH` and under `bash -lc`,
  no shim in `/usr/bin`/`/usr/local/bin`/`/bin`; `[OBSERVED 2026-10-13]`

---

# `find -fprint FILE` logs every entry VISITED, not every match — counting it as matches lies [OBSERVED 2026-10-13]

## Problem
`find "$C" -fprint "$all" -user root -print` writes **every visited path** to `$all` and only the matches
to stdout, and `wc -l <"$all"` is therefore the size of the tree rather than the number of root-owned
entries. Both numbers are formatted identically, so the error is invisible: a sweep of a *fully chowned*
244,892-entry tree "reports 244,892 root-owned entries". The wrong figure propagates — it reached a
launcher's user-facing refusal message before anything noticed.

## How to avoid
Treat the `-fprint` file as a **coverage** counter, which is all gate 7e uses it for ("how much ground this
PASS covered"), and take counts only from the matched stream. When a message must state a count, capture
the match stream to its own file and count that. The tell is a number that contradicts a guard's own exit
status: here the probe returned `rc=0` while the message claimed a quarter-million root-owned files, and
the *number* was the thing to re-check, not the guard.

## Sources
- vllmcustom: Plan 01 T11, 2026-10-13 — the same command reported 244,892 while
  `find "$CACHE_DIR" \! -user 1000 | wc -l` returned 0; `common/_run-lib.sh` refusal text corrected in
  the same commit, `context/plans/RULES.md` § step 7e carries the coverage-only wording

---

# A `[[ x == *' word '* ]]` guard dies at *parse* time if the padding isn't quoted — and a syntax error is the one failure your test won't explain [OBSERVED 2026-10-13]

## Problem
Matching a word inside a space-joined array needs padding so a leading/trailing word still matches, and
writing that padding unquoted on the left of `==` makes bash parse `==` as a fresh command:
`syntax error near unexpected token 'sudo', conditional binary operator expected`. This bit three times in
one plan because the guard exists in three copies (`common/_run-lib.sh`, `container/build`,
`container/archs`) and the same typo was in all three. The trap is not the error — it is that a script
dying at **parse** time and a script **refusing** at guard time both exit non-zero with nothing on
stdout, so a test that asserts "a bad `DOCKER` is refused" passes on a script that never ran.

## How to avoid
Quote the padding as its own operand: `[[ ' '"${DOCKER[*]}"' ' == *' sudo '* || ' '"${DOCKER[*]}"' ' ==
*/sudo ]]`. Then split the two failure modes in the test: `bash -n` first (parse), then the guard matrix
(refuse / refuse / pass / pass). When a guard is duplicated across files, `bash -n` every copy — one
`bash -n` on the file you happen to be editing proves nothing about the others.

## Sources
- vllmcustom: Plan 02 T1-T3, 2026-10-13 — found by `bash -n` before commit in all three copies; the
  correct form is in `common/_run-lib.sh`'s `DOCKER` guard, `container/build` and `container/archs`

---

# A docs sweep that greps for command shapes misses what the docs *assert* — I wrote a ban and then wrote the banned sentence [OBSERVED 2026-10-13]

## Problem
Plan 02's job was to remove every instruction that tells a reader to run `sudo`, and its own text bans the
claim "this host has no sudo" as too strong, because the Plan 01 tracker carries a dated measurement of
`sudo -n -l` → `(ALL) NOPASSWD: ALL`. The rewrite pass then produced `AGENTS.md`: "nothing here uses
`sudo`, **and none is installed**", with the same claim in `SERIES.md`, plus "this box has no `sudo`" in a
`README` host-tuning bullet. None of it was caught by the plan's own Verify, because that grep hunted
*commands* — `sudo -n`, `sudo chown`, `sudo env` — and a prose assertion contains no flag and no
subcommand, so it matches nothing. Two independent reviewers found all four instances; the machine found
none. The stakes are asymmetric: `AGENTS.md` is auto-loaded into every session on this tree, so a single
host fact that the repo's own measurement refutes teaches the next agent to distrust the whole doc set —
including the root refusal whose justification rests on exactly that measurement.

## How to avoid
Grep the **claims**, not only the commands. Add the forbidden predicates to the same alternation as the
command forms — for this plan: `none is installed\|none installed\|this box has no`. Prefer a claim about
the project to a claim about the host: "this project uses no privilege helper, and none is expected"
survives a host change; "is not installed" is a fact you did not measure and someone else's `command -v`
will refute. The general shape: **a docs task's verification must assert what the docs mean, not what they
contain** — counting occurrences of a word is never the test, because a guard that refuses `sudo` must
name it.

## Sources
- vllmcustom: Plan 02 T5, 2026-10-13 — four instances removed, found only in review; the assertion in
  `context/plans/02_no_sudo.md` § T5 Verify was rewritten to hunt claims as well as commands

---

# Extracting a fenced block by remembered line numbers silently drops its tail — recompute the range from the markers [OBSERVED 2026-10-13]

## Problem
The project gate is a bash fence inside `context/plans/RULES.md`, extracted with
`awk 'NR>=101 && NR<=459'` before the ` ```bash ` scan. Plan 02 T4 edited that file and the fence's end
moved from 438 to 459-460. A remembered range does not fail: it extracts a *shorter* gate, which runs,
prints its verdicts, and exits 0 — the missing steps simply never execute. A green gate that skipped the
ownership checks is indistinguishable from a green gate that ran them, and RULES.md names precisely this
failure ("a silently un-executed step").

## How to avoid
Derive the range every time from the section markers, never from memory:
`A=$(grep -n '^### Rule A' RULES.md | cut -d: -f1); B=$(grep -n '^### Rule B' RULES.md | cut -d: -f1);
awk -v a="$A" -v b="$B" 'NR>a && NR<b' RULES.md`. Then prove the extraction is whole — count the
`PASS(`/`FAIL(` verdict tokens, or check that the last step you expect is present — and print the line
count of the generated script so a shrink is visible in the log. Same lesson applies to any fence
extracted by line range, including the step-7 sub-fence.

## Sources
- vllmcustom: Plan 02 T4, 2026-10-13 — range moved 438 → 460 mid-plan; recomputed-from-markers form used
  for the T5/T6 gate runs, which reported 9 PASS / 3 named SKIP / 0 FAIL

---
