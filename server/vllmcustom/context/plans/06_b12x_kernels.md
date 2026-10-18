# Plan 06 — b12x SM12x kernels as an opt-in overlay

> **Status**: in-progress
> **Created**: 2026-10-16
> **Revised**: 2026-10-17 — added T4 (`MXFP8_BACKEND` knob) after T3's first boot showed `--linear-backend b12x` is global and dies on FP4-Local's vision `linear_fc2` (K=4304).
> **Revised**: 2026-10-18 — added T5 (skip the mount on images that ship b12x). The `fe8f214` repin (`2f7fe38`) moves flashinfer to 0.7.2, which bundles b12x, and the owner chose to drop the mount there rather than make it win.
> **Depends on**: N/A
> **Goal**: `MOE_BACKEND=b12x` / `NVFP4_BACKEND=b12x` work on any launcher against today's image, with no rebuild, and the Flash-Next A/B numbers say whether b12x earns a place in the Dockerfile.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Fetch the pinned, sha256-checked `b12x` wheel (pure Python) into `$CACHE_DIR`, and have the
shared launcher plumbing mount it plus a `.pth` whenever a backend asks for `b12x`, so vLLM's
existing b12x kernels light up on sm_120.

**Deliverables**:
1. `container/patches/b12x/` — `b12x.env` (the pin), `fetch` (download + verify + unpack),
   `zz_b12x.pth`, `UPSTREAM`.
2. `b12x_args` in `common/_run-lib.sh`, called first thing in `env_extra_args`: knob `B12X=auto|0|1`,
   `auto` = mount iff `MOE_BACKEND` or `NVFP4_BACKEND` is exactly `b12x`.
3. README § on the knob; gate step 1 count `30 b` → `31 b` (RULES.md still says 29: `b9aa163` added
   `lfm2.5/run` without the bump — T1 records both).
4. Owner A/B boots (gate step 6) on both Flash-Next checkpoints, with numbers in the tracker.

**Estimated effort**: Small–Medium (half day) + owner boots.

---

## Scope

**In**: `container/patches/b12x/`, `common/_run-lib.sh` (one new function + one call), `README.md`,
`context/plans/RULES.md` (gate count line), `context/distilled.md`.
**Out**: `container/Dockerfile`, `container/versions.env`, `container/preflight`, every per-launcher
script, any vLLM source edit, b12x env tuning (`B12X_*` / `VLLM_B12X_*` stay `ENV_EXTRA` territory),
the community-fork-only `--gdn-decode-kernel b12x`. Baking b12x into the image is a follow-up gated
on T3's numbers. If the work needs to cross this line, **stop** and record the blocker in the tracker.

---

## Context

The `nvidia/Qwen3.8-Flash-Next-FP4-Local` card's RTX PRO 6000 recipe runs `--linear-backend b12x
--moe-backend b12x` (`models/vllm/nvidia/Qwen3.8-Flash-Next-FP4-Local/README.md:333,343`). Our vLLM at
`4d9a8d6` carries every b12x kernel class but treats the package as an optional extra
(`setup.py:1709` `"b12x": ["b12x==1.5.1"]`) and our Dockerfile installs no extras, so all of them
report unsupported and the backend values are dead.

### Pre-Identified Bug/Issue

Today's image has no b12x (2026-10-16):

```bash
docker run --rm --entrypoint /opt/venv/bin/python iphands/vllm-blackwell:cu1321-sm120-main-vllm0.33.0.dev20261015-g4d9a8d6 -c '
import importlib.metadata as m
for p in ("b12x","nvidia-cutlass-dsl","flashinfer-python","humming"):
    try: print(p, m.version(p))
    except Exception as e: print(p, "MISSING")
'
```
```
b12x MISSING
nvidia-cutlass-dsl 4.8.0
flashinfer-python 0.7.1
humming MISSING
```
(`humming MISSING` is a distribution-name question, not an absence: the same image serves
`Using 'HUMMING' NvFp4 MoE backend` — `log.txt`, 2026-10-16.)

With the wheel unpacked to a scratch dir and mounted via a `.pth` (no other change), on the GPU:
```
b12x 1.5.1 /opt/b12x-site/b12x/__init__.py
has_b12x True
  b12x.attention.paged         ok
  b12x.gemm.blockscaled        ok
  b12x._lib.intrinsics         ok
  b12x.gemm.mxfp8_linear       ok
  b12x.gemm.tensor_fp8_linear  ok
  b12x.moe.fused_moe           ok
mxfp8     B12xMxfp8LinearKernel: (True, None)
nvfp4     B12xNvFp4LinearKernel: (True, None)
scaled_mm B12xFp8BlockScaledMMKernel: (True, None)
scaled_mm B12xTensorFP8ScaledMMLinearKernel: (True, None)
moe B12xExperts True
```

### Why an overlay mount (not a Dockerfile line first)

Rejected for now: `pip install b12x==1.5.1` in `container/Dockerfile`. It is the end state if b12x
wins, but it costs an owner rebuild before a single number exists, and the image has no `pip`
(`/opt/venv/bin/python: No module named pip`, 2026-10-16) so even that line is not one line. The
wheel is `py3-none-any` and every dependency is already in the image at the pinned versions, so a
read-only mount + `.pth` is the whole install — the same mechanism as `draft-load-filter`. A `.pth`
**appends** to `sys.path`, so if the image ever ships b12x itself, the image copy wins and the
overlay goes inert by construction.

Rejected: `PYTHONPATH`. `BOOT_TIMING` already owns it (`-e PYTHONPATH=/opt/boot-timing`) and the
last `-e` wins.

### Key Facts

| Fact | How confirmed |
|---|---|
| b12x 1.5.1 wheel `b12x-1.5.1-py3-none-any.whl`, sha256 `8731e23571d8d45db26d78e7699ec0b23e7ba20b52487adf8c1063c8d8b53a3d`, 2,434,952 B, Apache-2.0 | `https://pypi.org/pypi/b12x/1.5.1/json` + `sha256sum -c`, 2026-10-16 |
| Its `requires_dist` (torch≥2.12, cuda-python, nvidia-cutlass-dsl{,-libs-base,-core,-cu12,-cu13}==4.8.0, apache-tvm-ffi 0.1.6–<0.2 not 0.1.8, rich≥13, transformers≥4.51, safetensors≥0.6) are all met in the image: torch 2.14.0, cuda-python 13.4.4, cutlass-dsl* 4.8.0, tvm-ffi 0.1.11, rich 15.0.0, transformers 5.19.0, safetensors 0.8.0 | `importlib.metadata.version` in the `g4d9a8d6` image, 2026-10-16 |
| vLLM pins exactly `b12x==1.5.1` | `$BUILD_DIR/src/vllm/setup.py:1709` @ `4d9a8d6` |
| `auto` never picks the b12x MoE (`B12X` is absent from `AVAILABLE_BACKENDS`); `FLASHINFER_B12X` is excluded too | `vllm/model_executor/layers/fused_moe/oracle/nvfp4.py:199-213` |
| `auto` ranks b12x linears behind FlashInfer-CUTLASS and Marlin (MXFP8) / behind 9 others (NVFP4) — installing it should not move other launchers' auto picks | `vllm/model_executor/kernels/linear/__init__.py:556-566,578-596` |
| b12x FP8 scaled-mm only takes static per-tensor or 128×128 block scales, so `ONLINE_FP8=1` (dynamic per-channel) gains nothing from `--linear-backend b12x` | `vllm/model_executor/kernels/linear/scaled_mm/b12x.py:92,227-231` |
| `--linear-backend` falls back to auto per layer type the backend cannot implement | `vllm/config/kernel.py` `linear_backend` docstring |
| b12x JIT-compiles CuTe-DSL kernels into `$XDG_CACHE_HOME`/`~/.cache` = the launcher's rw `$CACHE_DIR` mount, so a warm boot reuses them | `grep XDG_CACHE_HOME` over the unpacked wheel; `common/_run-lib.sh` `docker_core_args` |
| `--gdn-decode-kernel b12x` does not exist upstream at `4d9a8d6` | `grep -rn gdn.*b12x vllm/` → nothing, 2026-10-16 |
| End-to-end speed on this card: **unknown** | unconfirmed — settle in T3 |

### Rejected Claims

- "b12x needs a source build for sm_120." No: pure-Python wheel, kernels compile at runtime via
  CuTe DSL (`py3-none-any`; no `.so` in the unpacked tree, 2026-10-16).

---

## Step-by-Step Tasks

### T1: The pinned fetch

**File**: `container/patches/b12x/{b12x.env,fetch,zz_b12x.pth,UPSTREAM}`, `context/plans/RULES.md`
**What to do**: `b12x.env` holds `B12X_VERSION` / `B12X_WHEEL` / `B12X_WHEEL_URL` / `B12X_WHEEL_SHA256`
(plain `KEY=value`, same shape rule as `versions.env`). `fetch` refuses root, downloads into a fresh
temp dir beside the destination, verifies sha256, unpacks with `python3 -I -m zipfile -e`, checks the
dist-info version, then renames into `${CACHE_DIR:-/mnt/noir/scratch/ai/vllm/cache}/b12x/$B12X_VERSION/site`
and stamps the sha. Re-running with the stamp present is a no-op. `zz_b12x.pth` is one line:
`/opt/b12x-site`. Bump gate step 1 to `31 b`, naming both `lfm2.5/run` (`b9aa163`) and `fetch`.
**Verify**: `./container/patches/b12x/fetch` twice (first: `fetched`, second: `already present`);
a run with `B12X_WHEEL_SHA256=0000…` (forced via env) refuses and leaves no `site/`; then gate 1–5.
**Commit**: `[P6][T1][b12x] add the pinned, sha-checked b12x wheel fetch`

### T2: Launcher plumbing

**File**: `common/_run-lib.sh`, `README.md`
**What to do**: `b12x_args()` — `B12X=auto` (default) mounts iff `MOE_BACKEND` or `NVFP4_BACKEND` is
exactly `b12x` (`flashinfer_b12x` is FlashInfer's and must not trigger); `1` always; `0` never, with a
warning when a backend still says `b12x`. Missing site → print the `fetch` command, exit 1, before
any docker call. Mounts the site `:ro` at `/opt/b12x-site` and `zz_b12x.pth` into the venv's
site-packages. Called as the first line of `env_extra_args`, so the `ENV_EXTRA` loop stays last
(rule 1 of the file header). README gets the knob and the backend table.
**Verify**: gate step 7a's stub-docker harness against `./qwen3.8-flash-next/run-fp4local`-shaped
runs: (a) `MOE_BACKEND=b12x` → both `-v` lines present and `--moe-backend b12x` emitted; (b) defaults
→ no `b12x` mount; (c) `MOE_BACKEND=flashinfer_b12x` → no mount; (d) `B12X=1` with an empty
`CACHE_DIR` → refusal text naming `fetch`, exit 1. Then gate 1–5 + 7a.
**Commit**: `[P6][T2][b12x] mount b12x when a backend asks for it`

### T4: `MXFP8_BACKEND` — b12x for the MXFP8 linears only

**File**: `common/_run-lib.sh` (`nvfp4_backend_args`, `b12x_args`), `README.md`
**What to do**: `MXFP8_BACKEND=<name>` emits `--kernel-config '{"linear_backend_per_quant":{"mxfp8":"<name>"}}'`
(vLLM's own per-quant override, `vllm/config/kernel.py:385`; it wins over `--linear-backend` for that
scheme, `kernels/linear/__init__.py:252-257`). Name must match `^[a-z0-9_]+$`. `b12x_args`' `auto` also
mounts on `MXFP8_BACKEND=b12x`. Added because T3's first boot proved `--linear-backend` is global.
**Verify**: stub docker on `run-fp4local`: `MXFP8_BACKEND=b12x` → the `--kernel-config` line, the b12x
mount, no `--linear-backend`; with `MOE_BACKEND=b12x` both flags; a bad name → rc 1, no `serve`;
defaults → neither. vLLM's own `EngineArgs` parses the emitted JSON in the image. Then gate 1–5 + 7.
**Commit**: `[P6][T4][b12x] add MXFP8_BACKEND for a per-quant linear backend`

### T5: No mount on images that ship b12x

**File**: `common/_run-lib.sh` (`b12x_args`, new `_image_ships_b12x`), `README.md`,
`container/patches/b12x/UPSTREAM`, `context/distilled.md`
**What to do**: flashinfer-python ≥ 0.7.2 bundles b12x 1.5.0 and installs a top-level `b12x`
(flashinfer#5767). On such an image the appended `.pth` can never win. Read the image's
`ai.vllmcustom.flashinfer.version` label with `docker image inspect --format`, which is metadata only
and needs no container, and ask only when a mount is in question. At ≥ `B12X_IMAGE_SINCE_FLASHINFER`
(0.7.2, compared with `sort -V`), `B12X=auto` mounts nothing, skips the not-fetched refusal and says
so. `B12X=1` still mounts, with an "inert" warning, as the escape hatch. Under `B12X=0`, the warning
"vLLM will refuse it" becomes a note that the image's copy serves the backend. A missing or unparsable
label keeps the old path. Owner decision, 2026-10-18.
**Verify**: a stub `docker` that answers the label query, on `run-fp4local` and `run`:
- 0.7.2, 0.7.10 and 0.7.2.post1 with `MOE_BACKEND`/`MXFP8_BACKEND=b12x` and an empty `CACHE_DIR`:
  rc 0, no mount, the backend flag still emitted.
- 0.7.1 and 0.6.18 unfetched: rc 1 naming `fetch`.
- 0.7.1 fetched, an empty label and garbage: mounted.
- `B12X=1` on 0.7.2: mounted plus the warning.
- `B12X=0` on 0.7.2 / 0.7.1: the note / the old warning.
- Defaults: no label query at all.

The real `docker image inspect` on the `…g4d9a8d6` and `:latest` images reads 0.7.1 / 0.6.18.post1,
and both count as "does not ship". `shellcheck -x` clean. Then gate 1–5 + 7.
**Commit**: `[P6][T5][b12x] skip the b12x mount on images that ship b12x`

### T3: Owner A/B on both Flash-Next checkpoints (gate step 6)

**File**: none (tracker Evidence only)
**What to do**: owner boots; agent reads the logs and benches. One change at a time (Rule P1.4):
- FP4-Local: baseline (launcher defaults) vs `MOE_BACKEND=b12x` vs `NVFP4_BACKEND=b12x` vs both.
- NVFP4: daily driver vs the same with `MOE_BACKEND=b12x`.
**Verify**: boot log shows `Using 'B12X' NvFp4 MoE backend` / a `B12x…LinearKernel` selection, no
`hooks NOT applied`, coherent output; `./bench/bench <label>` ≥3 runs each, mean + spread.
**Expected observation**: confirm = single-stream `tg_tps` above the baseline's spread on the short
convo turns. Refute = within or below spread. Noise = anything inside the baseline's own spread —
written "within noise". A boot failure is a result too (record the error verbatim).
**Commit**: `[P6][T3][bench] record b12x A/B on Flash-Next`

---

## Critical Files

| File | Change | Priority |
|---|---|---|
| `container/patches/b12x/fetch` | new: pinned download + verify + unpack | P0 |
| `container/patches/b12x/b12x.env` | new: the pin | P0 |
| `common/_run-lib.sh` | new `b12x_args`, called from `env_extra_args` | P0 |
| `container/patches/b12x/zz_b12x.pth`, `UPSTREAM` | new | P1 |
| `README.md` | knob + backend table | P1 |
| `common/_run-lib.sh` `nvfp4_backend_args` | `MXFP8_BACKEND` → `--kernel-config` per-quant override (T4) | P1 |
| `common/_run-lib.sh` `b12x_args` / `_image_ships_b12x` | no mount when the image's flashinfer label is ≥ 0.7.2 (T5) | P1 |
| `context/plans/RULES.md` | step 1 count 29 → 31 b (`lfm2.5/run` + `fetch`) | P1 |

---

## Open Questions / Risks

1. **Does the b12x MoE accept this model's experts** (Qwen4Exp routed NVFP4 + BF16-activation MTP
   experts; on FP4-Local, CSF scales decoded by Plan 04's patch)? *How we'll settle it*: T3 boot.
2. **First-boot JIT time.** CuTe-DSL compiles per shape. *Mitigation*: cache lands in `$CACHE_DIR`;
   record cold vs warm boot time in T3.
3. **The `.pth` path is python3.12-specific** (`/opt/venv/lib/python3.12/site-packages`), like the
   existing overlays. *Mitigation*: same assumption as `draft-load-filter`; a repin that moves python
   breaks all of them together and loudly (missing module), not silently.
4. **PyPI URL rot.** *Mitigation*: sha256 pin means a moved file fails closed; the fetched tree lives
   in `$CACHE_DIR`, so a boot never touches the network.
5. **Gate step 2 is red before this plan, and prints `SKIP` while red.** `shellcheck` 0.11.0 is now at
   `/usr/local/bin/shellcheck` (SERIES still says absent), and over the tracked bash set it reports one
   error (`muse/run:355` SC2145) and one warning (`scripts/muse-quality-diff:25` SC2034), plus notes.
   The step's `command -v … && { … | xargs shellcheck; } || echo SKIP` turns a non-zero shellcheck
   into `SKIP(shellcheck not installed)` — a failing step reading as an absent tool (2026-10-16).
   *Mitigation*: out of scope here (other launchers, the gate's own text); every file this plan adds or
   edits must be `shellcheck -x` clean, checked per task. Recorded as a tracker Follow-up.

---

## Verification Checklist

- [x] T1: `fetch` prints `fetched` then `already present`; a forced bad sha refuses and leaves no `site/`.
- [x] T1: gate step 1 reads `31 b` / `25 p`, `env ok`; `shellcheck -x container/patches/b12x/fetch` prints nothing.
- [x] T2: stub-docker run with `MOE_BACKEND=b12x` emits both b12x `-v` lines; defaults and `flashinfer_b12x` emit none; `B12X=1` with no fetch exits 1 naming `fetch`.
- [x] T4: stub-docker `MXFP8_BACKEND=b12x` emits `--kernel-config {"linear_backend_per_quant":{"mxfp8":"b12x"}}` + the b12x mount; a bad name exits 1; vLLM's `EngineArgs` parses it.
- [x] T5: stub label 0.7.2 / 0.7.10 / 0.7.2.post1 → no mount, no refusal; 0.7.1 / 0.6.18 / missing / garbage → the old mount-or-refuse path; `B12X=1` mounts with a warning; defaults make no label query; real images read 0.7.1 / 0.6.18.post1.
- [ ] T3: boot log names a B12X MoE / B12x linear kernel; ≥3 bench runs per arm with mean + spread in Evidence.
- [ ] Findings harvested into `context/distilled.md` / `pitfalls.md` (Rule D).
- [ ] Plan + tracker moved to `completed/`, SERIES updated (Rule C).
