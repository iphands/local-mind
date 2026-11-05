<!-- hands-plan:v1 2026-11-04 -->
# basalt — Plan Series

Dependency chain and status of every plan. **Read this before creating a plan. Update it whenever
a plan is added, starts, completes, or is abandoned** — in the same commit as that change.

Plan status: `pending` | `in-progress` | `blocked` | `done` | `abandoned`.
Plans further out are intentionally light: an untouched pending plan is a **hypothesis, not a
contract** — re-read and revise it before starting (RULES.md Rule C).

**Next free plan number: `01`.** <!-- hp:next-free -->

> **North star (user directive, 2026-11-04):** serve Qwen3.8-Flash-Next through
> [Basalt](https://github.com/jesdga95/basalt), source-built from a pinned commit in our own
> container tuned for the RTX PRO 6000 (sm_120), launched by `./run` on `:8700` with a `.basalt`
> pack (`Qwen3.8-FN-NVFP4-DQ6_K` is upstream's default). Hints come from `../vllmcustom`
> (container shape, `:8700`, run-as-yourself). <!-- hp:north-star -->
>
> **Out of scope (owner decision, 2026-11-04):** the
> `jesdga/Qwen3.8-27B-QUASAR-DFlash2-nvfp4-NInfer` artifact first named for `./run`. It is a
> `.ninfer` file of the dense 27B; Basalt loads only Flash-Next `.basalt` packs
> (`context/distilled.md`). It belongs in `../ninfer`.
>
> **Ordering principle:** boot before tune — a pinned image and a `./run` that boots a pack land
> before any setting is called optimized.

## Currently Active <!-- hp:active -->

<!-- One line per in-progress plan, in the order they should be worked:
     - **Plan NN** — <title>: in progress, X/Y tasks done; next TN (<short title>).
     When nothing is in progress: "None in progress — next up: Plan NN (`/hands-plan resume NN`)." -->

None yet — start with `/hands-plan new <first piece of work>`.

## Plans <!-- hp:plans -->

Keep **Notes** to ~25 words — the detail lives in the plan's tracker.

| Plan | Title | Depends on | Status | Notes |
|------|-------|-----------|--------|-------|

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

- **The owner runs every long or GPU step** (the convention in `../ninfer` and `../vllmcustom`,
  carried here at init 2026-11-04): the image build, the pack download, every boot and every
  bench. Agents write the scripts, verify them with the gate and dry runs, and record what the
  owner pastes. A task whose behavior is runtime-only is not `done` until the owner has run it.
- **No `sudo` anywhere; everything runs as yourself over plain `docker`.** Containers run as
  uid/gid `1000:1000`. Never wrap a script in anything that scrubs the environment (`env -i`, a
  login shell). Same contract as `../vllmcustom` (its `context/distilled.md`).
- **One server on `:8700` at a time.** `../vllmcustom` (container `vllm`), `../ninfer` and
  `../ignis` all serve there; stop the running one before booting Basalt.
- **One Blackwell card.** GPU 0 is the RTX PRO 6000 (sm_120, 96 GiB); GPU 1 is an RTX 4060
  (sm_89) that Basalt cannot use — the container sees GPU 0 only. Host RAM is 125 GiB, the tier
  for experts that do not fit in VRAM (`nvidia-smi`, `free -g`, 2026-11-04).

---

Completed plans move to `completed/`, abandoned ones to `abandoned/` (RULES.md Rule C). When this
file passes ~30 KB, move the narrative of fully closed series to `completed/SERIES_ARCHIVE.md` and
leave one summary row per series here.
