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
| A pin in `versions.env` (the NInfer commit, the CUDA base images, the moby seccomp ref) or the `Dockerfile` | **Yes.** A NInfer bump can change the artifact contract (v2 → v3 already happened) and force an hours-long re-conversion; a CUDA bump changes every kernel. |
| The launcher `run`, the shared `lib/common.sh`, or a new launcher | **Yes.** A wrong flag is a server that boots wrong or silently slower, and nothing fails until a bench says so. |
| The seccomp profile, or any `--security-opt` / `--privileged` / capability change | **Yes.** Loosening it widens a server bound to `0.0.0.0`; tightening it breaks the PLE disk reads at boot. |
| `scripts/convert`, a conversion recipe or override, or anything that writes a `.ninfer` artifact | **Yes.** A bad artifact costs hours of conversion and fails only at serve time. |
| Anything that mutates the host outside git: `nvidia-smi -pl/-lgc`, chowning a cache, copying or deleting a 100 GiB artifact | **Yes.** Unlintable and uncommitted — the plan is the only record that it happened. |

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

There is **no compiler here and no CI**. This project is bash scripts, a Dockerfile, a seccomp
profile and a pin file that drive a C++/CUDA engine it does not contain. A launcher typo is a
server that never boots; a missing syscall in the profile is a boot that dies at the first PLE
read; a flag NInfer renamed is a server that silently runs a slower default. So the per-commit
gate is mechanical, needs **no GPU and no model load**, and runs before every commit, from the
project root (`server/ninfer`).

Every step reports **pass / fail / skip**, and **skip is not pass**: no task is `done` on a run
where an applicable step was skipped.

**1. Syntax + counts.** Files are classified by **shebang first, extension second** — `bash -n`
happily passes a Python file, and the launchers have no extension. Pass: no `FAIL` line, the
counts below match the expected numbers, `env ok` (or the stated `SKIP` before `versions.env`
exists). **Expected counts: `4 b`, `1 j`** (Plan 01 T1: `lib/common.sh`; T2:
`scripts/seccomp-profile`, `seccomp/ninfer.json`; T3: `build`; T4: `scripts/convert`). Bump the expected
numbers in this paragraph when a file is added or removed on purpose — a count that silently drops
is the failure this step exists for.

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
# 1c — versions.env stays plain KEY=value: it is `source`d by the scripts and passed to
#      `docker build --build-arg`; a quoted value or trailing comment breaks one of the readers
if [ -f versions.env ]; then
  grep -nvE '^[A-Z][A-Z0-9_]*=[^[:space:]]*$|^[[:space:]]*#|^$' versions.env \
    && echo 'FAIL(env)' || echo 'env ok'
else echo 'SKIP(1c: no versions.env yet)'; fi
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

**3. seccomp probe** (Plan 01 T2). Runs `io_uring_setup` (425) and `keyctl` (250) in a throwaway
container under docker's default profile, `seccomp/ninfer.json`, and `unconfined`. `PROBE_IMAGE`
defaults to the `../vllmcustom/container/.last-build` image (it has `python3`; the NInfer runtime
image has none); `--network none`, nothing pulled. Pass: the last line is `seccomp ok` — default
blocks both, ours allows io_uring and still blocks `keyctl`, and unconfined lets `keyctl` through
(the negative control is live, not vacuous). Missing docker or probe image is **fail**.

```bash
PROBE_IMAGE=${PROBE_IMAGE:-$(. ../vllmcustom/container/.last-build && echo "$IMAGE:$VTAG")}
P='import ctypes,os; L=ctypes.CDLL(None,use_errno=True); b=ctypes.create_string_buffer(120)
r=L.syscall(425,8,b); print("uring", "ok" if r>=0 else os.strerror(ctypes.get_errno()))
r=L.syscall(250,0,-3,0); print("keyctl", "ok" if r>=0 else os.strerror(ctypes.get_errno()))'
g3() { docker run --rm --network none "$@" --entrypoint python3 "$PROBE_IMAGE" -I -c "$P" 2>&1 | tr '\n' ' '; }
g3d=$(g3); g3n=$(g3 --security-opt "seccomp=$PWD/seccomp/ninfer.json"); g3u=$(g3 --security-opt seccomp=unconfined)
printf '%-12s %s\n' default "$g3d" ninfer.json "$g3n" unconfined "$g3u"
[[ $g3d == 'uring Operation not permitted keyctl Operation not permitted ' &&
   $g3n == 'uring ok keyctl Operation not permitted ' &&
   $g3u == 'uring ok keyctl '* && $g3u != *'keyctl Operation not permitted'* ]] \
  && echo 'seccomp ok' || echo 'FAIL(seccomp)'
```

**4. build preflight** (Plan 01 T3). `PREFLIGHT_ONLY=1 ./build`: manages `vendor/ninfer` (clones it
on first run), resolves both CUDA base digests, lints the `Dockerfile` with `docker build --check`,
prints the build command and builds nothing. Needs network (GitHub, Docker Hub metadata — no
layers). Pass: the last line is `build preflight ok` — exit 0, checkout clean at `NINFER_SHA`, both
bases resolve, `no warnings found`, and the count of images labelled with the pin is unchanged.

```bash
g4sha=$(sed -n 's/^NINFER_SHA=//p' versions.env)
g4n0=$(docker images -q --filter "label=ai.ninfer.ref=$g4sha" | wc -l)
g4=$(PREFLIGHT_ONLY=1 ./build 2>&1); g4rc=$?
g4n1=$(docker images -q --filter "label=ai.ninfer.ref=$g4sha" | wc -l)
grep -E '^== (vendor/ninfer at|base resolves)|^Check complete|^##' <<<"$g4"
[[ $g4rc == 0 && $g4 == *"vendor/ninfer at $g4sha, clean"* && $g4 == *'Check complete, no warnings found.'* &&
   $(grep -c '^== base resolves' <<<"$g4") == 2 && $g4n0 == "$g4n1" &&
   -z $(git -C vendor/ninfer status --porcelain) ]] \
  && echo 'build preflight ok' || echo 'FAIL(build preflight)'
```

**5. convert dry-run** (Plan 01 T4). `DRY_RUN=1 ./scripts/convert` against fake checkpoints —
crafted safetensors headers (no tensor data) and a 294,912-key NVIDIA index — in a
`mktemp -d /tmp/ninfer-gate5.*` tree, with a stub `docker` on `PATH` (answers `version`,
`image inspect`, and `ps` from `STUB_PS`). Needs `vendor/ninfer` clean at the pin (step 4). Pass: the
last line is `convert dry-run ok` — the happy path exits 0, prints a command with every wanted flag
and creates no directory; each of the six refusals exits 1 with its message. The tree is removed.

```bash
g5=$(mktemp -d /tmp/ninfer-gate5.XXXXXX); mkdir -p "$g5/stub" "$g5/base" "$g5/bad" "$g5/nvidia"
printf '%s\n' '#!/bin/bash' 'case $1 in version|image) exit 0 ;; ps) [[ -n ${STUB_PS:-} ]] && echo "$STUB_PS"; exit 0 ;; esac' \
  'echo "stub docker: unexpected: $*" >&2; exit 99' >"$g5/stub/docker"; chmod +x "$g5/stub/docker"
python3 -I - "$g5" <<'PY'
import json, struct, sys
g = sys.argv[1]
GU = "model.language_model.layers.0.mlp.experts.gate_up_proj"
PLE = "model.language_model.layers.1.ple.ple_embedding.ngram_embedding.shard_0.weight"
def tree(d, t):  # config + index + one header-only shard
    h = json.dumps({k: {"dtype": a, "shape": s, "data_offsets": [0, 0]} for k, (a, s) in t.items()}).encode()
    open(f"{d}/m.safetensors", "wb").write(struct.pack("<Q", len(h)) + h)
    json.dump({"architectures": ["Qwen4ExpForConditionalGeneration"]}, open(f"{d}/config.json", "w"))
    json.dump({"weight_map": {k: "m.safetensors" for k in t}}, open(f"{d}/model.safetensors.index.json", "w"))
tree(f"{g}/base", {GU: ("BF16", [512, 1280, 2560]), PLE: ("BF16", [2500012, 160])})
tree(f"{g}/bad", {PLE: ("F8_E4M3", [2500012, 160])})            # the NVFP4 checkpoint's shape of wrong
n = f"{g}/nvidia"
json.dump({"quantization": {"quantized_layers": {f"model.language_model.layers.{i}.mlp.experts":
           {"quant_algo": "NVFP4"} for i in range(48)}}}, open(f"{n}/hf_quant_config.json", "w"))
json.dump({"weight_map": {f"model.language_model.layers.{l}.mlp.experts.{e}.{p}_proj.{t}": "m.safetensors"
           for l in range(48) for e in range(512) for p in ("gate", "up", "down")
           for t in ("weight", "weight_scale", "weight_scale_2", "input_scale")}},
          open(f"{n}/model.safetensors.index.json", "w"))
open(f"{n}/m.safetensors", "wb").close()
PY
g5run() { env PATH="$g5/stub:$PATH" DRY_RUN=1 BASE="$g5/base" NVIDIA="$g5/nvidia" OUT="$g5/out/x.ninfer" \
            CACHE_DIR="$g5/cache" MIN_FREE_GB=0 "$@" ./scripts/convert 2>&1; }
g5ok=1
g5out=$(g5run); g5rc=$?
for want in '--user 1000:1000' '--gpus device=0' '--network none' '--recipe qwen3_8_flash_next' \
            '--source nvidia=/in/nvidia' '--out /out/x.ninfer' '-v '"$g5"'/out:/out'; do
  if [[ $g5out == *"$want"* ]]; then echo "PASS(cmd $want)"; else echo "FAIL(cmd $want)"; g5ok=0; fi
done
if [[ $g5rc == 0 && ! -e $g5/out && ! -e $g5/cache ]]; then echo 'PASS(happy rc 0, nothing created)'
else echo "FAIL(happy rc $g5rc or created a dir)"; g5ok=0; fi
g5ref() {  # g5ref <label> <message substring> <env…>
  local label=$1 want=$2 out rc; shift 2
  out=$(g5run "$@"); rc=$?
  if [[ $rc == 1 && $out == *"$want"* ]]; then echo "PASS(refuse $label)"
  else echo "FAIL(refuse $label: rc $rc)"; g5ok=0; fi
}
g5ref 'missing base' 'download Qwen/Qwen3.8-Flash-Next --revision' BASE="$g5/nope"
g5ref 'non-BF16 base' 'is F8_E4M3 [2500012, 160], not BF16' BASE="$g5/bad"
mkdir "$g5/out"; : >"$g5/out/x.ninfer.part-0001"
g5ref 'OUT exists' 'already exists' ; rm -r "$g5/out"
g5ref 'vllm running' 'busy: running container(s) vllm' STUB_PS=vllm
g5ref 'no space' 'the artifact needs >=' MIN_FREE_GB=999999999
g5ref 'sudo' "DOCKER names 'sudo'" DOCKER='sudo docker'
rm -rf "$g5"
(( g5ok )) && echo 'convert dry-run ok' || echo 'FAIL(convert dry-run)'
```

**6–7. Behavioral steps — added by the Plan 01 task that creates what they check** (Rule A.5).
Each lands with its exact commands and pass condition in that task's commit; until then it does
not exist, and nothing may claim it passed.

| Step | Added by | Checks | Pass condition (to be made exact by that task) |
|---|---|---|---|
| 6 launcher dry-run | Plan 01 T5 | `./run` against a stub `docker` | the emitted command carries `--user 1000:1000`, the seccomp profile, `--ulimit memlock=-1`, `-p …:8700`, the MTP flags; `DOCKER` naming sudo is refused (both arms) |
| 7 image identity | Plan 01 T6 | the built image | label `ai.ninfer.ref` == `NINFER_SHA`, exactly one image ID, `USER 1000:1000`, `ldd` reports nothing `not found`, cuobjdump shows `sm_120a` only |

**Runtime tier — NOT part of the per-commit gate.** Booting a model, a smoke request and a bench
happen only when a plan task names them, and **the owner runs them** (decision 2026-10-27: image
builds, conversions, boots and benches are the owner's). When one runs: assert the server on
`:8700` is the one under test first (`docker ps --filter publish=8700 --format '{{.Names}} {{.Image}}'`),
and record numbers per P1.

**What this gate cannot see.** Steps 1–2 catch syntax, not behavior. Nothing in steps 1–7 touches
the GPU: it cannot see a kernel that is slow, a PLE read that is slow on NFS, an expert cache that
spilled to the CPU, or an artifact that converted but serves garbage. That is why the runtime tier
is a named tier and not a quiet omission — never mark a launcher, Dockerfile or conversion task
`done` on steps 1–7 alone if its behavior was runtime: say in the tracker that the runtime tier did
not run.

**Prerequisites.** `shellcheck`, `python3`, `git`, `docker` reachable as yourself (no `sudo` — read
the socket's owner with `stat -c '%U:%G %a' /var/run/docker.sock` if it is refused). GPU and models
are needed for the runtime tier only.

**Baseline at init (2026-10-27, branch `dev`, empty project):** step 1 **pass** vacuously (no
tracked scripts, no counts printed, `SKIP(1c: no versions.env yet)`); step 2 **pass** vacuously
(`shellcheck ok` over an empty set). Steps 3–7 do not exist yet. The baseline generated **no
files**.

### Rule B — Commit at every task boundary (or more often) <!-- hp:commit -->

**YOU MUST COMMIT BEFORE MARKING ANY TASK COMPLETE.** If you haven't committed, you haven't
finished.

1. Commit at the end of **every** task. Smaller intermediate commits are welcome. **Never wait
   for the plan to finish.**
2. One task per commit — do not batch tasks unless they are truly inseparable.
3. Message format: `[ninfer][P<n>][T<n>][<topic>] <imperative summary>` — the project prefix
   `[ninfer]` **first, on every commit** (owner directive, 2026-10-27), then plan number (not
   zero-padded), task number, short area tag, then the summary. Keep the whole subject under 72
   characters. Outside a plan: `[ninfer] <summary>`. Plan bookkeeping replaces the task tag:
   `[ninfer][P<n>][plan] add <title>`, `[ninfer][P<n>][revise] …`,
   `[ninfer][P<n>][close] <title>: <outcome>`. Examples:
   - `[ninfer][P1][T3][docker] add pinned sm_120a Dockerfile + build`
   - `[ninfer][P1][close] flash-next container serving: shipped, K=3 default`

   Commits before 2026-10-27's directive (`f7e52fa`, `0815f46`, `7d0885d`, `52f360f`, `6c708d2`)
   carry the old forms; history is append-only (Rule B2), so they stay as they are.
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
- Plan numbers repeat across the monorepo's sub-projects (vllmcustom also has a Plan 01). The
  `[ninfer]` prefix is what tells this project's commits apart in a monorepo `git log`.

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

The same rule covers `vendor/ninfer`, the pinned NInfer checkout `./build` manages: never
`checkout --force`, `reset --hard` or `clean` it. A dirty checkout is a **refusal**, not something
to clean up.

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
   settings. It goes in the tracker's Evidence table. For this project that means the image tag
   **and** its `ai.ninfer.ref` label, the artifact path and its `.conversion.json`, and the
   launcher's echoed `docker run` line.
4. **Change one thing at a time.** A different build *and* different settings in one comparison
   measures nothing.
5. **Check the environment first** (throttling, background load, warm vs cold caches). A run under
   a bad state is re-measured, not caveated.
6. **Inconclusive is a valid, recordable outcome.** Record it. Do not round it up.

### P2 — Capture hygiene

1. Captures, traces and raw result files go in `captures/` and are **gitignored** — they are
   large and regenerable.
2. Name them so they are identifiable six months later: `<workload>_<YYYY-MM-DD>_<what-varies>.<ext>`.
3. The tracker's Evidence table says which capture backs which number.
4. **Restore the system after a session** — `nvidia-smi -pl/-lgc`, launcher env knobs, debug
   flags, a copied artifact you no longer need. The next session's baseline depends on a known
   state.

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
- Code blocks always carry a language (` ```bash `, ` ```json `, ` ```dockerfile `).
- Cross-reference plans as "Plan N" or "Plan N T2"; cite sources as `path/file:line`.
- Dense, no fluff — but never so compressed a detail is lost.

---

## Templates & History <!-- hp:templates -->

- New plan: copy `NN_example.md` → `NN_name.md` and `NN_example_tracker.md` →
  `NN_name_tracker.md` (drop their first-line `hands-plan` stamp), take the number from SERIES,
  fill every section, add the SERIES row, and bump **Next free plan number**.
- For real examples of the live format, browse `context/plans/completed/` — newest first.
