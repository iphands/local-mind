# Plan 02 — No sudo anywhere: plain `docker` is the only privilege path

> **Status**: in-progress
> **Created**: 2026-10-13
> **Depends on**: Plan 01 | Plan 01's uid-1000 contract is unchanged by this plan — only the
> mechanism that reached the socket changes. Plan 01 stays open (T3b + T10 blocked on a rebuild).
> **Goal**: No file in this project names, parses, defaults to, or tells the operator to run
> `sudo`, while every guard that protects an unrecoverable path survives in sudo-free form.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Replace the `sudo -n docker` default in all three docker-calling scripts with plain
`docker`, delete `ALLOW_ROOT` and every sudo-flavoured error string, and rewrite the gate and the
docs that still assert the sudo contract.

**Deliverables**:
1. `common/_run-lib.sh`, `container/build`, `container/archs` — `DOCKER` defaults to plain `docker`;
   a `DOCKER` value naming `sudo` is refused with a migration message; `ALLOW_ROOT` is gone and
   announces itself if set.
2. The root refusal in `container/build` and in the launchers **kept**, reworded to name no privilege
   tool — because root is still reachable via `su` and `_writable` cannot check anything against it.
3. `container/build` prints remedies that are *runnable without privilege* per path class
   (`su -c` / `mv` aside / relocate), never a paste-unrunnable `chown` and never a destructive
   `rm -rf` over an irreplaceable tree.
4. `context/plans/RULES.md` gate: raw docker lines plain, 7a's sudo sub-check replaced by an
   assertion on the new guard, root-owned sweep check unconditional.
5. Docs corrected where they now lie: `AGENTS.md`, `README.md` § *Caching* / § *Notes*,
   `context/distilled.md`, `context/pitfalls.md`, `SERIES.md`, `repin.md`.

**Estimated effort**: Small–Medium (half day, most of it doc surgery)

---

## Scope

**In**: `common/_run-lib.sh`, `container/{build,archs}`, the gate fence in `context/plans/RULES.md`,
and the doc set listed in deliverable 5.

**Out**: `container/push`, `bench/*`, `scripts/*` — already bare `docker`, which this plan makes the
norm rather than the exception. `container/{slice-setup,gc-setup}` — they legitimately need root and
are already invoked with `su -c`. `context/plans/01_*` and `notes/ccd_vllm_test.txt` — historical
record; supersession is recorded in `SERIES.md` and `distilled.md`, not by editing history.
`vendor/` — third-party. If a fix needs to cross this line, **stop** and record the blocker in the
tracker instead of expanding scope.

---

## Context

The uid-1000 work (Plan 01) routed every docker call through `${DOCKER[@]}`, defaulting to
`sudo -n docker`, on the premise that the invoking account cannot reach the socket. That premise has
now been retired twice over. First the owner declared the operating environment has no sudo
installed and must not expect it. Then the socket itself was fixed: it is now owned by the invoking
user, so plain `docker` works and the whole privilege indirection is dead weight that fails loudly
where it once succeeded quietly.

What actually triggered the work was a hard failure of the documented repin path
(`repin.md` step 3): `VLLM_REF=main ./container/build && ./container/push` died with
`timeout: failed to run command 'sudo': No such file or directory`, exit 127. The probe caught it
before any write, which is the one thing Plan 01 T5 bought that worked as designed.

The subtlety that shaped this plan: **"no sudo" is not "no root."** The repo prescribes `su -c` in
five places and two of its own scripts *require* `EUID == 0`. So a privileged run is still one
password away, and `_writable` is no defence against it — root writes anything. The root refusal in
`container/build` is therefore the *only* thing between `su -c ./container/build` and a root-owned
`$SRC_DIR`, which RULES.md § Project gate step 4 calls destroying the owner's work. It stays. It
just stops talking about sudo.

### Pre-Identified Bug/Issue

The documented build command fails on a host with no sudo, before it writes anything:

```bash
$ VLLM_REF=main ./container/build
timeout: failed to run command 'sudo': No such file or directory   # exit 127
```

And on the host as it is *now*, after the socket fix, the same command's first gate passes with no
privilege at all:

```bash
$ docker version --format 'server={{.Server.Version}}'
server=29.7.2
$ stat -c '%U:%G %a' /var/run/docker.sock
iphands:iphands 660
$ id -nG
iphands 27
```

The scripts still default to `sudo -n docker`, so they still fail. The gap between those two blocks
is the entire plan.

### Why keep a root refusal in a project that forbids sudo

Because the guard protects something unrecoverable and nothing else does. `container/build:313`
runs `git checkout --force` on the four hand-editable trees; RULES.md:493 says a force-checkout in
them "is destroying the owner's work"; `AGENTS.md` says a `checkout --force` in one of them "is not
recoverable". `_writable` tests `-w`, which is always true for root, so it reports success on a tree
it is about to ruin. Deleting the `EUID` test would mean the *only* remaining defence against a
root-owned `$BUILD_DIR` is noticing it later, after the mutation.

The alternative considered and rejected: delete the guard as literally instructed and compensate by
`rmdir`-ing a directory this run created. That protects the cache leaf and does nothing at all for
the four source trees, which is the damage that cannot be undone. Owner decision 2026-10-13: keep a
root refusal, purge all sudo wording.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| Socket ownership after the owner's fix | `iphands:iphands 660` — owned by the invoking uid, not by a group | `stat -c '%U:%G %a' /var/run/docker.sock`, 2026-10-13 |
| Plain `docker` reaches dockerd | `server=29.7.2`, rc 0 | `docker version --format …`, 2026-10-13 |
| `sudo` is absent from the normal env | `exit 127` on `timeout … sudo` | observed failure, 2026-10-13 |
| `sudo` exists in the *agent's* container | `/usr/sbin/sudo` (`docker` also `/usr/sbin/docker`) | `command -v` in the agent shell, 2026-10-13. **Not representative** — do not re-derive host facts from this shell |
| Root is still reachable | `su -c` prescribed by the repo itself | `README.md:473`, `:681`, `:714`, `:798`; `container/build:400`, `:404`; `container/slice-setup:52` |
| Two scripts *require* root | `EUID -ne 0` → refuse | `container/slice-setup:49`, `container/gc-setup:86` |
| `_writable` cannot see a root run | it tests `-w`; root is writable by definition | `container/build:206` |
| `build`'s three `DOCKER` copies already differ | `_run-lib.sh` has no zero-word guard and no 127 branch; `archs:24` claims they are "byte-identical on purpose" | `container/build:182`, `container/archs:33` vs `common/_run-lib.sh:66` |
| Ambient `DOCKER_BUILDKIT=0` is unreachable today | the wrapper is `timeout`, so the env never reaches the CLI | `container/build:429-431` + the `docker build` line — **confirm in T2 before relying on it** |

### Prior Measurements

| Source | Metric | Result | Runs | Spread |
|--------|--------|--------|------|--------|
| Plan 01 tracker T8 row | gate step 7e full-tree sweep | 244,890 entries, **11 s**, exit 0 | 1 | n/a (metadata walk, deterministic) |
| Plan 01 tracker, `README` § Caching census | `$CACHE_DIR` contents | 205,953 files / 35.05 GiB across 244,892 entries, `find … \! -user 1000` → 0 | 1 | n/a |
| Plan 01 tracker T7 row | cold-boot cost of losing that tree | 20–60 min per model family | owner-reported, unmeasured | — |

### Rejected Claims

- **"Just delete every root check, there's no sudo."** Refuted: see *Why keep a root refusal*.
  `su -c` is in the README, and two scripts require uid 0.
- **"Print `rm -rf` as the remedy for a root-owned path."** Refuted twice. `rm -rf` on a path that
  does not exist exits 0, so the silent no-op becomes a first-class branch of `_writable`
  (`container/build:220-221`). And `rm` into a root-owned 0755 leaf fails partway — you need write
  on each directory to unlink inside it, whereas a *rename* needs write on the parent only. So for
  root-owned staging, `mv` it aside; `rm -rf` is worse than silence.
- **"This host has no sudo."** Too strong for the docs. Plan 01's tracker has a dated LIVE row
  measuring `sudo -n -l` → `(ALL) NOPASSWD: ALL`. The honest phrasing is *"the normal operating
  environment has no sudo and this project does not use it."*
- **`PIP_USER_BASE`** is not a pip variable (the real lever is `PYTHONUSERBASE`). Out of scope here;
  it is already a Follow-ups row on Plan 01.

---

## Step-by-Step Tasks

### T1: Launcher library stops knowing about privilege

**File**: `common/_run-lib.sh`

**What to do**: Five edits, all inside the existing structure. Keep the `@a` array predicate
untouched — it was measured on bash 5.2 and is correct.

**Before** (`:63`, `:66`):
```bash
  echo "## DOCKER must be a plain string (DOCKER='sudo -n docker'), not an array." >&2
IFS=' ' read -r -a DOCKER <<< "${DOCKER:-sudo -n docker}"
```

**After**:
```bash
  echo "## DOCKER must be a plain string (DOCKER=docker), not an array." >&2
IFS=' ' read -r -a DOCKER <<< "${DOCKER:-docker}"
if (( ${#DOCKER[@]} == 0 )); then
  echo "## DOCKER is set but holds no words; use DOCKER=docker or unset it." >&2
  exit 1
fi
if [[ ' '"${DOCKER[*]}"' == *' sudo '* || ' '"${DOCKER[*]}"' == */sudo ]]; then
  echo "## DOCKER names 'sudo'. This project does not use it — run docker directly." >&2
  echo "##   fix:  unset DOCKER        (the default is plain 'docker')" >&2
  echo "##   need a wrapper? name it:  DOCKER=/usr/local/bin/my-docker" >&2
  exit 1
fi
```

Then: the `:46-50` comment that justifies `RUN_UID` by "the launcher can be started under sudo"
becomes "a privileged launcher" (comment and old rationale change together, or the next agent
re-derives the guard from the prose); the `:53-56` "Only the docker CLI needs privilege" comment
becomes a plain statement of the `DOCKER` contract; the `:73-83` refusal **stays** with its message
rewritten to name no tool; the `:85-109` probe loses its sudoers/NOPASSWD advice and gains the
`_probe_rc` 124/127 branch that `container/archs:52-57` already has; and `:164`'s `fix: chown …`
becomes honest — you cannot `chown` a directory you do not own without privilege, which was true
before this plan and is now the only advice on that branch.

**Verify**:
```bash
bash -n common/_run-lib.sh                                            # → syntax OK
for d in 'sudo -n docker' 'sudo' 'docker' '/opt/sudo-docker/bin/docker'; do
  env DOCKER="$d" ./qwen/run Fake >/tmp/p2o 2>&1                      # → refuse, refuse, pass, pass
done; cat /tmp/p2o
grep -n ':-sudo\|sudo -n\|sudo chown\|sudo env\|ALLOW_ROOT' common/_run-lib.sh   # → empty
```
Expected: the two `sudo` values print `does not use it` and exit 1 **before** any socket call; the
other two get past the guard. `grep -ci sudo → 0` was the assertion this plan shipped with and it is
**wrong**: a guard that rejects `sudo` must contain the word. The check is that every remaining hit is
the refusal or its comment, and that no *default*, *remedy* or *instruction* uses it.

**Commit**: `[P2][T1][no-sudo] launchers default to plain docker, refuse a sudo'd DOCKER`

### T2: `container/build` — same contract, runnable remedies, BuildKit pinned

**File**: `container/build`

**What to do**: Delete `ALLOW_ROOT` and the `:158-163` escape-hatch block, keeping the `EUID` refusal
and its `checkout --force` rationale. `:168-185` gains the same default + refusal as T1. Replace the
`case $kind` remedies at `:227-232` per class (see *Rejected Claims*). Add a warning if
`${ALLOW_ROOT+set}` is set, where the guard used to be — a silently-ignored flag is worse than a
refusal, because whoever set it believes it is silencing something.

**After** (remedies, the load-bearing hunk):
```bash
    case $kind in
      chown)
        printf '##   no privilege here, so nothing you run as yourself can take it. Pick one:\n'
        printf '##     a) with root:  su -c "chown -R %s:%s -- %q"\n' "$(id -u)" "$(id -g)" "$probe"
        printf '##     b) rename it aside — a rename needs write on the PARENT only:\n'
        printf '##        mv -- %q %q.uid%s\n' "$probe" "$probe" "$owner"
        printf '##     c) relocate: BUILD_DIR= a path you own.\n'
        printf '##   WARNING for b/c on a source tree: hand-editable by design. Check first:\n'
        printf '##     git -C %q status --short\n' "$probe" ;;
      rm)
        printf '##   regenerable staging, but root-owned inside, so rm -rf fails partway.\n'
        printf '##   rename it aside instead — write on the parent is enough, and rsync\n'
        printf '##   recreates it owned by you:\n'
        printf '##     mv -- %q %q.uid%s\n' "$probe" "$probe" "$owner" ;;
      rmf)
        printf '##   one-step fix (unlink needs write on the DIRECTORY, not the file):\n'
        printf '##     rm -f -- %q\n' "$probe" ;;
    esac
```

And pin the builder at the build line — `env DOCKER_BUILDKIT=1 "${DOCKER[@]}" build …` — because the
`timeout` wrapper is what keeps ambient `DOCKER_BUILDKIT=0` from reaching the CLI today, and once
`DOCKER` is plain, a stray `export DOCKER_BUILDKIT=0` in a profile selects the classic builder and
fails at `:433` *after* four `checkout --force`s and GB of staged context.

**Verify**:
```bash
bash -n container/build
env ALLOW_ROOT=1 ./container/build 2>&1 | head -4      # → says the flag is gone, then proceeds/fails normally
DOCKER_BUILDKIT=0 PREFLIGHT_ONLY=1 ./container/build >/dev/null 2>&1; echo "rc=$?"   # → 0, and the build line carries DOCKER_BUILDKIT=1
grep -c -i 'sudo\|ALLOW_ROOT' container/build          # → 0
```
Expected: preflight still green; `grep -ci` zero.

**Commit**: `[P2][T2][no-sudo] build: plain docker, runnable remedies, BuildKit pinned`

### T3: `container/archs` follows, and stops telling people to wrap it

**File**: `container/archs`

**What to do**: Default at `:32` → `docker`; refusal message at `:29` → `DOCKER=docker`; the `:9-11`
usage comment and the `:16-17` `env_reset` paragraph lose sudo. `:16-17` must be *replaced*, not
deleted: the rule it encodes — do not wrap this script in an env-scrubbing wrapper — is what stops a
gate step from silently not running. Keep the zero-word guard and the probe branch, byte-identical
with the other two copies, and update the `:21-27` comment that claims what the default is.

**Verify**:
```bash
bash -n container/archs && grep -c -i sudo container/archs    # → 0
DOCKER='sudo -n docker' ./container/archs 2>&1 | head -2      # → refuses
```
Expected: refuses before probing; a plain `./container/archs` reaches `docker run` (its own table,
or the pre-existing image-missing error — not a privilege error).

**Commit**: `[P2][T3][no-sudo] archs: plain docker default, sudo-free wrapper warning`

### T4: The gate stops requiring a tool the project forbids

**File**: `context/plans/RULES.md`

**What to do**: Every raw docker line in the fence drops its `sudo -n` prefix. Step 7a's fifth
sub-check (the sudo-refusal assertion) is deleted **and replaced in place**, so coverage does not
shrink:

```bash
env DOCKER='sudo -n docker' PATH=/tmp/gate7/stub:$PATH CACHE_DIR=/tmp/gate7/cache \
    MODEL_ROOT=/tmp/gate7/model ./qwen/run Fake 2>&1 \
  | grep -q "does not use it" && echo 'PASS(7a rejects a sudo in DOCKER)' \
  || echo 'FAIL(7a does not reject a sudo in DOCKER (guard deleted?))'
```

The bounded `find /tmp/gate7/cache -user root` check lifts out of the refusal branch so it runs
unconditionally — a launcher that appends a second `--user` still wins on docker's last-wins rule,
and 7a greps for the *presence* of `--user 1000:1000`, so presence alone is not proof. The `:368`
and `:375` remedies keep their **FAIL** and lose the `sudo chown -R` text in favour of `su -c` /
relocate. `:388` keeps its FAIL too: an unreadable entry *is* the finding, so a partial sweep cannot
certify anything and must not soften to SKIP. `:426-427` prerequisites swap "passwordless
`sudo docker`" for "the invoking account may open the docker socket".

**Verify**:
```bash
awk 'NR>=101 && NR<=438' context/plans/RULES.md | awk '/^```bash$/{f=1;next} /^```$/{f=0;print "";next} f{print}' > /tmp/gate_all.sh
bash /tmp/gate_all.sh; echo "rc=$?"      # → every step PASS or a NAMED SKIP, rc 0
```
Expected: `PASS(7a rejects a sudo in DOCKER)` present in the output; no step FAILs; exit 0.

**Commit**: `[P2][T4][no-sudo] gate: plain docker lines, 7a asserts the new guard`

### T5: Docs that currently assert the opposite

**Files**: `AGENTS.md`, `README.md`, `context/distilled.md`, `context/pitfalls.md`,
`context/plans/SERIES.md`, `repin.md`

**What to do**: Ranked by how badly each misleads — i.e. by whether an agent trusts it and then
*acts* wrong. `README.md` § *Caching* first: it claims "the launchers refuse to run under `sudo`"
and documents `sudo env ALLOW_ROOT=1` as a live escape hatch. Both become false safety nets, and
`context/pitfalls.md` already documents the exact poison that follows believing them. Then
`AGENTS.md` (auto-loaded, asserts the scripts carry their own sudo). Then `RULES.md`'s stated
prerequisite — a missing prerequisite makes an agent report the gate BLOCKED on a condition that no
longer exists. Then `distilled.md`'s `[LIVE]` fact card, amended **in place** with a dated note per
house style. Then `README.md` § *Notes*, which tells you to run `bench/`+`scripts/` with `sudo -n`.
Then `SERIES.md`'s standing constraint and its Plan 01 supersession note. Then `repin.md`'s
"`DOCKER=docker` is load-bearing" paragraph and its stub-gate regex `^(sudo -n )?docker `.
`pitfalls.md`'s two sudo entries keep their *traps* (deleting a documented foot-gun is how the next
agent steps on it) and lose only the mitigation sentences.

**Verify**:
```bash
grep -rn -i 'sudo' --include='*.md' . | grep -v '^\./context/plans/01_\|^\./notes/\|^\./vendor/'
grep -rn 'sudo -n\|sudo env\|sudo chown\|with `sudo\|run it with sudo\|DOCKER=.*sudo' \
     --include='*.md' --include='AGENTS.md' . | grep -v '^\./context/plans/01_\|^\./notes/'   # → empty
```
Expected: the first grep may still hit — every hit must be a sentence stating the project does **not**
use sudo, or a dated historical note. The second grep hunts *instructions* and must print nothing.
Counting matches is the wrong test: the refusal guard in three scripts necessarily contains the word.

**Commit**: `[P2][T5][no-sudo] docs: socket is reached directly, privilege machinery gone`

### T6: Full gate, harvest, close

Run the gate verbatim from RULES.md, record every step's status in the tracker (a `skip` is named,
never counted green), harvest findings per Rule D, then move both plan files to `completed/`.

**Expected observation**:
- **Confirms if**: gate exit 0, 7a's new assertion PASS, `grep -ci sudo` = 0 across all touched
  files, and a real `PREFLIGHT_ONLY=1 ./container/build` succeeds with no privilege.
- **Refutes if**: any step FAILs, or the only reason it passed is a named SKIP that this plan
  removed the excuse for.
- **Within noise if**: nothing here is a timing measurement; a flaky socket is a host fault, not
  noise. Log it as a deviation rather than averaging it away.

**Commit**: `[P2][T6][no-sudo] gate green, plan closed`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `common/_run-lib.sh` | `DOCKER` default, sudo-in-`DOCKER` refusal, sudo-free probe advice, honest `:164` remedy | P0 |
| `container/build` | Default, `ALLOW_ROOT` deleted + self-announcing, per-class remedies, `DOCKER_BUILDKIT=1` | P0 |
| `context/plans/RULES.md` | Gate lines plain; 7a asserts the new guard; sweep unconditional | P0 |
| `container/archs` | Default, refusal, replaced-not-deleted wrapper warning | P1 |
| `README.md` § Caching | Removes two false safety nets and a dead escape hatch | P1 |
| `AGENTS.md` | Auto-loaded contract the next agent obeys first | P1 |
| `context/{distilled,pitfalls}.md`, `SERIES.md`, `repin.md` | Remove the sudo contract without rewriting history | P2 |

Priority values: `P0` = blocking, `P1` = important, `P2` = nice-to-have.

---

## Open Questions / Risks

1. **Does the root refusal survive contact with the owner's intent?** The instruction was "delete all
   root machinery"; the finding is that root is reachable by `su` and two scripts need it.
   *How we'll settle it*: owner decision recorded 2026-10-13 — keep a refusal, purge the wording.
   If that is revisited, the compensating fix is already written down: `rmdir` anything this run
   created on the refusal path, using the `CACHE_WAS_NEW` flag `_run-lib.sh:154` already sets.
2. **Is the `DOCKER_BUILDKIT=0` reachability claim real?** It is reasoned from how the wrapper is
   built, not observed. *How we'll settle it*: T2's verify line, before the comment is rewritten to
   depend on it.
3. **Risk: a `DOCKER` containing `sudo` is refused on a host that legitimately needs it.**
   *Mitigation*: the refusal names the escape hatch (`DOCKER=/path/to/wrapper`), and matches whole
   words only, so `/opt/sudo-docker/bin/docker` still passes. Migration friction should cost one
   round trip, not a workaround.
4. **Risk: `bench/*` and `scripts/*` keep calling bare `docker` and are now the only scripts that
   bypass `${DOCKER[@]}`.** *Mitigation*: they write no host files, so the uid contract is unaffected;
   what they lose is crash-log error reporting. Already a Plan 01 Follow-ups row; deliberately not
   in this plan's scope.
5. **Question: is a rootless daemon the eventual target?** If so, `--user 1000:1000` maps through
   subuid and Plan 01's whole cache contract changes shape. *How we'll settle it*: not here — the
   socket is user-owned today and works; recorded so it is not a surprise.

---

## Verification Checklist

- [ ] T1: `env DOCKER='sudo -n docker' ./qwen/run Fake` → refuses naming the fix; `grep -ci sudo common/_run-lib.sh` → 0
- [ ] T2: `PREFLIGHT_ONLY=1 ./container/build` → rc 0 with no privilege; `grep -ci 'sudo\|ALLOW_ROOT' container/build` → 0
- [ ] T3: `grep -ci sudo container/archs` → 0; a `sudo`-valued `DOCKER` refused before the probe
- [ ] T4: `/tmp/gate_all.sh` → exit 0, `PASS(7a rejects a sudo in DOCKER)` in output
- [ ] T5: repo-wide `grep -rn -i sudo` → only sentences stating the project does not use sudo
- [ ] T6: every gate step reported `pass` or a named `skip`, in the tracker
- [ ] All: the project gate (Rule A) passes on the final commit, with every step reported
- [ ] All: step 6 ran **or** the tracker says the behavior went unverified at runtime (step 6 needs a
      model boot; this plan changes no runtime behaviour, so it is named-skipped with that reason)
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
