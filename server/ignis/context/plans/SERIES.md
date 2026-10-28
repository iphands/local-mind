<!-- hands-plan:v1 2026-10-27 -->
# ignis — Plan Series

Dependency chain and status of every plan. **Read this before creating a plan. Update it whenever
a plan is added, starts, completes, or is abandoned** — in the same commit as that change.

Plan status: `pending` | `in-progress` | `blocked` | `done` | `abandoned`.
Plans further out are intentionally light: an untouched pending plan is a **hypothesis, not a
contract** — re-read and revise it before starting (RULES.md Rule C).

**Next free plan number: `02`.** <!-- hp:next-free -->

> **North star (user directive, 2026-10-27):** serve Qwen3.8-Flash-Next — and the Qwen3.8-27B —
> through [Ignis](https://github.com/gpillon/ignis) on the one RTX PRO 6000 (sm_120) at the
> fastest decode the card can do, from one launcher per model (`qwen38fn/run` for Flash-Next) on
> `:8700` with the vLLM daily driver's model names, so `llama-proxy` needs no change; the README
> says how to download the Ignis weights, and the numbers are measured against the vLLM FP4-Local
> daily driver in `../vllmcustom`. <!-- hp:north-star -->
>
> **Ordering principle:** a launcher that boots and answers correctly lands before any speed
> claim; every "faster" setting is measured against a same-session vLLM FP4-Local baseline (P1).

## Currently Active <!-- hp:active -->

<!-- One line per in-progress plan, in the order they should be worked:
     - **Plan NN** — <title>: in progress, X/Y tasks done; next TN (<short title>).
     When nothing is in progress: "None in progress — next up: Plan NN (`/hands-plan resume NN`)." -->

None in progress — next up: Plan 01 (`/hands-plan resume 01`).

## Plans <!-- hp:plans -->

Keep **Notes** to ~25 words — the detail lives in the plan's tracker.

| Plan | Title | Depends on | Status | Notes |
|------|-------|-----------|--------|-------|
| 01 | Ignis launchers for Flash-Next and the 27B on :8700 | — | pending | Pinned ghcr 0.8.0 image, `qwen38fn/run` + `qwen38-27b/run` as `cosmo-6000`, README download how-to; owner-run boots + same-session bench vs FP4-Local. |

<!-- Add these sections when they earn their place:

## Dependency Chain
```text
01_foundation
    ├── 02_feature_a
    │     └── 04_feature_c
    └── 03_feature_b
```

## Milestones
- After **01**: <an observable capability, phrased so it can be checked>.

## Execution Order
1. <which plans, in what order, and what may run in parallel>

## Backlog Rationale
- **NN — Title.** Why it exists and what it is blocked on, so whoever picks it up starts with context.
-->

## Abandoned / Superseded <!-- hp:abandoned -->

| Plan | Reason | Superseded by | Date |
|------|--------|---------------|------|

*(Record the reason. A plan dropped without one gets re-attempted six months later by someone who
doesn't know it failed.)*

## Standing Constraints <!-- hp:constraints -->

- **The owner runs every long or GPU step** (decision 2026-10-27): the image pull, every weights
  download (the README documents the commands; agents never run them), every boot and every
  bench. Agents write the launchers, verify them with stubs and dry runs, and record what the
  owner pastes. A task whose behavior is runtime-only cannot be closed as verified until the owner
  has run it.
- **The per-commit gate never needs a model loaded** (decision 2026-10-27) — no boot, no image
  pull, no `ignis-server --help` probe of the image in the gate.
- **No `sudo` anywhere; everything runs as yourself over plain `docker`** (socket `root:docker
  660`, docker 29.8.0, 2026-10-27). Never wrap a script in anything that scrubs the environment
  (`env -i`, a login shell). Same contract as `../vllmcustom`.
- **One server on `:8700` at a time.** On 2026-10-27 the vLLM container `vllm` publishes
  `0.0.0.0:8700->8799/tcp` (`docker ps`); `../ninfer` targets `:8700` too. Stop the other server
  before booting Ignis — they all want the whole RTX PRO 6000.
- **Host (2026-10-27):** AMD Ryzen 9 5950X (`lscpu`; no `avx512f` in `/proc/cpuinfo`), 125 GiB RAM
  (`free -g`), GPU 0 = RTX PRO 6000 Blackwell Workstation, 97,887 MiB; GPU 1 = RTX 4060, 8,188 MiB,
  **never used here** — Ignis picks a profile from the card it sees, so the container sees GPU 0
  only. Driver 615.78.08 (`nvidia-smi`). Models tree `/mnt/noir/scratch/ai/llm/models` on NFS 4.2
  (`findmnt`), holding `comfy/`, `gguf/`, `vllm/` and no Ignis artifacts yet.

---

Completed plans move to `completed/`, abandoned ones to `abandoned/` (RULES.md Rule C). When this
file passes ~30 KB, move the narrative of fully closed series to `completed/SERIES_ARCHIVE.md` and
leave one summary row per series here.
