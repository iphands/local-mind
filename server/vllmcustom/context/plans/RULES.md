<!-- hands-plan:v1 2026-10-13 -->
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
| A runtime patch under `container/patches/` or `muse/patches/` — a new patch, a new hook target, or an edit to its `test_*.py` | **Yes.** A renamed upstream target turns the shim into a silent no-op, and editing a test to make it pass is the same class of failure. |
| The shared launcher plumbing `common/_run-lib.sh` / `common/_model-lib.sh`, or a new launcher | **Yes.** That file documents three call-order rules that break with no error message, and a wrong answer in `_model-lib.sh` changes a bind mount instead of failing. |
| A pin in `container/versions.env`, a change to `container/preflight`, or a new anchor in `container/Dockerfile` | **Yes.** A pin move drags torch and flashinfer with it and invalidates hours of cache, and preflight is the thing that would catch it — so editing preflight is the riskiest edit here. |
| Anything that mutates the host outside git: `container/gc-setup`, `container/slice-setup`, `nvidia-smi -pm/-pl`, docker cache or prune state | **Yes.** Unlintable, uncommitted, largely irreversible — the plan is the only record that it happened. |

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
(required — this project's product is a number, see Rule P1), **Notes / Deviations**,
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

There is **no compiler and no CI here**. The repo is bash launchers, a Dockerfile, and PYTHONPATH
patches that hook into a library they do not contain. A launcher typo is a server that never boots;
a patch aimed at a renamed upstream function is a shim that quietly does nothing. So the gate is
mechanical, needs **no GPU and no model load**, and is what runs before every commit.

Every step reports **pass / fail / skip**, and **skip is not pass**: no task is `done` on a run
where an applicable step was skipped. Until Plan 01 lands `./check`, run the blocks from the
project root.

**1. Syntax + pin-file shape.** Files are classified by **shebang first, extension second**. Shebang
first: `bench/bench`, `bench/make-test-convo`, `scripts/decode-rate` and `scripts/qwen-conc-report`
are **Python** with no extension, and `bash -n` happily *passes* any `.py` file — a glob alone both
false-fails and false-passes. Extension second: 16 of the 20 tracked `.py` files are imported modules
with no shebang at all, and the three sourced libs (`common/_run-lib.sh`, `common/_model-lib.sh`,
`bench/_bench-lib.sh`) open with `# shellcheck shell=bash` instead of one. A shebang-only classifier
syntax-checks none of those nineteen and still prints a green run — it reported `24 b / 8 p` here on
2026-10-13 while the true set was `27 b / 24 p`. Pass: no `FAIL` line, counts **`33 b`** / **`25 p`**,
`env ok`. (Python was 24 until the `4d9a8d6` repin deleted `qwen3.8-flash-next/patches/ple-ct-ignore/`,
whose `ngram_embedding.py` was a tracked no-shebang `.py` and so counted here. Removal on purpose, so
the number moves. Then +2 `p` for `csf-scale-decode` (Plan 04 T1: the patch and its test, both no-shebang) and
`b` 27 → 28 for `nemotron/run`, added by an earlier commit without this line being bumped (2026-10-15), and 28 → 29 for `qwen3.8-flash-next/run-fp4local`, committed in `ddcd061` the same way (2026-10-16), 29 → 30 for `lfm2.5/run` (`b9aa163`, same way, same day), and 30 → 31 for `container/patches/b12x/fetch` (Plan 06 T1), and 31 → 32 for `scripts/flashnext-matrix` (2026-10-18), 32 → 33 for `scripts/flashnext-spec-sweep` (2026-10-18) — a count that drops with no such story is still the failure this step exists for.)

```bash
# 1a — syntax, over tracked files only (in-memory compile: no __pycache__ written)
git ls-files -z | while IFS= read -r -d '' f; do
  case "$f" in vendor/*) continue ;; esac
  case "$(head -n1 "$f" 2>/dev/null)" in   # 2>: `models` is a tracked symlink to a directory
    \#!*bash*|\#!*/sh) bash -n "$f" || echo "FAIL(bash) $f" ;;
    \#!*python*) python3 -c 'import sys; compile(open(sys.argv[1]).read(), sys.argv[1], "exec")' "$f" \
                   || echo "FAIL(py) $f" ;;
    # No shebang: the file says what it is by its extension. Both orders land here — a sourced
    # lib and an imported module alike — so neither can hide from the gate.
    *) case "$f" in
         *.sh) bash -n "$f" || echo "FAIL(bash) $f" ;;
         *.py) python3 -c 'import sys; compile(open(sys.argv[1]).read(), sys.argv[1], "exec")' "$f" \
                 || echo "FAIL(py) $f" ;;
       esac ;;
  esac
done
# 1b — the counts, so an empty file list can never read green. Bump both numbers when a file
#      is added or removed on purpose; a count that silently drops IS the failure.
git ls-files -z | while IFS= read -r -d '' f; do case "$(head -n1 "$f" 2>/dev/null)" in
  \#!*bash*|\#!*/sh) echo b; continue ;; \#!*python*) echo p; continue ;; esac
  case "$f" in *.sh) echo b ;; *.py) echo p ;; esac; done | sort | uniq -c
# 1c — container/versions.env stays plain KEY=value: it is `source`d by three scripts and is
#      meant to be valid to `docker run --env-file`. A quoted value or a trailing comment fails
#      the source silently in one of the two readers.
grep -nvE '^[A-Z][A-Z0-9_]*=[^[:space:]]*$|^[[:space:]]*#|^$' container/versions.env \
  && echo 'FAIL(env)' || echo 'env ok'
```

**2. shellcheck.** Pass: zero findings over the same bash set. `repin.md:181` already runs
`shellcheck -x`, so this is the project's own bar — but **the binary is not installed on this
host**, so today the step is **`skip`**, never `pass`.

```bash
command -v shellcheck >/dev/null \
  && { git ls-files -z | while IFS= read -r -d '' f; do case "$(head -n1 "$f")" in
       \#!*bash*|\#!*/sh) printf '%s\0' "$f" ;; esac; done \
       | xargs -0 shellcheck -x; } \
  || echo 'SKIP(shellcheck not installed)'
```

**3. The patch tests.** Each runtime patch ships a stdlib-only host test that fakes the vLLM seam
it hooks and drives it, **including every graceful-degradation path**. Pass: the count, then that
many `ok` lines.

```bash
ls container/patches/*/test_*.py | wc -l        # want 5 — it grows only when a patch adds a test
for t in container/patches/*/test_*.py; do
  PYTHONDONTWRITEBYTECODE=1 python3 "$t" >/dev/null && echo "ok   $t" || echo "FAIL $t"
done
```
`muse/patches/atem-parsers/selftest.py` is a fifth test that needs `transformers` + `vllm`, so it
runs **in the image** (`./muse/run --selftest`) and belongs to step 6, not here. `muse-embed-norm`
and `muse-dflash` have **no host test at all** — a task touching either must exercise it in step 6
or say in the tracker that it did not.

**4. Pin preflight, read-only form.** Pass: `## preflight OK`, exit 0, for **both** sets.

The two sets no longer share a torch: main took 2.14.0 in vllm#52183 while the release set
stays on 2.13.0. So the snapshot's pins must be prefixed **per command**, not exported — one
exported `TORCH_VERSION` for both lines reads green only while the two sets agree, and after
vllm#52183 it is a false failure on a healthy tree.

```bash
set -a; . container/versions.env; set +a
export SRC_DIR=${SRC_DIR:-/mnt/noir/scratch/ai/vllm/build/src} TORCHVISION_INDEX=cu132 \
  TORCHAUDIO_VERSION=${DEFAULT_TORCHAUDIO_REF#v}      # not diverged: one value serves both sets
VLLM_REF=$DEFAULT_VLLM_REF  FLASHINFER_VERSION=${DEFAULT_FLASHINFER_REF#v}    \
TORCH_VERSION=${DEFAULT_TORCH_REF#v}      TORCHVISION_VERSION=$DEFAULT_TORCHVISION_VERSION \
  ./container/preflight
VLLM_REF=$VLLM_MAIN_SHA     FLASHINFER_VERSION=${VLLM_MAIN_FLASHINFER_REF#v}  \
TORCH_VERSION=${VLLM_MAIN_TORCH_REF#v}    TORCHVISION_VERSION=$VLLM_MAIN_TORCHVISION_VERSION \
  ./container/preflight
```

Use **this** form as the routine check. The documented `PREFLIGHT_ONLY=1 ./container/build` is the
*pin-change* path and is **destructive outside the repo**: it `git fetch`es and
`checkout --force`s all four trees under `$BUILD_DIR/src`, and the README invites hand-editing
those trees in place. Guard it first — anything printed means stop, the tree holds hand edits:

```bash
for t in pytorch flashinfer vllm torchaudio; do
  git -C "${SRC_DIR:-/mnt/noir/scratch/ai/vllm/build/src}/$t" status --porcelain --untracked-files=no \
    | sed "s/^/DIRTY $t: /"
done
```
(`--untracked-files=no` on purpose: flashinfer legitimately carries an untracked
`3rdparty/nccl/`.)

**5. The image you tested is the image you built.** Tags are derived from the image's own OCI
labels and never re-declared, so assert the labels. Pass: `last-build == <the ref under test>`,
`1`, and an arch table with no `sm_80`/`sm_75` **SASS**.

```bash
set -a; . container/versions.env; . container/.last-build; set +a
# WANT = the ref THIS BUILD was made from: $VLLM_MAIN_SHA for a snapshot build (what .last-build
# holds today), $DEFAULT_VLLM_REF for a release build. Hardcoding one of the two makes this line
# fail on the day someone builds the other set and reports a broken gate to a healthy tree.
WANT=${WANT:-$VLLM_MAIN_SHA}
[ "$VLLM_REF" = "$WANT" ] && echo "last-build == $WANT" || echo "FAIL last=$VLLM_REF want=$WANT"
docker images --filter "label=ai.vllmcustom.vllm.ref=$WANT" -q | sort -u | wc -l
./container/archs "$IMAGE:$VTAG"           # minutes of cuobjdump; no GPU, no model; as yourself
```
Compare **image IDs** (`-q`), not tags, and require exactly one: a rebuild of the same sha retags,
so a tag alone proves nothing, and a multi-line filter result makes any string compare arbitrary.
In the arch table the only tolerated non-120 entries are the documented Marlin PTX fallbacks and
the deliberately empty `_vllm_fa3_C` stub; `sm_80` SASS under `_vllm_fa2_C` means the fetch patch
stopped applying and is a **fail**. `container/archs` goes through `${DOCKER[@]}` (plain `docker` by
default since Plan 02), so step 5 runs it **bare** — and wrapping it in anything that scrubs the
environment is not merely redundant but harmful: `env -i`, a login shell, or any privilege helper with
`env_reset` drops your `DOCKER`, the script falls back to its own default, and the step goes green while
exercising something other than what you set. A silently un-executed gate step is worse than a failing
one. `container/push` calls bare `docker` and always has. Every **raw** docker line in this gate is
bare too — the `docker images` line above, step 6's `docker ps`, step 7's `docker image inspect`. Raw
lines stay literal on purpose: the gate is pasted shell with no array parsed, and `${DOCKER[@]}`
expands to *nothing* in a fresh interactive shell, turning the line into `images --filter …`. Since
Plan 02 there is nothing left to bridge: the scripts and the raw lines all say plain `docker`, so
`permission denied` on any of them means your socket access has not landed (`stat -c '%U:%G %a'
/var/run/docker.sock`, then `id -nG`) rather than that a prefix is missing. The launchers likewise call
plain `docker` and **refuse to run privileged**, so a gate step that shells out to `./qwen/run` must be
run as the invoking user.

**6. Runtime / measurement tier — NOT part of the per-commit gate** (owner decision, 2026-10-13:
a model reload is too heavy to demand at every task boundary). It runs only when a plan task names
it or the human asks. When it does:

- **Assert the server is the build under test first.** `docker ps --filter name=^vllm$
  --format '{{.Image}}'` must equal the image you mean to measure — the container name and port
  `8700` are shared by every launcher, so a server left up from an earlier run answers for a build
  it is not, and the sweep measures the wrong model.
- The boot report must **not** contain `hooks NOT applied` (`container/patches/boot-timing/boot_timing.py:692`
  — that line is the report's own admission that upstream renamed its targets), and the headroom
  block must be present.
- The row `bench/lib/measure.py` appends to `bench/bench-results.jsonl` must carry the expected
  image tag and vLLM version. `bench/bench-results.md` is gitignored — an appended row there is
  **not** an assertion; the jsonl provenance fields are.
- Per Rule P1: ≥3 runs, mean and spread, and a delta inside the spread written as **within noise**.

 **7. Run identity (Plan 01).** Every process this project starts runs as uid/gid 1000, and the cache
 directory the container writes is *owned* by 1000 — not merely named `uid-1000`. Five assertions,
 named separately on purpose: bundled under one step number, a legitimate `SKIP` on the image check
 would hide a genuine `FAIL` on the launcher check.

```bash
# 7a — the identity reaches the emitted command line, and a DOCKER naming sudo is refused. Stub docker
#      plus a two-byte fake model: no GPU, no image, no model. About 2 s. NEVER skippable.
install -d /tmp/gate7/stub /tmp/gate7/cache /tmp/gate7/model/Fake
printf '%s\n' '#!/bin/bash' '[[ $1 == image && $2 == inspect ]] && exit 0' \
  'for a in "$@"; do [[ $a == serve ]] && exit 0; done' 'exit 0' > /tmp/gate7/stub/docker
chmod +x /tmp/gate7/stub/docker; echo '{}' > /tmp/gate7/model/Fake/config.json
R="DOCKER=docker PATH=/tmp/gate7/stub:$PATH CACHE_DIR=/tmp/gate7/cache MODEL_ROOT=/tmp/gate7/model"
OUT=$(env $R ./qwen/run Fake 2>&1)          # DOCKER=docker stays explicit even though it is now the
                                            # default: the step must pin the value it means, not drift
                                            # with whatever the script's default becomes next.
grep -qE -- '--user 1000:1000'         <<<"$OUT" && echo 'PASS(7a --user)' || echo 'FAIL(7a --user)'
grep -qE -- 'HOME=/home/ubuntu'        <<<"$OUT" && echo 'PASS(7a HOME)'   || echo 'FAIL(7a HOME)'
grep -qE -- ':/home/ubuntu/\.cache:rw' <<<"$OUT" && echo 'PASS(7a mount)'  || echo 'FAIL(7a mount)'
[ "$(stat -c '%u' /tmp/gate7/cache)" = 1000 ] && echo 'PASS(7a owner)' || echo 'FAIL(7a owner)'
# Fifth assertion. It used to start the launcher privileged and assert the refusal text; Plan 02
# removed that privilege path, so the slot asserts the guard that replaced it. One assertion in, one
# assertion out — coverage must not quietly shrink just because the machinery it tested went away.
# TWO values, because the predicate has two arms: `sudo -n docker` exercises the word arm, `/usr/bin/sudo`
# the path arm. One case per guard instead of one per BRANCH is exactly how the path arm shipped dead for
# an entire plan with this gate green — see the padded-operand entry in `context/pitfalls.md`.
for _7a_docker in 'sudo -n docker' '/usr/bin/sudo'; do
  env DOCKER="$_7a_docker" PATH=/tmp/gate7/stub:$PATH CACHE_DIR=/tmp/gate7/cache \
      MODEL_ROOT=/tmp/gate7/model ./qwen/run Fake >/tmp/gate7/refuse.out 2>&1
  grep -q 'does not use it' /tmp/gate7/refuse.out \
    && echo "PASS(7a rejects a sudo in DOCKER: $_7a_docker)" \
    || echo "FAIL(7a DOCKER=$_7a_docker not rejected — guard deleted, or that arm is dead)"
done
# Unconditional on purpose, and deliberately NOT folded into the line above. The four greps test for
# the PRESENCE of --user 1000:1000, and docker's last-wins rule means a launcher that APPENDED a
# second `--user 0:0` would still satisfy every one of them. Ownership of the directory the run
# touched is the only assertion here that catches that, so it may not hide behind another result.
[ -z "$(find /tmp/gate7/cache -user root)" ] \
  && echo 'PASS(7a left nothing root-owned)' || echo 'FAIL(7a left root-owned files in the cache dir)'
rm -rf /tmp/gate7

# 7b — the image declares the identity itself, so a hand-run `docker run` cannot regress it.
set -a; . container/versions.env 2>/dev/null; . container/.last-build 2>/dev/null; set +a
if [ -z "${VTAG:-}" ]; then echo 'SKIP(7b: no container/.last-build — nothing built on this host)'
else
  U=$(docker image inspect -f '{{.Config.User}}' "$IMAGE:$VTAG" 2>/dev/null)
  if   [ -z "$U" ]; then echo 'SKIP(7b: image declares no USER — Plan 01 T3 rebuild pending)'
                       echo 'SKIP(7b env: same rebuild — the Env defaults ship with the USER)'
  elif [ "$U" = 1000:1000 ]; then echo 'PASS(7b USER 1000:1000)'
  else echo "FAIL(7b USER=$U, want 1000:1000)"; fi
  #    …and the two Env defaults that make the USER alone sufficient. Asked only once the USER is
  #    there, so a stale image stays one honest SKIP rather than turning into three FAILs. The
  #    CCACHE_DIR line is the only canary for it: if it regresses, serve-time compiles go silently
  #    uncached and NOTHING else in the gate or at runtime ever notices.
  if [ -n "$U" ]; then
    E=$(docker image inspect -f '{{json .Config.Env}}' "$IMAGE:$VTAG" 2>/dev/null)
    grep -q '"HOME=/home\/ubuntu"'                     <<<"$E" && echo 'PASS(7b HOME env)' \
      || echo 'FAIL(7b HOME env — docker gives a numeric user no HOME, so a hand-run serve gets HOME=/)'
    grep -q '"CCACHE_DIR=/home/ubuntu/\.cache/ccache"' <<<"$E" && echo 'PASS(7b CCACHE_DIR)' \
      || echo 'FAIL(7b CCACHE_DIR — points at /root: wrapper on PATH, cache dead, compiles still exit 0)'
  fi

  # 7c — the image WORKS at uid 1000. Runs on today's pre-T3 image, and it is the fact the whole
  #      plan rests on, so it is never skippable while an image exists.
  docker run --rm --user 1000:1000 --ulimit memlock=-1 -e HOME=/home/ubuntu \
    --entrypoint bash "$IMAGE:$VTAG" \
    -c 'id -u; ulimit -l; test -r /opt/venv/lib/python3.12/site-packages/vllm/__init__.py && echo readable' \
    | tr '\n' ' ' | sed 's/^/PASS(7c) /; $!s/$//; s/$/\n/'   # want: 1000 unlimited readable
fi

# 7d — USER is the LAST side-effecting instruction. Host-side text check: needs no image, so it
#      cannot SKIP, which is the point — 7b goes SKIP exactly when the image is stale, and a RUN
#      appended after USER is the regression that would then hide (it runs as uid 1000 and
#      half-fails). Anchors are column-0 because continuation lines never start at column 0.
awk '/^USER /{u=NR} /^RUN |^COPY /{r=NR} END{exit !(u && u>r)}' container/Dockerfile \
  && echo 'PASS(7d USER last)' \
  || echo 'FAIL(7d no USER, or a RUN/COPY follows it — it would execute as uid 1000)'

# 7e — after ANY serve (step 6): nothing root-owned landed in the live tree. The product-level fact
#      the plan exists for, and invisible to 7a-7d, which all check the flag and not the outcome.
#      DEPTH-UNBOUNDED ON PURPOSE — a `-maxdepth` sat here when this step was written (T3) and was
#      taken out at T7. Both halves of that story matter, so read them before putting one back.
#      WHY IT WAS CAPPED: an unbounded `du` on this mount once ran ten minutes without returning
#      (README § Caching). That was `du` resolving sizes across 35 GiB, and it walked the SAME tree
#      this step now walks: since Plan 01 T11 the live mount is $CACHE_DIR whole, with no per-uid
#      leaf to retreat to. So this sweep excludes nothing — no `-path … -prune`, no `-maxdepth` —
#      and is bounded by `timeout` alone. The same search capped at six levels reads 19x low.
#      WHY A CAP IS WRONG AND NOT MERELY SLOW: a serve writes huggingface/hub/<repo>/<rev>/… which
#      is depth 6, and torchinductor / flashinfer JIT go deeper. A `-maxdepth 2` therefore prints
#      PASS ON A TREE FULL OF ROOT-OWNED FILES — the one failure this plan exists to catch. So the
#      cost is bounded by `timeout -k`, never by coverage. bash-only like the rest of this fence,
#      and the only step that `exit 1`s — deliberately, and it sits last in the fence for that.
C="${CACHE_DIR:-/mnt/noir/scratch/ai/vllm/cache}"
if [ ! -d "$C" ]; then
  echo "SKIP(7e: $C absent — no cache tree yet; run a launcher once)"
else
  _t0=$SECONDS _7e=0
  _o=$(mktemp) _a=$(mktemp) _e=$(mktemp)   # root-owned matches / every entry swept / find's errors
  # `-fprint` records every visited entry in the SAME walk, so the PASS line can state how much
  # ground it covered — a 2-entry tree and a 200k-entry tree must not print the same green.
  # No `-quit`: one match says nothing about the rest, and the count is the point of the step.
  # Measured 2026-10-13 on the full 244,892-entry live tree: 11 s. The 120 s ceiling is ~10x slack,
  # and it is also what catches a wedged mount — do not lower it because 11 s looked generous.
  timeout -k 10 120 find "$C" -fprint "$_a" -user root -print >"$_o" 2>"$_e" || _7e=$?
  _f=$(head -n1 "$_o"); _m=$(wc -l <"$_o"); _n=$(wc -l <"$_a"); _w=$(wc -l <"$_e")
  _eh=$(sed -n '1,3p' "$_e"); rm -f "$_o" "$_a" "$_e"
  # Order is load-bearing: a match is evidence already in hand, so it outranks a timeout after it.
  if [[ -n $_f ]]; then
    if (( _m > 1000 )); then
      # Scale, not a stray. The single-match remedy below would chown ONE subdirectory, leave the
      # rest, and print a second FAIL that reads like a gate stuck in a loop. This is the pre-split
      # tree, and since T11 it is the live mount — so name the size of the job and give one command.
      echo "FAIL(7e: $_m root-owned entries under $C — the pre-split tree, not a stray file.)"
      echo "     not a broken gate: T11 mounts $C whole, so the live cache IS this directory."
      echo "     one-time fix (this tree: 244,892 entries / 205,953 files / 35.05 GiB, 2026-10-13 —"
      echo "     minutes of NFS metadata, not bytes. Run it to completion: a half-done chown just"
      echo "     reads as a smaller FAIL, and the launcher refuses the same tree until it is done):"
      echo "         su -c \"chown -R $(id -u):$(id -g) -- $C\""
      echo "     no root available? there is NO privilege-free fix for this one: it is the warm cache,"
      echo "     so point CACHE_DIR= at a directory you own and accept a cold boot per model family."
      echo "     done when this prints 0:  find $C -not -user $(id -u) | wc -l"
      echo "     cache-legacy is a SIBLING of $C and stays root-owned on purpose — run-legacy serves"
      echo "     as root and writes that one itself. The command above cannot reach it, and must not."
    else
      _d=$(dirname "$_f"); [[ $_d == "$C"/* ]] || _d=$_f   # never widen past the cache root
      echo "FAIL(7e: $_m root-owned under $C — first: $_f)"
      echo "     one-time fix:   su -c \"chown -R $(id -u):$(id -g) -- $(printf '%q' "$_d")\""
      echo "     no root? relocate instead: CACHE_DIR= a directory you own (cold boot, but certifiable)"
      echo "     then re-count:  find $C -user root | wc -l"
    fi
    exit 1
  elif (( _7e == 124 )); then
    echo "FAIL(7e: sweep of $C TIMED OUT at 120 s — INCOMPLETE, not a pass."
    echo "     The whole live tree sweeps in ~11 s, so this is a wedged mount, not a big tree.)"
    exit 1
  elif (( _7e != 0 )) || (( _w != 0 )); then
    # find could not read part of the tree. A root-written directory is routinely mode 0700, so the
    # very failure this step hunts makes itself unreadable — silence here is not innocence.
    echo "FAIL(7e: sweep of $C incomplete (exit $_7e, $_w stderr lines) — not a pass:"
    if [ -n "$_eh" ]; then printf '%s\n' "$_eh"; fi
    echo "     an unreadable entry IS the finding, not a gap in the check: a directory a root container"
    echo "     wrote is routinely mode 0700, so the exact failure this step hunts is what hides itself."
    echo "     A partial sweep certifies nothing — do not read this as a pass. Take the tree, or move off it:"
    echo "         su -c \"chown -R $(id -u):$(id -g) -- $C\"   # then re-run this step"
    echo "         CACHE_DIR=<somewhere you own>                # cold boot, but the sweep can finish"
    exit 1
  else
    echo "PASS(7e: $C root-free — $_n entries swept, no -maxdepth, $(( SECONDS - _t0 )) s)"
  fi
fi
```

`7d` passes only once Plan 01 T3 lands; before that its `FAIL` is the correct reading, not a broken
gate. `7e` is the assertion to re-run after a boot even when nothing relevant changed, since a
`docker exec -u root` or a stray root container is enough to fail it. It is also the only step in the
fence that `exit 1`s, and the only one whose `FAIL` can mean "could not finish" rather than "found
something" — read the wording, not just the token. Since Plan 01 T11 the whole of `CACHE_DIR` *is*
the live tree, so a `TIMED OUT` means a wedged mount and not a misconfigured variable — the full-tree
sweep measures ~11 s against a 120 s ceiling, so it never times out for being large. A match count in
the hundreds of thousands is the pre-split tree, not a regression: read the FAIL body before acting
on it, because its remedy is one `chown` of the whole root and not the narrow one. Its `SKIP` is not
a pass either, and unlike `7b` it never expires: it says the tree does not exist yet.

`7c` must read `1000 unlimited readable`: the uid, because vLLM imports its own package tree;
`unlimited`, because the Qwen3.8 PLE table is a ~95 GiB `pin_memory` allocation and `--ulimit
memlock=-1` is the only thing between it and `EACCES` — and `--user` is exactly the flag that could
have taken memlock away, since `dockerd` grants the rlimit to the process it starts; `readable`,
because `/opt/venv` is traversable by uid 1000 only by luck of its mode, and a `0700` there would
break the serve at import rather than at first cache write. Measured on the `gdf6c7ad` image
2026-10-13: `soft,hard = (-1, -1)` and a 2 GiB `pin_memory` allocation succeeds.

**What this gate cannot see.** Steps 1–2 catch syntax, not behavior. Step 3 tests each patch
against a *fake* vLLM shaped like the real seam — it proves the patch degrades safely, never that
the real seam still exists. Step 4 checks the pins against upstream's `requirements/cuda.txt` but
**cannot see a torch mismatch**: `use_existing_torch.py` strips that pin, so the resolver never
complains and the build silently compiles vLLM against a torch upstream never tested (preflight
flags it precisely because nothing else will). Nothing in 1–5 touches the GPU path, which is the
product — that is the price of a no-model-load gate, and it is why step 6 exists as a named tier
rather than being quietly skipped. Never mark a launcher, patch or Dockerfile task `done` on steps
1–5 alone if its behavior was runtime: say in the tracker that step 6 did not run.

**Prerequisites.** `shellcheck` (absent → step 2 is `skip`; a task needing it is `blocked`, not
`done`; install it host-side, e.g. `su -c "dnf install ShellCheck"` / `su -c "apt-get install shellcheck"`
— this project uses no `sudo`, so a `sudo`-based install hint is a dead end here) · the invoking account
can open `/var/run/docker.sock` (read who owns it before naming a group: `stat -c '%U:%G %a'
/var/run/docker.sock`; after any membership change, start a new login shell — groups never apply to a
running one) · the four `$BUILD_DIR/src` trees for step 4 · `pytest`, `uv` and `rsync` are **not**
installed — the four host tests are stdlib-only and run with `python3 <file>`; pytest is not the
runner. GPU + models are needed for step 6 only.

**Baseline at init (2026-10-13, branch `dev`, untouched tree):** step 1 **pass** (27 bash /
24 python, `env ok`); step 2 **skip** (shellcheck absent); step 3 **pass** (4/4 `ALL TESTS PASS`);
step 4 **pass** for both the `v0.29.0` release set and the `187a0eb` snapshot; step 5 **pass** on
the two cheap lines (`last-build == pin`, exactly one image ID for the pinned ref) — `container/archs`
was not re-run, it is a per-build check and no build happened; step 6 not run, by design. The
baseline generated **no files inside the repo**.

### Rule B — Commit at every task boundary (or more often) <!-- hp:commit -->

**YOU MUST COMMIT BEFORE MARKING ANY TASK COMPLETE.** If you haven't committed, you haven't
finished.

1. Commit at the end of **every** task. Smaller intermediate commits are welcome. **Never wait
   for the plan to finish.**
2. One task per commit — do not batch tasks unless they are truly inseparable.
3. Message format: `` `[P<n>][T<n>][<topic>] <imperative summary>` `` — plan number (not
   zero-padded), task number, short area tag, then the summary. Keep the **whole subject under 72
   characters**; the tags already cost ~16. Fixed token order, no variants.
   Outside a plan, keep the repo's habitual `scope: summary` (`container: pin main snapshot at
   187a0eb`, `qwen/run: default SPEC_TOKENS 6`). Plan bookkeeping replaces the task tag:
   `[P<n>][plan] add <title>`, `[P<n>][revise] …`, `[P<n>][close] <title>: <outcome>`.
   Examples:
   - `[P1][T1][check] add ./check encoding gate steps 1-5`
   - `[P2][T3][spec-live] reject a depth above max_num_scheduled_tokens`
   - `[P2][close] live spec depth: shipped, SPEC_LIVE default off`
4. **Tracker row in the same commit.** The row update — and any `context/` update the task
   produced — lands with the work. Stage explicit paths: `git add <paths>`, never `git add -A`.
5. The full project gate (Rule A) passes before every commit.
6. A change in observable behavior updates `README.md` in the same commit.
7. **Never push, and never offer to** — the human pushes after review; `git push` in any form,
   `--dry-run` included, is theirs alone, and `./container/push` publishes the *image* and is never
   a licence for it. **Report the commit, never the remote:** say "I did not run `git push`" and
   stop — that is something you observed, while "not pushed" is a claim about a remote you cannot
   see. Incident: `context/pitfalls.md` § *Push state is invisible from an agent shell*.
   **No co-author trailers** unless asked.
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

> **Why this rule exists** (the incident behind it, in the project this skill came from,
> 2026-07-18): a scripted edit hit an assertion and wrote nothing, but the unchained `git commit`
> on the next line ran anyway — a commit whose message claimed updates it did not contain. It had
> already been pushed; "fixing" it with `--amend` diverged `main` from `origin/main` and forced a
> force-push. A follow-up commit would have cost nothing.

> One project-specific extension: the same rule covers `$BUILD_DIR/src/*`. Those four clones are
> hand-editable by design, so `git checkout --force`, `reset --hard` or `clean` in them is
> destroying the owner's work. Gate step 4's guard exists for exactly this.

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

1. Captures, traces and raw result files go in `bench/` (this project's equivalent of
   `captures/`) and are **gitignored** — they are large and regenerable.
2. Name them so they are identifiable six months later: `<workload>_<YYYY-MM-DD>_<what-varies>.<ext>`.
3. The tracker's Evidence table says which capture backs which number.
4. **Restore the system after a session** — `nvidia-smi -pl`, power management, launcher env knobs,
   debug flags. The next session's baseline depends on a known state.

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
- Code blocks always carry a language (` ```bash `, ` ```python `).
- Cross-reference plans as "Plan N" or "Plan N T2"; cite sources as `path/file:line`.
- Dense, no fluff — but never so compressed a detail is lost.

---

## Templates & History <!-- hp:templates -->

- New plan: copy `NN_example.md` → `NN_name.md` and `NN_example_tracker.md` →
  `NN_name_tracker.md` (drop their first-line `hands-plan` stamp), take the number from SERIES,
  fill every section, add the SERIES row, and bump **Next free plan number**.
- For real examples of the live format, browse `context/plans/completed/` — newest first.
