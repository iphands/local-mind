# ninfer — NInfer in a container, tuned for the RTX PRO 6000

Self-owned [NInfer](https://github.com/Neroued/ninfer) stack for the RTX PRO 6000 Blackwell
(sm_120): the engine built from a pinned commit for `sm_120a` on CUDA 13.4 in our own image, the
Qwen3.8-Flash-Next `.ninfer` artifact converted **in a container**, one launcher (`./run`) serving
on `:8700`, and measurements against the vLLM FP4-Local daily driver in `../vllmcustom`. The goal
is the fastest single-stream decode this card can do. `README.md` is the user-facing reference;
this file is the working contract.

## Build & verify

- The pre-commit gate is `context/plans/RULES.md` § Project gate — model-free, no GPU.
- **The owner runs every long or GPU step**: `./build` (image), the BF16 base download,
  `./scripts/convert`, `./run` boots and benches. Agents write and dry-run them, then record what
  the owner pastes.
- **Run everything as yourself — no `sudo`**, plain `docker`, containers as uid/gid `1000:1000`.
  Never wrap a script in anything that scrubs the environment (`env -i`, a login shell).
- **One server on `:8700` at a time.** Stop vLLM (`../vllmcustom`, container `vllm`) before
  booting NInfer or converting: both need the GPU and NInfer pins ~63 GiB of host RAM.
- `vendor/ninfer` is the pinned checkout `./build` manages. Never `checkout --force`, `reset`
  or `clean` it; a dirty checkout is a refusal.

<!-- hands-plan:begin v1 -->
## Plans & Context

`context/` is this project's living memory and `context/plans/` its plan system.
**`context/plans/RULES.md` is authoritative** — where this file and RULES.md disagree, RULES.md wins.

- **Before non-trivial work:** read `context/plans/RULES.md` in full (its first table says what
  needs a plan), then `context/plans/SERIES.md` (active plans, next free number, north star).
  New plan = copy `NN_example.md` + `NN_example_tracker.md`, register it in SERIES.
- **Working a plan:** follow the tracker's Resume Instructions → pass the project gate
  (Rule A) → commit `[P<n>][T<n>][<topic>] <summary>` with the tracker row in the same commit. One
  task per commit.
- **Git:** never push — and never offer to; no co-author trailers; history is append-only — fix a
  bad commit with a new one, and chain `edit && git commit`. Report the commit hash and that you
  did not run `git push`, nothing about the remote.
- **Knowledge:** read `context/AGENTS.md` for what lives where; record findings as you go, and
  harvest them before a plan moves to `completed/`.
- **Current state lives in `SERIES.md`**, not in this file.
- **Honesty:** never claim something is done, verified or recorded unless it is — on disk, in the
  command output.

This is a sub-project of `local-mind` (git root `../..`). The monorepo root has no agent file of
its own and its `context/` is empty, so nothing above this file constrains you. The sibling
`../vllmcustom` is a separate project with its own rules; read its `context/` for host facts, but
its plans and gate do not govern this one.
<!-- hands-plan:end -->
