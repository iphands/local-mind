<!-- hands-plan:v1 2026-10-27 -->
# ninfer — Plan Series

Dependency chain and status of every plan. **Read this before creating a plan. Update it whenever
a plan is added, starts, completes, or is abandoned** — in the same commit as that change.

Plan status: `pending` | `in-progress` | `blocked` | `done` | `abandoned`.
Plans further out are intentionally light: an untouched pending plan is a **hypothesis, not a
contract** — re-read and revise it before starting (RULES.md Rule C).

**Next free plan number: `03`.** <!-- hp:next-free -->

> **North star (user directive, 2026-10-27):** the fastest single-stream decode of
> Qwen3.8-Flash-Next on the one RTX PRO 6000 (sm_120) through NInfer — source-built and pinned in
> our own container, converted in a container, launched by `./run` on `:8700` — measured against
> the vLLM FP4-Local daily driver in `../vllmcustom`. <!-- hp:north-star -->
>
> **Ordering principle:** the measurement has to exist before the tuning claim — a pinned image,
> a converted artifact and a same-session vLLM baseline land before any "fastest" setting is
> chosen.

## Currently Active <!-- hp:active -->

<!-- One line per in-progress plan, in the order they should be worked:
     - **Plan NN** — <title>: in progress, X/Y tasks done; next TN (<short title>).
     When nothing is in progress: "None in progress — next up: Plan NN (`/hands-plan resume NN`)." -->

- **Plan 01** — NInfer Flash-Next container serving: in progress, 5/12 tasks done; Wave 1 done. **Blocked on the owner**: T6 (`./build`); T7 (BF16 download + `./scripts/convert`) next.
- **Plan 02** — `./run` profile for Qwen3.8-27B NVFP4: in progress, 1/2 tasks done. **Blocked**: T2 (owner boots `MODEL=27b ./run`) needs Plan 01 T6's image.

## Plans <!-- hp:plans -->

Keep **Notes** to ~25 words — the detail lives in the plan's tracker.

| Plan | Title | Depends on | Status | Notes |
|------|-------|-----------|--------|-------|
| 01 | NInfer Flash-Next: pinned sm_120a image, containerized conversion, `./run`, measured | — | in-progress | Wave 1 scripts (agent); then owner builds, downloads BF16 base, converts, boots; A/B vs vLLM FP4-Local picks the defaults. |
| 02 | `./run` profile for Qwen3.8-27B NVFP4 (neroued's published artifact) | 01 T5 (boot: 01 T6) | in-progress | `MODEL=27b` profile + manifest-aware identity check; owner boots once Plan 01's image exists. |

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

- **The owner runs every long or GPU step** (decision 2026-10-27): the NInfer image build, the
  BF16 base download, the conversion, every boot and every bench. Agents write the scripts, verify
  them with stubs and dry runs, and record what the owner pastes. A task whose behavior is
  runtime-only cannot be closed as verified until the owner has run it.
- **No `sudo` anywhere; everything runs as yourself over plain `docker`.** Containers run as uid/gid
  `1000:1000`. Never wrap a script in anything that scrubs the environment (`env -i`, a login
  shell). Same contract as `../vllmcustom` (its `context/distilled.md`).
- **One server on `:8700` at a time.** vLLM (`../vllmcustom`, container `vllm`) must be down to
  boot NInfer or to convert: both need the GPU, and NInfer pins ~63 GiB of host RAM for the expert
  backing (`docs/cli.md` at `64d10fbf`).
- **Docker's default seccomp profile blocks `io_uring`** (`io_uring_setup` → `EPERM`, docker
  29.8.0, 2026-10-27) and NInfer's direct-I/O reader has no fallback. Every NInfer container runs
  under the project's seccomp profile.
- **Host (2026-10-27):** Ryzen 9 5950X (AVX2, **no AVX-512**), 126 GiB RAM, RTX PRO 6000 96 GB as
  device 0 plus an RTX 4060 (never used here), driver 615.78.08 (CUDA 13.4 UMD), models on
  NFS-over-RDMA (`/mnt/noir/scratch`), local NVMe `/home` (btrfs, ~319 GB free).

---

Completed plans move to `completed/`, abandoned ones to `abandoned/` (RULES.md Rule C). When this
file passes ~30 KB, move the narrative of fully closed series to `completed/SERIES_ARCHIVE.md` and
leave one summary row per series here.
