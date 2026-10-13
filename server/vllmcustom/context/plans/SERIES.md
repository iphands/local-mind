<!-- hands-plan:v1 2026-10-13 -->
# vllmcustom — Plan Series

Dependency chain and status of every plan. **Read this before creating a plan. Update it whenever
a plan is added, starts, completes, or is abandoned** — in the same commit as that change.

Plan status: `pending` | `in-progress` | `blocked` | `done` | `abandoned`.
Plans further out are intentionally light: an untouched pending plan is a **hypothesis, not a
contract** — re-read and revise it before starting (RULES.md Rule C).

**Next free plan number: `03`.** <!-- hp:next-free -->

> **North star (user directive, 2026-10-13):** single-stream decode throughput on one RTX PRO 6000
> (sm_120), from a vLLM stack we build ourselves — torch, FlashInfer and vLLM all compiled from
> source, image provably sm_120-only, launchers that refuse to serve the wrong model. A plan earns
> its place by moving measured tok/s or by making a claim about the build checkable without booting
> a model. <!-- hp:north-star -->
>
> **Ordering principle:** the measurement has to exist before the optimization does — harness and
> gate work lands ahead of any tuning claim that depends on it.

## Currently Active <!-- hp:active -->

**02 — No sudo anywhere: plain `docker` is the only privilege path** (`in-progress`). Supersedes the
privilege half of 01: same uid-1000 contract, no `sudo` in any file. The root refusal stays — root is
reachable by `su` and `_writable` cannot see a root run.

**01 — Run everything as uid/gid 1000:1000, host and container** (`pending`, T3b + T10 blocked on an
image rebuild). Wave 1 needs no GPU and no rebuild; do T4 before asking the owner to rebuild.
Superseded in part by 02 (`sudo` machinery removed 2026-10-13); the uid contract and its gate steps
still stand — see `context/distilled.md`.

## Plans <!-- hp:plans -->

Keep **Notes** to ~25 words — the detail lives in the plan's tracker.

| Plan | Title | Depends on | Status | Notes |
|------|-------|-----------|--------|-------|
| 01 | Run everything as uid/gid 1000:1000, host and container | — | pending | Containers run as root today; cache mount target is unreadable to uid 1000, so identity and mount must move together. |
| 02 | No sudo anywhere: plain `docker` is the only privilege path | 01 | in-progress | Socket is user-owned since 2026-10-13; `sudo` default fails 127. Root refusal kept — `su` still reaches root. |

## Pre-existing material <!-- hp:pre-existing -->

Written before this plan system existed. Kept where they are, **not moved, not renumbered**, and
not a substitute for a plan:

- `plans/diffusion-gemma.md` (2026-08-10) — a plan document outside the plan system. Read it before
  proposing anything DiffusionGemma-shaped; it may already contain the reasoning.
- `repin.md` — the runbook for moving `VLLM_MAIN_SHA`. It is the closest thing this repo has to a
  gate for pin changes; a plan touching a pin must follow it, not restate it.
- `notes/ccd_vllm_test.txt`, `qwen3.8-flash-next/upstream-watch-prompt.md` — notes and a remote
  watcher prompt, not plans.

## Abandoned / Superseded <!-- hp:abandoned -->

| Plan | Reason | Superseded by | Date |
|------|--------|---------------|------|

*(Record the reason. A plan dropped without one gets re-attempted six months later by someone who
doesn't know it failed.)*

## Standing Constraints <!-- hp:constraints -->

- **`shellcheck` is not installed.** Gate step 2 reports `skip`, and a task that needs it is
  `blocked`, not `done`. It is the project's own bar already (`repin.md:181`).
- **Run our docker scripts bare — no `sudo -n` prefix.** The account is not in the `docker` group and
  the scripts handle that themselves through `${DOCKER[@]}`; prefixing one with `sudo` *defeats* it,
  because `env_reset` drops your `DOCKER` (`container/archs:16`), and `container/build` plus every
  launcher refuse uid 0 outright. Gate lines of **raw** docker keep `sudo -n`: outside those scripts
  `${DOCKER[@]}` is unset, so a pasted one runs nothing. `container/push`, `bench/*` and `scripts/*`
  are the scripts still calling bare `docker`.
- **`pytest`, `uv` and `rsync` are absent.** The four host patch tests are stdlib-only and run with
  `python3 <file>`.
- **No model load in the per-commit gate** (owner decision 2026-10-13). The runtime tier is gate
  step 6: named by a task or asked for by the human, never assumed. A task whose behavior is
  runtime-only cannot be closed as verified without it, so it says so in the tracker.
- **`$BUILD_DIR/src/{pytorch,flashinfer,vllm,torchaudio}` are hand-editable by design**, and
  `PREFLIGHT_ONLY=1 ./container/build` force-checks them out. Run gate step 4's porcelain guard
  before ever invoking it.
- **The parent `context/` is empty** (`../../../../context`) and the monorepo root has no agent
  file. Nothing generalizes out of here yet; cross-cutting lessons go to the parent only once a
  second sub-project needs them.

---

Completed plans move to `completed/`, abandoned ones to `abandoned/` (RULES.md Rule C). When this
file passes ~30 KB, move the narrative of fully closed series to `completed/SERIES_ARCHIVE.md` and
leave one summary row per series here.
