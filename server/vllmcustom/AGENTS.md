# vllmcustom — source-built, sm120-optimized vLLM

Self-owned vLLM stack for the RTX PRO 6000 Blackwell (sm_120): PyTorch, FlashInfer and vLLM
compiled from source for `TORCH_CUDA_ARCH_LIST=12.0` on CUDA 13, one launcher per model family,
runtime patches mounted over `PYTHONPATH`, and a bench harness aimed at single-stream decode
throughput. `README.md` is the detailed reference; this file is the working contract.

## Build & verify

- `PREFLIGHT_ONLY=1 ./container/build` checks the pins and stops. `./container/build` builds;
  `VLLM_REF=main` builds the pinned snapshot. `./container/archs IMAGE:TAG` proves what it emitted.
- `./qwen/run` · `./muse/run` · `./laguna/run` · `./qwen3.8-flash-next/run` serve on `:8700`
  (one model at a time, by design — that is why every bench finds the right server).
  `./bench/bench <label>` measures whatever is currently up.
- **Run everything as yourself — nothing here uses `sudo`, and none is expected.** `DOCKER` defaults to
  plain `docker`; point it at a wrapper *path* if you need one, and a value naming `sudo` is refused.
  **Never wrap these scripts in anything that scrubs the environment (`env -i`, a login shell): it drops
  `DOCKER`, the run silently takes the default, and the gate step that exercised the wrong command still
  exits 0.** `container/build`, `container/archs` and every launcher **refuse to run privileged**, with
  no escape hatch since Plan 02 — root is one `su -c` away, so this refusal is the only guard that can
  see a privileged run at all (rationale: README § *Caching*). Root-side steps are only
  `container/slice-setup` and `container/gc-setup`, both via `su -c`. On `permission denied …
  /var/run/docker.sock`, read the socket — `stat -c '%U:%G %a' /var/run/docker.sock` — and do **not** run
  `gc-setup`: it restarts docker and kills live containers.
- **The pre-commit gate is `context/plans/RULES.md` § Project gate, steps 1–5** — all of them
  model-free. Booting a model is step 6 and happens only when a task names it.

<!-- hands-plan:begin v1 -->
## Plans & Context

`context/` is this project's living memory and `context/plans/` its plan system.
**`context/plans/RULES.md` is authoritative** — where this file and RULES.md disagree, RULES.md wins.

- **Before non-trivial work:** read `context/plans/RULES.md` in full (its first table says what
  needs a plan), then `context/plans/SERIES.md` (active plans, next free number, north star).
  New plan = copy `NN_example.md` + `NN_example_tracker.md`, register it in SERIES.
- **Working a plan:** follow the tracker's Resume Instructions → pass the project gate (Rule A) →
  commit `[P<n>][T<n>][<topic>] <summary>` with the tracker row in the same commit. One task per
  commit.
- **Git:** never push; no co-author trailers; history is append-only — fix a bad commit with a new
  one, and chain `edit && git commit`. The same rule covers `$BUILD_DIR/src/*`: those four trees are
  hand-editable by design, and a `checkout --force` in one of them is not recoverable.
- **Knowledge:** read `context/AGENTS.md` for what lives where; record findings as you go, and
  harvest them before a plan moves to `completed/`.
- **Current state lives in `SERIES.md`**, not in this file.
- **Honesty:** never claim something is done, verified or recorded unless it is — on disk, in the
  command output.

This is a sub-project of `local-mind` (git root `../..`). The monorepo root has no agent file of its
own and its `context/` is empty, so nothing above this file constrains you.
<!-- hands-plan:end -->
