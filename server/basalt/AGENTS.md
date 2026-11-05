# basalt — Basalt in a container, tuned for the RTX PRO 6000

Self-owned [Basalt](https://github.com/jesdga95/basalt) stack for the RTX PRO 6000 Blackwell
(sm_120): the engine built from a pinned commit in our own image, a Qwen3.8-Flash-Next `.basalt`
pack, one launcher (`./run`) serving on `:8700`. Basalt runs Flash-Next only; `.ninfer` artifacts
(e.g. the QUASAR 27B) belong in `../ninfer`. `README.md` is the user-facing reference (once it
exists); this file is the working contract.

## Build & verify

- The pre-commit gate is `context/plans/RULES.md` § Project gate — model-free, no GPU.
- **The owner runs every long or GPU step**: the image build, the pack download, `./run` boots
  and benches. Agents write and dry-run them, then record what the owner pastes.
- **Run everything as yourself — no `sudo`**, plain `docker`, containers as uid/gid `1000:1000`.
  Never wrap a script in anything that scrubs the environment (`env -i`, a login shell).
- **One server on `:8700` at a time.** `../vllmcustom`, `../ninfer` and `../ignis` serve there
  too; stop the running one before booting Basalt.

<!-- hands-plan:begin v1 -->
## Plans & Context

`context/` is this project's living memory and `context/plans/` its plan system.
**`context/plans/RULES.md` is authoritative** — where this file and RULES.md disagree, RULES.md wins.

- **Before non-trivial work:** read `context/plans/RULES.md` in full (its first table says what
  needs a plan), then `context/plans/SERIES.md` (active plans, next free number, north star).
  New plan = copy `NN_example.md` + `NN_example_tracker.md`, register it in SERIES.
- **Working a plan:** follow the tracker's Resume Instructions → pass the project gate
  (Rule A) → commit `[basalt][P<n>][T<n>][<topic>] <summary>` with the tracker row in the same
  commit. One task per commit.
- **Git:** never push; no co-author trailers; history is append-only — fix a bad commit with a
  new one, and chain `edit && git commit`.
- **Knowledge:** read `context/AGENTS.md` for what lives where; record findings as you go, and
  harvest them before a plan moves to `completed/`.
- **Current state lives in `SERIES.md`**, not in this file.
- **Honesty:** never claim something is done, verified or recorded unless it is — on disk, in the
  command output.

This is a sub-project of `local-mind` (git root `../..`). The monorepo root has no agent file of
its own and its `context/` is empty, so nothing above this file constrains you. The siblings
`../vllmcustom` and `../ninfer` are separate projects with their own rules; read their `context/`
for host facts and container conventions, but their plans and gates do not govern this one.
<!-- hands-plan:end -->
