# Plan 02 — `./run` profile for Qwen3.8-27B NVFP4 (neroued's published artifact)

> **Status**: pending
> **Created**: 2026-10-27
> **Depends on**: Plan 01 T5 (`run` exists); the boot (T2) also needs Plan 01 T6's image
> **Goal**: `MODEL=27b ./run` serves neroued's published `qwen3_8_27b_nvfp4.ninfer` on `:8700` as `cosmo-6000`, with flags right for a resident dense model, while `./run` with no `MODEL` keeps Plan 01's Flash-Next behavior byte for byte.
> **Agent**: implementation agent (T1) · owner runs (T2: the boot)

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Teach `./run` a second model. The owner downloaded `neroued/Qwen3.8-27B-nvfp4-NInfer`
(an official, ready-made v3 artifact — no conversion); `./run` refuses it today because it ships
no `.conversion.json`, and its Flash-Next flags are wrong for it anyway.

**Deliverables**:
1. `run`: `MODEL=flash-next|27b` profiles (default `flash-next`, unchanged); a `SPEC=mtp|dflash2|off`
   and `LM_HEAD_DRAFT` knob; an artifact identity check that reads `.conversion.json` **or** the
   published `artifact-manifest.json` (size, container v3, architecture, components) (T1).
2. Gate step 6 covers both profiles; `README.md` documents `MODEL=27b` (T1).
3. A first owner boot of `MODEL=27b ./run`, recorded (T2).

**Estimated effort**: Small — T1 is a couple of hours; T2 is one owner boot once Plan 01 T6's
image exists.

---

## Scope

**In**: `run`, gate step 6 in `RULES.md`, `README.md`, `context/`. **Out**: benchmarking the 27B
(a Follow-up — this plan makes it bootable, not tuned), DFlash2 tuning, any change to Plan 01's
Flash-Next defaults, `../vllmcustom`, `llama-proxy`. If the work must cross this line, stop and
record a blocker.

---

## Context

### Pre-Identified Bug/Issue

`./run` cannot take the artifact. With gate 6's stub `docker` on `PATH` (no image is built yet):

```bash
$ DRY_RUN=1 ./run models/vllm/neroued/Qwen3.8-27B-nvfp4-NInfer/qwen3_8_27b_nvfp4.ninfer   # stub docker on PATH
## no qwen3_8_27b_nvfp4.ninfer.conversion.json beside the artifact; cannot check its parts.
rc=1
```

Past that, the Flash-Next flags would be wrong: no `--lm-head-draft` (the 27B has the optimized
proposal head; upstream's MTP numbers use it), a 63 GiB-expert RAM warning and an NFS-PLE note
that do not apply to a resident dense model, and no way to pick DFlash2.

### Why profiles in one `./run`, and the manifest as identity

- **One launcher, `MODEL=` profiles**, not a second script: the docker plumbing, guards, seccomp
  and knobs are identical; only defaults and checks differ. A copy would drift. (Per-model
  launchers are what `../vllmcustom` does for vLLM-shaped differences; NInfer's are flags.)
- **`artifact-manifest.json` as the published identity**: neroued ships it (schema v2) with the
  file's exact bytes and SHA-256, container version, architecture and components. Checking size +
  architecture + components on every start is cheap; a full SHA-256 is a one-time T1 evidence row,
  not a per-boot cost.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| Artifact | `qwen3_8_27b_nvfp4.ninfer`, one file, 23,719,715,844 B, SHA-256 `74d2c571…`, container v3, `Qwen3_5ForCausalLM`, components text/vision/mtp/dflash2 | `artifact-manifest.json` from HF, byte-identical (as JSON) to `vendor/ninfer/model-cards/Qwen3.8-27B-nvfp4-NInfer/` @ `64d10fbf`, 2026-10-27 |
| Engine floor | runtime `minimum_revision` `98dada0e` (2026-09-15) — an ancestor of our pin `64d10fbf` | `git merge-base --is-ancestor 98dada0e 64d10fbf` → true, 2026-10-27 |
| Download | complete at `models/vllm/neroued/Qwen3.8-27B-nvfp4-NInfer/` (size = manifest bytes; `.cache/huggingface/download/` holds only `.lock`/`.metadata`) | `ls -la`, 2026-10-27 |
| No `.conversion.json` | the HF repo has `artifact-manifest.json`, `SHA256SUMS`, `README.md`, `LICENSE` | HF API file list, 2026-10-27 |
| Upstream serve profile (RTX 5090, 32 GB) | `--max-context 240000 --kv-capacity 240000 --max-concurrency 2 --kv-dtype fp8 --device-state-slots 2 --spec mtp --draft-tokens 3 --lm-head-draft --preserve-thinking` | pinned model card (`c772812b`) [DOC] |
| HF README is ahead of the pin | it adds `--host-state-slots 8 --host-kv-mib 8192`; neither exists in `src/serve/serve_options.cpp` @ `64d10fbf` | `grep -c` → `0`, 2026-10-27 — use the pinned card |
| Residency | dense, all weights on the GPU; `--kv-capacity auto` "maximizes resident-model KV" | `docs/cli.md` § Common options [DOC] |
| Speculation | MTP 1..5 or DFlash2 1..15 (recommended 7); `--lm-head-draft` optional and needs a selected backend | manifest `runtime.dflash2_draft_tokens`; `docs/cli.md` § Speculative decoding [DOC] |
| Model name | `:8700` is `cosmo-6000` to `llama-proxy`, whatever model serves it | `../../llama-proxy/config.yaml:30-31` [REPO] |
| Report shape | our converter writes `components` keyed by name, with `text.config.architectures` | `tools/convert/pipeline.py:64` @ `64d10fbf` [SOURCE] |

---

## Step-by-Step Tasks

### T1: `MODEL` profiles, `SPEC`/`LM_HEAD_DRAFT`, manifest-aware identity check

**File**: `run`, `context/plans/RULES.md` (gate step 6), `README.md`, `context/distilled.md`

**What to do**:
- `MODEL` (default `flash-next`) selects defaults: `ARTIFACT`, the expected architecture prefix
  (`Qwen4Exp` / `Qwen3_5ForCausalLM`), `SPEC` (`mtp` both), `DRAFT_TOKENS` (3; 7 when
  `SPEC=dflash2`), `LM_HEAD_DRAFT` (0 / 1), and which host notes apply (Flash-Next: 63 GiB
  expert RAM warning, NFS-PLE note; 27B: neither). Every other knob keeps its Plan 01 default.
- `SPEC=mtp|dflash2|off`; `DRAFT_TOKENS=0` still means off (gate 6 relies on it). Ranges: MTP
  1..5, DFlash2 1..15. Flash-Next refuses `dflash2` and `LM_HEAD_DRAFT=1`. `--lm-head-draft` is
  emitted only with a backend selected.
- Identity: read `<entry>.conversion.json` if present (files by basename, architecture from
  `components.text.config.architectures`, component names), else `artifact-manifest.json` beside
  the entry when its `artifact.filename` is the entry (exact `bytes`, `container_version` 3,
  `architecture`, `components`). Refuse: neither present; a missing file or a size mismatch; an
  architecture that does not match `MODEL`; `VISION=1` or the selected `SPEC` without its
  component.
- Default `./run` (Flash-Next) must emit exactly the Plan 01 command.

**Before**: `run` @ `c863e39` — one profile, `.conversion.json` required (output above).

**Verify**:
```bash
DRY_RUN=1 MODEL=27b PATH=<stub>:$PATH ./run   # → rc 0; --lm-head-draft, --spec mtp --draft-tokens 3, no MemAvailable warning, no NFS-PLE note
DRY_RUN=1 MODEL=27b SPEC=dflash2 …            # → --spec dflash2 --draft-tokens 7 --lm-head-draft
DRY_RUN=1 MODEL=flash-next ./run <27b entry>  # → refuses: architecture Qwen3_5ForCausalLM is not Qwen4Exp*
sha256sum models/vllm/neroued/Qwen3.8-27B-nvfp4-NInfer/qwen3_8_27b_nvfp4.ninfer   # → 74d2c571… (SHA256SUMS)
# + project gate steps 1–6 (step 6 extended: a fake 27B tree with a manifest; the Flash-Next command unchanged)
```

**Commit**: `[ninfer][P2][T1][run] add MODEL=27b profile for the NVFP4 artifact`

### T2: Owner boots `MODEL=27b ./run`; record it

**File**: tracker Evidence, `context/distilled.md`, `README.md`

**What to do**: Needs Plan 01 T6's image. Owner: stop vLLM, `MODEL=27b LOG_LEVEL=debug ./run`,
paste the startup log (KV capacity chosen by `auto`, StateImages, Engine-ready line with the sync
mode), then `curl -s localhost:8700/v1/models` and one chat request with `"model":"cosmo-6000"`.

**Verify**: `/v1/models` lists `cosmo-6000` with `max_model_len` 262144; the chat returns
`reasoning_content` and `content`; the boot log names the RTX PRO 6000. + project gate 1–6.

**Expected observation**:
- **Confirms if**: it boots with MTP + `--lm-head-draft` + vision at C=4, and `auto` picks a KV
  capacity well above one 262,144-token context (resident 22 GiB of weights on 96 GB).
- **Refutes if**: the boot rejects a flag or a component (record the verbatim error; fix `run` in
  this task's commit), or io_uring/memlock fails (seccomp/ulimit).
- **Within noise if**: n/a (not a measurement).

**Commit**: `[ninfer][P2][T2][run] record first Qwen3.8-27B boot`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `run` | `MODEL` profiles, `SPEC`/`LM_HEAD_DRAFT`, manifest-aware identity check | P0 |
| `context/plans/RULES.md` | gate step 6 covers the 27B profile; the Flash-Next command is pinned | P0 |
| `README.md` | `MODEL=27b` usage | P1 |
| `context/distilled.md` | the 27B artifact facts | P2 |

---

## Open Questions / Risks

1. **Risk: the default Flash-Next command changes by accident.** *Mitigation*: gate 6 keeps
   asserting every Plan 01 flag on the default run, and T1 diffs the default dry-run command
   before and after.
2. **Question: best 27B settings on 96 GB (C, KV dtype, MTP vs DFlash2)?** deferred — this plan
   makes it bootable; measuring is a Follow-up (P1 rules apply).
3. **Risk: the artifact lives under `models/vllm/`** (the owner's download location), not
   `models/ninfer/`. *Mitigation*: the default follows where it is; `ARTIFACT=` overrides.

---

## Verification Checklist

- [ ] T1: `MODEL=27b` dry-run emits `--lm-head-draft` and `--spec mtp --draft-tokens 3`, no Flash-Next host notes; `SPEC=dflash2` → `--draft-tokens 7`; Flash-Next profile refuses the 27B artifact by architecture; the default command is unchanged; the file's SHA-256 matches `SHA256SUMS`; gate 1–6 pass
- [ ] T2: owner boot answers `/v1/models` (`cosmo-6000`, 262144) and a chat with reasoning + content
- [ ] All: the project gate (Rule A) passes on the final commit, every applicable step `pass`
- [ ] All: the runtime tier ran (owner), **or** the tracker says the behavior went unverified at runtime
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
