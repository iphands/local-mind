# Plan 06 — b12x SM12x kernels as an opt-in overlay — Tracker

## Overview

- Status: 80% complete (4/5 tasks)
- Start date: 2026-10-16
- Plan: `context/plans/06_b12x_kernels.md`
- Evidence: the launcher's echoed `docker … serve …` line (T2), owner boot logs + `bench/bench-results.jsonl`
  rows (T3). Image `iphands/vllm-blackwell:cu1321-sm120-main-vllm0.33.0.dev20261015-g4d9a8d6`
  (`ai.vllmcustom.vllm.ref` = `VLLM_MAIN_SHA` `4d9a8d6`), b12x 1.5.1.

## Resume Instructions

1. Read `CLAUDE.md`, `context/plans/RULES.md` (in full), then `06_b12x_kernels.md`.
2. Environment: `CACHE_DIR` (default `/mnt/noir/scratch/ai/vllm/cache`) is where the wheel unpacks;
   `B12X`, `MOE_BACKEND`, `NVFP4_BACKEND` are the knobs. SERIES Standing Constraints: no `sudo`; no
   model load in the per-commit gate (T3 is step 6, owner-run); `shellcheck` absent → step 2 `skip`.
3. Ordering: T1 → T2 (T2's refusal path needs T1's `fetch` to name) → T3 (owner boots).
4. Commit per task as `[P6][T<n>][<topic>] <summary>`, with **this tracker's row in the same commit**.
5. Restore the system state at the end of the session (Rule P2.4): unset `B12X` / backend knobs.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: The pinned fetch | `container/patches/b12x/` | done | `fetch` → `fetched`, rerun → `already present`, bad-sha copy → refused rc 1, no `site/`, no temp left; tree owned `iphands:iphands`; `shellcheck -x` clean |
| 2 | T2: Launcher plumbing | `common/_run-lib.sh`, `README.md` | done | stub-docker on `run-fp4local`: (a) both `-v` b12x lines + `--moe-backend b12x --linear-backend b12x`; (b) defaults: 0 mounts, `--moe-backend humming`; (c) `flashinfer_b12x`: 0 mounts; (d) `B12X=1` unfetched: rc 1 naming `fetch`, no `serve` line; (e) `B12X=0`+b12x: warning, no mount; (f) bogus: rc 1; (g) `ENV_EXTRA` still after the mounts. `run` (NVFP4) with `ONLINE_FP8=1 SM120_SKINNY=1 MOE_BACKEND=b12x`: mounted. Step 6 not run — runtime is T3 |
| 4 | T4: `MXFP8_BACKEND` knob | `common/_run-lib.sh`, `README.md` | done | stub docker: `MXFP8_BACKEND=b12x` → `--kernel-config {"linear_backend_per_quant":{"mxfp8":"b12x"}}`, b12x mounted, no `--linear-backend`; `+MOE_BACKEND=b12x` → both; `b12x;rm` → rc 1, 0 `serve` lines; defaults → neither. In-image `EngineArgs.from_cli_args` → `{'mxfp8': 'b12x'}`, `moe: b12x`. Step 6 not run |
| 5 | T5: No mount on images that ship b12x | `common/_run-lib.sh`, `README.md`, `container/patches/b12x/UPSTREAM` | done | 13 stub-docker cases (see Evidence). Label ≥ 0.7.2 → no mount and no `fetch` refusal, backend flags intact. ≤ 0.7.1 / missing / garbage → the old path. `B12X=1` mounts + warns inert. `B12X=0` on 0.7.2 notes the image's copy instead of the false refusal warning. Defaults make 0 label queries. Real images: `…g4d9a8d6` 0.7.1, `:latest` 0.6.18.post1 → both keep the mount. Runtime on a fe8f214 image: not verified (not built) |
| 3 | T3: Owner A/B on both Flash-Next checkpoints | tracker only | in-progress | arm `MOE_BACKEND=b12x NVFP4_BACKEND=b12x` on FP4-Local: **boot failed** in encoder profiling (`ValueError: A16 requires stored K32 and N8`, vision `linear_fc2` K=4304); MoE never reached. Boot 2 (`MOE_BACKEND=b12x MXFP8_BACKEND=b12x`) boots and serves one request, then **segfaults at 2 concurrent requests** (unattributed). On `…gfe8f214` (flashinfer's bundled b12x) `MOE_BACKEND=b12x` served 4 parallel conversations with no crash (owner report, 2026-10-18; the MXFP8-b12x arm that crashed was not re-run). **FP4-Local arm measured (2026-10-18): b12x MoE decodes 9–14% slower than humming and wins long-prompt prefill by ~10%, and K=4 does not beat K=3 on either backend** (Evidence). Open: the NVFP4 arm (daily driver vs `MOE_BACKEND=b12x`) |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-18 | T5: stub `docker` answering the `ai.vllmcustom.flashinfer.version` label query from `STUB_FI`, on `run-fp4local` (and `run` for k, l), `IMAGE_TAG=…g4d9a8d6`; empty scratch `CACHE_DIR` for "unfetched", the real T1 site for "fetched" | a 0.7.2 moe unfetched: rc 0, 0 mounts, `--moe-backend b12x`, `## b12x: the image ships its own`. b 0.7.1 unfetched: rc 1, `fetch it once`. c 0.7.1 fetched: mounted. d empty label: mounted. e 0.7.2 `B12X=1`: mounted + inert warning. f 0.7.2 `B12X=0`: note, no mount. g 0.7.1 `B12X=0`: old warning. h defaults: 0 label queries, 0 mounts. i 0.7.2 `MXFP8_BACKEND`: rc 0, `--kernel-config`, 0 mounts. j 0.7.10: ships (`sort -V`). k 0.7.2.post1 on `run`: ships. l 0.6.18 on `run`: rc 1. m `<no value>`: mounted. Then the real `docker image inspect --format` under `set -euo pipefail` reads `…g4d9a8d6` 0.7.1, `:latest` 0.6.18.post1 and a missing image empty: all "does not ship" | 1 each | emitted-command and detection check; nothing booted |
| 2026-10-16 | T1: `./container/patches/b12x/fetch` ×2, then a copy with `B12X_WHEEL_SHA256=0…0` and `CACHE_DIR=<scratch>` | `fetched` → `/mnt/noir/scratch/ai/vllm/cache/b12x/1.5.1/site`, stamp `8731e235…`; then `already present`; bad sha: `sha256 mismatch … Nothing was unpacked.` rc 1, `find` shows only `b12x/1.5.1/` | 1 each | behavior check, not a number |
| 2026-10-18 | T3 FP4-Local matrix: `./scripts/flashnext-matrix` (`326768e`) on `run-fp4local`, image `…gfe8f214` (flashinfer 0.7.2's bundled b12x, nothing mounted), `SM120_SKINNY=1`, 1 unrecorded warm-up pass + 4 recorded `./bench/bench` passes per config, single stream, ignore_eos; `bench/bench-logs/flashnext-matrix-20261018-184027/` (gitignored) | decode tok/s mean (min–max), code / long: humming-k3 **195.5 (188.5–210.6) / 190.7 (184.4–197.4)**; humming-k4 181.3 (168.9–192.3) / 181.3 (172.9–184.9); b12x-k3 177.6 (170.2–186.6) / 166.7 (148.7–184.7); b12x-k4 165.0 (162.7–167.3) / 156.8 (154.5–158.7). Long-prompt (~16k ctx) TTFT ms: humming-k3 433 (416–481), humming-k4 418 (413–429), b12x-k3 **385** (378–392), b12x-k4 **379** (374–387). MTP mean acceptance length over the run's log intervals: humming-k3 2.77, humming-k4 3.02, b12x-k3 2.58, b12x-k4 2.97. Logged MoE backend matched the request in all four (`HUMMING` / `B12X`); no error, no segfault; one `Triton kernel JIT compilation during inference` (`_pack_topk_routes_post_prefix_kernel`) under b12x-k3 | 4 per config | Not overlapping: humming > b12x on decode at K=4 (code and long) and at K=3 on code; b12x < humming on long TTFT at both K. K=3 vs K=4: b12x code not overlapping (K=3 wins); humming code/long overlap, **within noise** (means favour K=3 ~5%). humming-k3 vs b12x-k3 long touches by 0.3 tok/s, mean delta 24 tok/s. The `short` prompt (256 tok) is noise: spreads 33–101% (b12x-k3 237→85 tok/s), not used |
| 2026-10-17 | T3 boot (owner): `SM120_SKINNY=1 MOE_BACKEND=b12x MXFP8_BACKEND=b12x ./qwen3.8-flash-next/run-fp4local`, image `…g4d9a8d6`, b12x 1.5.1; log `storage/lastlog` (untracked) | **boots and serves.** `Applied linear backend override for 'mxfp8': 'b12x'` → `Using B12xMxfp8LinearKernel for MXFP8 GEMM`; `Using 'B12X' NvFp4 MoE backend`; `Warmed up 8 b12x MXFP8 kernel signature(s)`, `1 b12x MoE`; csf-decode 73,728 planes, load-filter + sm120-gemm-plans applied, no `NOT applied`. Boot 9m18s (weights 4m52s @ 255 MiB/s, profiling 2m26s, warmup 1m17s); KV 17.03 GiB at util 0.975; MTP K=3 mean acceptance length 2.48–3.79 per log interval. `CuTeDSL JIT compilation during inference` on first requests (MXFP8 at num_tokens=4096, `_DynamicMoE…`, `W4A16FusedMoe…`): warmup does not cover every shape | 1 | **then segfaulted at 2 concurrent requests** (next row). No bench: engine-log 1 s generation averages (135–224 tok/s under a live client) are not a measurement and have no baseline |
| 2026-10-17 | same boot, live traffic; `storage/lastlog:509-513` | ~1 min of single-request decode, then a second request at 06:17:42 (`Running: 2 reqs`, 8 tokens/step at K=3); 06:17:43 generation 0.0 tok/s; then `!!!!!!! Segfault encountered !!!!!!!` / `File "<unknown>", line 0, in syscall` — the apache-tvm-ffi handler's banner (used by CuTe DSL/b12x **and** FlashInfer, so not attributable from this); EngineCore hung until Ctrl-C 06:19:01 → `EngineDeadError` | 1 | **unattributed**: no native frames; FP4-Local has no recorded 2-concurrent baseline without b12x, so a non-b12x cause is not excluded. Bisect: baseline / MoE-only / MXFP8-only, each with two concurrent requests |
| 2026-10-17 | T4: stub `docker` on `run-fp4local` + in-image `EngineArgs` parse of the emitted `--kernel-config` | as in the T4 Progress row | 1 each | emitted-command + parser check; nothing booted |
| 2026-10-17 | T3 boot (owner): `SM120_SKINNY=1 MOE_BACKEND=b12x NVFP4_BACKEND=b12x ./qwen3.8-flash-next/run-fp4local`, image `…g4d9a8d6`, b12x 1.5.1 mounted | weights loaded (72.66 GiB, 387.9 s); b12x prepared MXFP8 `1152→3456`, `1152→1152` and NVFP4-A16 `1152→4304`, `4304→1152` at `num_tokens=65536`; then `EngineCore failed to start` in `profile_encoder_cache` → `qwen3_vl.py:440` `linear_fc2` → `kernels/linear/nvfp4/b12x.py:191` → `b12x/gemm/blockscaled/_tuning.py:191` `ValueError: A16 requires stored K32 and N8` | 1 | deterministic shape check, not noise: K=4304, 4304 % 32 = 16 |
| 2026-10-16 | T2: `run-fp4local` + `run` under gate 7a's stub `docker`, `IMAGE_TAG=…g4d9a8d6`, scratch `CACHE_DIR`, `B12X_SITE=` the T1 tree | cases (a)–(g) as in the Progress row; the echoed `serve` line is the evidence | 1 each | emitted-command check; nothing booted |
| 2026-10-16 | Plan-time probe: wheel unpacked to scratch, mounted `:ro` + `.pth`, `--gpus device=0`, image `…g4d9a8d6` | all 6 `vllm.utils.b12x` submodules import; `B12xMxfp8LinearKernel`, `B12xNvFp4LinearKernel`, `B12xFp8BlockScaledMMKernel`, `B12xTensorFP8ScaledMMLinearKernel` `is_supported() == (True, None)`; `B12xExperts._supports_current_device() == True` | 1 | capability check only — no GEMM was executed |

## Notes / Deviations

- 2026-10-16, plan commit gate: steps 1 (`30 b` / `25 p`, `env ok` — 30 not 29 because `b9aa163` added
  `lfm2.5/run` without bumping RULES.md), 3 (5/5 ok), 4 (`## preflight OK` ×2; dirty-tree guard printed
  nothing), 5 (`last-build == 4d9a8d6…`, `1`; `container/archs` not re-run — no build happened), 7a–7e
  PASS. Step 2 is **red, not skipped**: see plan Risk 5.

- 2026-10-16, T1 gate: step 1 `31 b` / `25 p`, `env ok`, no FAIL; step 2 still red (`muse/run:355` SC2145, `scripts/muse-quality-diff:25` SC2034 — pre-existing, untouched; Risk 5), `fetch` itself `shellcheck -x` clean; 3 5/5 ok; 4 `## preflight OK` ×2, no DIRTY; 5 `last-build == 4d9a8d6…`, `1`, archs not re-run (no build); 7a–7e PASS.

- 2026-10-16, T2 gate: identical to T1's — step 1 `31 b` / `25 p`, `env ok`, no FAIL/DIRTY; step 2 red only on the same two pre-existing findings (`common/_run-lib.sh` itself `shellcheck -x` clean); 3 5/5; 4 OK ×2; 5 `last-build == 4d9a8d6…`, `1`; 7a–7e PASS.

- 2026-10-17, T3 first boot: **`NVFP4_BACKEND=b12x` cannot serve FP4-Local with vision on.** b12x A16 requires `padded_in_features % 32 == 0 and out_features % 8 == 0` (`b12x/gemm/blockscaled/_tuning.py:189-190`); vLLM's `B12xNvFp4LinearKernel.can_implement` returns `True, None` unconditionally (`vllm/model_executor/kernels/linear/nvfp4/b12x.py:71-73` @ `4d9a8d6`), so the selector hands it a layer it cannot run and the failure lands at first use instead of falling back. Shape scan of every MXFP8 / W4A16_NVFP4 layer in the checkpoint (safetensors headers, 2026-10-17): only `model.visual.blocks.N.mlp.linear_fc2` (27×, N=1152, K=4304) violates it; every MXFP8 layer passes K%32 and N%8. MXFP8-via-b12x ran in vision block 0's attention before the MLP failed.

- 2026-10-17, T4 gate: step 1 `31 b` / `25 p`, `env ok`, no FAIL/DIRTY; step 2 red only on the two pre-existing findings (`common/_run-lib.sh` `shellcheck -x` clean); 3 5/5; 4 OK ×2; 5 `last-build == 4d9a8d6…`; 7a–7e 13 PASS. Plan revised (T4 added) in the same commit as T4.

- 2026-10-18, T5 gate: step 1 `31 b` / `25 p`, `env ok`, no FAIL/DIRTY; step 2 red only on the two pre-existing findings (`muse/run:355` SC2145, `scripts/muse-quality-diff:25` SC2034; `common/_run-lib.sh` `shellcheck -x` clean, no finding in any file touched here); 3 5/5; 4 `## preflight OK` ×2; 5 `last-build == 4d9a8d6…` (run with `WANT=4d9a8d6`: versions.env now pins `fe8f214`, which is not built yet), `1`; 7a–7e 13 PASS. Plan revised (T5 added) in the same commit as T5. Origin: the `fe8f214` repin (`2f7fe38`) found flashinfer 0.7.2 bundles b12x, and the owner chose "drop the mount on images that ship b12x".

- 2026-10-18, T3 deviation: the FP4-Local arms ran as **MoE backend × MTP depth** (humming/b12x × K=3/4, `SM120_SKINNY=1`), not as the plan's baseline / MoE / NVFP4-linear / both. `NVFP4_BACKEND=b12x` cannot boot FP4-Local with vision on (T3 boot 1), and the MXFP8-b12x arm was dropped after its segfault (owner's choice). The image was `…gfe8f214`, so b12x here is flashinfer 0.7.2's bundled 1.5.0-based copy, not the 1.5.1 wheel. These numbers do not pool with the `…g4d9a8d6` boots. Why b12x is slower: at K=3 part of it is acceptance (2.58 vs 2.77). At K=4 acceptance is near-equal (2.97 vs 3.02) and b12x is still ~10% slower, so the MoE kernel itself is slower at decode batch sizes on this card. T3 gate: step 1 `32 b` / `25 p`, `env ok`; step 2 red only on the two pre-existing findings; 3 5/5; 4 `## preflight OK` ×2; 5 `last-build == fe8f214…`, `1` (default `WANT`; `container/archs` not re-run in this commit); 7a–7e 13 PASS.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| ~~Bake `b12x==1.5.1` into `container/Dockerfile`~~ | **moot for FP4-Local**: images ≥ `fe8f214` already ship b12x (flashinfer 0.7.2), and T3's FP4-Local arm shows no decode win to bake in. Revisit only if the NVFP4 arm wins | — |
| T3 on a `fe8f214` image runs flashinfer's bundled b12x, not 1.5.1: label the arms by image, and do not pool the numbers with `…g4d9a8d6` + mount | different b12x builds (1.5.0 + flashinfer#6294/#6317/#6211 vs 1.5.1) | T3 Evidence |
| Port `--gdn-decode-kernel b12x` | community-fork only, not upstream at `4d9a8d6` | watch upstream |
| Upstream: `B12xNvFp4LinearKernel.can_implement` should reject A16 shapes with K%32 or N%8 (fall back instead of raising at first use) | the FP4-Local vision `linear_fc2` (K=4304) kills the boot under `--linear-backend b12x` | vLLM issue/PR (owner) |
| Gate step 2: fix `muse/run:355` SC2145 + `scripts/muse-quality-diff:25` SC2034, make the step print FAIL (not `SKIP(shellcheck not installed)`) when shellcheck exits non-zero, and drop SERIES' "shellcheck is not installed" constraint | shellcheck 0.11.0 is installed now; the step misreports a red run as a skip (plan Risk 5) | a small follow-up commit or plan |
