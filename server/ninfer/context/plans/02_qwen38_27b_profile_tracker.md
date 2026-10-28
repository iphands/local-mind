# Plan 02 — `./run` profile for Qwen3.8-27B NVFP4 — Tracker

## Overview

- Status: 50% complete (1/2 tasks)
- Start date: 2026-10-27
- Plan: `context/plans/02_qwen38_27b_profile.md`
- Evidence: dry-run output and the artifact's `sha256sum` (T1); the owner's pasted boot log and
  curl output (T2). Test environment per row: image tag + `ai.ninfer.ref`, the artifact path, the
  launcher's echoed `docker run` line.

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), `context/AGENTS.md`, then
   `02_qwen38_27b_profile.md`. NInfer source at the pin: `vendor/ninfer`.
2. Environment: work from `server/ninfer`. `docker` as yourself (no `sudo`). T1 needs no GPU and
   no image (gate 6 uses a stub `docker`). Standing Constraints in `SERIES.md` apply.
3. Ordering: T1 → T2. T2 needs Plan 01 T6's image and the owner; when blocked, mark it `blocked`
   with what is needed, and stop.
4. Commit per task as `[ninfer][P2][T<n>][<topic>] <summary>`, with **this tracker's row in the
   same commit**. Never push; report the hash and that you did not run `git push`.
5. Restore the system state: no leftover `ninfer` container, `/tmp/ninfer-gate*` removed.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `MODEL` profiles, `SPEC`/`LM_HEAD_DRAFT`, manifest-aware identity check | `run`, `RULES.md` gate 6, `README.md` | done | Real artifact: `MODEL=27b` dry-run rc 0 with `--spec mtp --draft-tokens 3 --lm-head-draft`, no host notes; `SPEC=dflash2` → 7; Flash-Next profile refuses it by architecture; default Flash-Next command byte-identical to `c863e39`'s; SHA-256 OK. Gate 1–6 pass (gate 6 +9 cases). **Not booted** (T2). |
| 2 | T2: Owner boots `MODEL=27b ./run`; record it | Evidence | pending | needs Plan 01 T6's image |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

**Mandatory in this project** (P1).

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-27 | T1: `sha256sum -c SHA256SUMS` in `models/vllm/neroued/Qwen3.8-27B-nvfp4-NInfer/` (NFS) | `qwen3_8_27b_nvfp4.ninfer: OK` (`74d2c571…`) | 1 | 16.8 s wall for 23.7 GB |
| 2026-10-27 | T1: `DRY_RUN=1 ./run <fake Flash-Next entry>` (stub docker) before (`c863e39`) and after | `diff` of the two launch lines: identical | 1 | proves the default profile did not move |
| 2026-10-27 | T1: `DRY_RUN=1 MODEL=27b ./run` on the real download (stub docker) | `== artifact ok … (1 files; artifact-manifest.json: Qwen3_5ForCausalLM, text,vision,mtp,dflash2)`; launch line ends `--spec mtp --draft-tokens 3 --lm-head-draft --vision --preserve-thinking --request-log-jsonl /storage/requests.jsonl` | 1 | |

## Notes / Deviations

- 2026-10-27 (planning): requested by the owner mid-Plan 01 ("wire the ./run script up to use
  these"), while downloading `neroued/Qwen3.8-27B-nvfp4-NInfer`. A launcher change needs a plan
  (RULES § When Does a Change Need a Plan?), hence this one.
- 2026-10-27 (T1): the identity check replaced the `.conversion.json`-only part check: it reads our
  converter's report (files by basename, `components.text.config.architectures`, component names)
  or the publisher's `artifact-manifest.json` (exact bytes, container v3, architecture,
  components). An unknown architecture or component list is a printed note, not a refusal. The
  default Flash-Next profile kept its exact launch line (Evidence); gate 6's Flash-Next fixture
  gained a `components` block so it exercises the new architecture/component checks too.
- 2026-10-27 (T1): the 27B keeps Flash-Next's context/capacity/concurrency defaults (262144 /
  `auto` / 4) rather than upstream's 5090 profile (240000 / 240000 / 2): the 96 GB card has the room,
  and nothing is measured yet — a Follow-up, not a claim.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| 27B bench vs the vLLM 27B and vs Flash-Next on NInfer; C / KV dtype / MTP vs DFlash2 arms | this plan makes it bootable, not tuned | a later plan |
