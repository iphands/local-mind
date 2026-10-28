# ignis — Ignis in a container, tuned for the RTX PRO 6000

Launchers for [Ignis](https://github.com/gpillon/ignis) (a single-GPU Rust + C++/CUDA engine for
Qwen3.8, `SM120a` only) on the RTX PRO 6000 Blackwell: one launcher per model — `qwen38fn/run`
for Qwen3.8-Flash-Next, one for the Qwen3.8-27B — serving on `:8700` with the vLLM daily driver's
model names so `llama-proxy` needs no change, and measurements against the vLLM FP4-Local daily
driver in `../vllmcustom`. The goal is the fastest decode this card can do. `README.md` is the
user-facing reference (including how to download the Ignis weights); this file is the working
contract.

## Build & verify

- The pre-commit gate is `context/plans/RULES.md` § Project gate — model-free, no GPU, no image
  pull.
- **The owner runs every long or GPU step**: the image pull, every weights download, every boot
  and bench. Agents write and dry-run the launchers, then record what the owner pastes.
- **Run everything as yourself — no `sudo`**, plain `docker`. Never wrap a script in anything
  that scrubs the environment (`env -i`, a login shell).
- **One server on `:8700` at a time.** Stop vLLM (`../vllmcustom`, container `vllm`) or NInfer
  (`../ninfer`) before booting Ignis: each wants the whole card.

<!-- hands-plan:begin v1 -->
## Plans & Context

`context/` is this project's living memory and `context/plans/` its plan system.
**`context/plans/RULES.md` is authoritative** — where this file and RULES.md disagree, RULES.md wins.

- **Before non-trivial work:** read `context/plans/RULES.md` in full (its first table says what
  needs a plan), then `context/plans/SERIES.md` (active plans, next free number, north star).
  New plan = copy `NN_example.md` + `NN_example_tracker.md`, register it in SERIES.
- **Working a plan:** follow the tracker's Resume Instructions → pass the project gate
  (Rule A) → commit `[ignis][P<n>][T<n>][<topic>] <summary>` with the tracker row in the same
  commit. One task per commit.
- **Git:** never push — and never offer to; no co-author trailers; history is append-only — fix a
  bad commit with a new one, and chain `edit && git commit`. Stage only `server/ignis/…` paths.
- **Knowledge:** read `context/AGENTS.md` for what lives where; record findings as you go, and
  harvest them before a plan moves to `completed/`.
- **Current state lives in `SERIES.md`**, not in this file.
- **Honesty:** never claim something is done, verified or recorded unless it is — on disk, in the
  command output.

This is a sub-project of `local-mind` (git root `../..`). The monorepo root has no agent file of
its own and its `context/` is empty, so nothing above this file constrains you. The siblings
`../vllmcustom` and `../ninfer` are separate projects with their own rules; read their `context/`
for host facts and prior numbers, but their plans and gates do not govern this one.
<!-- hands-plan:end -->
