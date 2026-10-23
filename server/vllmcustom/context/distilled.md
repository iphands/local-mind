# Distilled — confirmed facts for vllmcustom

Compact, confirmed learnings: formats, protocols, tool/library behavior, measured limits. Read
before new work; add an entry whenever something is confirmed — **above the `## Open Questions`
section**, which always stays last. Keep it dense.

**Provenance discipline:** every entry carries a tag — `[SOURCE]` (upstream code, cite
`path:line`), `[DOC]` (documentation), `[REPO]` (this repo's own code/config), `[LIVE YYYY-MM-DD]`
(observed on a running system). **Do not upgrade a tag without doing the work.**

<!-- Entry shape:

## <Topic — the rule in one line> [TAG][TAG]

- The fact, stated so it can be checked.
- The evidence: command + verbatim output, or `path:line`.
- What it means for this project.
-->

## The host-runnable test surface is exactly five files, and two patches have none [LIVE 2026-10-13, amended 2026-10-15 (Plan 04)]

- `container/patches/{boot-timing,csf-scale-decode,draft-load-filter,sm120-gemm-plans,spec-live}/test_*.py` run on
  the host with plain `python3 <file>` (stdlib only, no vLLM, no pytest — pytest is not installed).
  Each prints `ALL TESTS PASS` and exits 0.
- `muse/patches/atem-parsers/selftest.py` looks host-runnable from its docstring but is not: it
  imports `transformers` and `vllm.parser.parser_manager`, so it runs in the image via
  `./muse/run --selftest`.
- `muse/patches/muse-embed-norm/` (five shims, one of which prevents the
  `assert isinstance(model_output, tuple)` crash) and `muse/patches/muse-dflash/` have **no test at
  all**. Anything touching them is verified at runtime or not at all — say which.

## No vLLM patch replaces an upstream file, so nothing carries a content hash [REPO, amended 2026-10-15]

- All four patches under `container/patches/` (`boot-timing`, `draft-load-filter`,
  `sm120-gemm-plans`, `spec-live`) are post-import hooks: a `sitecustomize`/`.pth` mount that wraps
  upstream objects looked up by name at import time. Their `UPSTREAM` files state that no
  `BASE_SHA256` is present **deliberately** — there is no upstream file to hash, and a renamed target
  just no-ops, with the boot report printing `hooks NOT applied`.
- The exception, so do not over-read the heading above: `container/patches/vllm-flash-attn-arch.py`
  is not a hook. It is a `FetchContent` `PATCH_COMMAND` that edits the fetched vllm-flash-attention
  `CMakeLists.txt`, and it asserts each edit lands exactly once. It guards itself by anchor rather
  than by sha, so it still carries no `BASE_SHA256` — but it does modify upstream content, and it is
  the row `./container/archs` exists to prove.
- There **used to be** one whole-file overlay: `qwen3.8-flash-next/patches/ple-ct-ignore/`, which
  copied `ngram_embedding.py` and carried a `BASE_SHA256` that the **launcher** compared against the
  copy inside the image, never the host tree (a host-side check would compare the wrong artifact, so
  gate step 5 does not do one). vllm#59431 landed the same branch upstream on 2026-09-30, the overlay
  was inert from `47de9d4` on, and it was **deleted at the `4d9a8d6` repin**. Its base file had moved
  +221/−17 in that last range, so it was unrecoverable as an overlay regardless.
- Consequence: "add a check that the patches match upstream" is a no-op here — there is no base file
  to match. The checks that matter are `./container/archs` for the compiled extensions, the host-side
  `container/patches/*/test_*.py` (gate step 3), and the hooks' own runtime `NOT applied` lines.

## This project reaches the docker socket directly — it uses no privilege helper [LIVE 2026-10-13, amended 2026-10-13 (Plan 02)]

- Socket ownership after the owner's host fix, measured 2026-10-13: `iphands:iphands 660` — owned by
  the invoking uid, **not group-owned**, so there is no group to join and no re-login step. Plain
  `docker version` returns `29.7.2` at rc 0 with no privilege of any kind.
- **`sudo` is absent from the normal operating environment** and must not be expected: the documented
  `VLLM_REF=main ./container/build` died on `timeout: failed to run command 'sudo': No such file or
  directory` (exit 127). Two provenance warnings: the *agent's* container ships `/usr/sbin/sudo` and
  `/usr/sbin/docker`, so never infer host facts from that shell; and Plan 01's tracker carries a dated
  row measuring `sudo -n -l` → `(ALL) NOPASSWD: ALL`. Both were true of their own environment on the
  same date. The durable statement is "this project uses no privilege helper", not "this box has no
  sudo".
- `container/build`, `container/archs` and every launcher define a `DOCKER` array defaulting to plain
  `docker`, **refuse a value naming `sudo`**, and probe `docker version` before the first call that
  could misreport why it did not. `container/build` also refuses uid 0, with no escape hatch.
- **History, in order, because each version was once correct.** (1) Every script called bare `docker`
  and an agent shell had to prefix it. (2) Plan 01 routed all of it through `${DOCKER[@]}`, default
  `sudo -n docker`, and the consequence *inverted*: prefixing a script **disables** the override,
  because `env_reset` drops `DOCKER`, so the script silently takes its own default — a gate step that
  runs the wrong command and still exits 0. `container/build` and the launchers began refusing uid 0.
  (3) **Plan 02 (2026-10-13) removed the sudo machinery entirely**: default is plain `docker`,
  `ALLOW_ROOT` is deleted (and announces itself if set), the gate's raw docker lines lost their
  prefix, and 7a's freed assertion slot now asserts the new guard. What survived from (2) is the shape
  — `${DOCKER[@]}` everywhere, probe before first use, never run privileged — **and the root refusal**,
  kept deliberately because `_writable` tests `-w`, which is always true for root, so nothing else in
  the tree can catch a privileged run before it force-checks-out the four hand-editable trees.
- Bare and not `DOCKER`-aware, unchanged: `container/push`, `bench/_bench-lib.sh`, `bench/bench-wrapper`,
  `bench/bench-context`, the ten `scripts/*` that shell out. Since Plan 02 that is no longer a
  difference worth tracking — plain `docker` is now what everything uses, so they work or fail exactly
  when everything else does. The two root-side scripts remain `container/gc-setup` and
  `container/slice-setup`, by design, both via `su -c`.

## Boot stats JSONL log (Plan 03) [REPO][LIVE 2026-10-16]

- `container/patches/boot-timing/boot_timing.py` gains `write_jsonl()` + `_report_struct()`; called from `_report_now` when `VLLM_BOOT_LOG_DIR` is set. Appends one compact JSONL line per boot to `<log_dir>/vllm_boot_stats.jsonl` with top-level `ts`, `model`, `vllm_args` (key→value dict; bare flags → `true`), and `boot` (total_seconds, processes, gpu_memory, host_ram, headroom, notes, raw_marks_path). No-op when env unset; never raises.
- `common/_run-lib.sh` `docker_core_args`: when `VLLM_BOOT_LOG=1`, creates `./storage` (owned by RUN_UID), mounts to `/opt/vllm-storage:rw`, exports `VLLM_BOOT_LOG_DIR=/opt/vllm-storage`.
- `qwen3.8-flash-next/run`: sets `VLLM_BOOT_LOG=1`, `VLLM_MODEL_NAME="$MODEL"`, `VLLM_BOOT_ARGS` as JSON-encoded dict converted from the bash `VLLM_ARGS` array.
- Opt-in per launcher: other launchers stay silent by not setting `VLLM_BOOT_LOG`.

## NVFP4-CSF checkpoints (Plan 04) [SOURCE][LIVE 2026-10-15]

- Format: routed-expert E4M3 block scales stored as `<X>.weight_scale.nvfp4_csf_fixed` (U8 `[rows/16, 16*(1+cols/2)]`: per 16-row slab, 16 row-base bytes then packed nibbles, low nibble = even column) + `…_exceptions` (U32: `row*cols+col | value<<24`, sorted). Scale = `base[row] + nibble`, exception words overwrite. cols = K/16. Codec string `byte-window4-fixed-stream-u24-exceptions/1`. [SOURCE: b12x `tests/quantization/test_nvfp4_csf.py::_fixture` codec 0, `local-inference-lab/b12x` master; vLLM fork `…/utils/nvfp4_csf_utils.py:_slice_scale_plane`, 2026-10-15]
- Only main-decoder routed experts are compressed; MTP, vision, PLE, shared experts, global scales stay plain. [DOC: HF card of `local-inference-lab/Qwen3.8-Flash-Next-NVFP4-MXFP8-CSF-QAD`]
- The community readers are bound to b12x kernels (and PP1/DP1, no EP); there is no upstream-vLLM reader. `container/patches/csf-scale-decode/` decodes in the weight stream instead. Real shard 1: 913 planes decoded in 0.3–0.4 s, torch backend == pure-Python reference on 6/6 planes. [LIVE 2026-10-15]
- A decode of real data looks sane (bases 89–104, no NaN codes, 0.2–1 % exceptions, scales finite 14–448) but ground truth is only the b12x fixture; boot coherence is the test (Plan 04 T4).

## b12x is a mount, not a build: pure-Python wheel, every dependency already in the image (Plan 06) [SOURCE][LIVE 2026-10-16]

- vLLM's `b12x` extra pins `b12x==1.5.1` (`setup.py:1709` @ `4d9a8d6`); our Dockerfile installs no extras, so the `g4d9a8d6` image has no b12x and every b12x kernel reports unsupported. The image also has **no `pip`** (`/opt/venv/bin/python: No module named pip`). [LIVE 2026-10-16]
- The 1.5.1 wheel is `py3-none-any` (no `.so`; CuTe-DSL JIT at runtime into `$XDG_CACHE_HOME`/`~/.cache`). Its `requires_dist` is met in the image: torch 2.14.0, cuda-python 13.4.4, nvidia-cutlass-dsl + 4× `-libs-*` 4.8.0, apache-tvm-ffi 0.1.11, rich 15.0.0, transformers 5.19.0, safetensors 0.8.0. [DOC: PyPI JSON][LIVE 2026-10-16]
- Mounted `:ro` + a `.pth`, on `--gpus device=0`: all six `vllm/utils/b12x.py` submodules import, `B12xMxfp8LinearKernel` / `B12xNvFp4LinearKernel` / `B12xFp8BlockScaledMMKernel` / `B12xTensorFP8ScaledMMLinearKernel` return `(True, None)`, `B12xExperts._supports_current_device()` is `True`. Capability only — no GEMM executed. [LIVE 2026-10-16]
- Auto-selection: `B12X` and `FLASHINFER_B12X` are both absent from the NVFP4 MoE `AVAILABLE_BACKENDS` (`fused_moe/oracle/nvfp4.py:199-213`), and b12x linears sit behind FlashInfer-CUTLASS + Marlin (MXFP8) and nine others (NVFP4) in `kernels/linear/__init__.py:556-596`. Installing it does not move an `auto` pick. [SOURCE]
- b12x FP8 scaled-mm takes only static per-tensor or 128×128 block scales (`kernels/linear/scaled_mm/b12x.py:92,227-231`): `ONLINE_FP8=1`'s dynamic per-channel layers fall back to auto under `--linear-backend b12x`. [SOURCE]
- `--gdn-decode-kernel b12x` (the FP4-Local card's recipe) is community-fork only: nothing matches `gdn.*b12x` in upstream `vllm/` at `4d9a8d6`. [SOURCE]
- **From the `fe8f214` snapshot on, the image ships b12x itself.** flashinfer-python 0.7.2 (vllm#61913) embeds b12x 1.5.0 under `flashinfer.experimental.b12x` and installs a top-level `b12x` compat package (flashinfer#5767). Our `.pth` only appends to `sys.path`, so on that image `import b12x` gets flashinfer's copy and the mounted 1.5.1 goes inert. vLLM's extra still pins 1.5.1 (`setup.py:1732`). The six submodules vLLM imports all exist in the embedded tree. [SOURCE: flashinfer v0.7.2 pyproject.toml + tree; not yet seen in a built image] Since Plan 06 T5 (2026-10-18), `b12x_args` mounts nothing on an image whose `ai.vllmcustom.flashinfer.version` label is ≥ 0.7.2. The label reads `0.7.1` on `…g4d9a8d6` and `0.6.18.post1` on `:latest`, so both keep the mount. [REPO][LIVE 2026-10-18: `docker image inspect --format` on both]
- **On FP4-Local, b12x MoE is slower at decode than humming and faster at long-prompt prefill.** On `…gfe8f214` with `SM120_SKINNY=1`, single stream, 4 runs per config, decode tok/s mean (min–max): code humming 195.5 (188.5–210.6) vs b12x 177.6 (170.2–186.6) at K=3, and 181.3 (168.9–192.3) vs 165.0 (162.7–167.3) at K=4. Long (~16k ctx) humming 190.7 (184.4–197.4) vs b12x 166.7 (148.7–184.7) at K=3, and 181.3 vs 156.8 at K=4. Long-prompt TTFT is humming ~425 ms vs b12x ~382 ms, ranges not overlapping. MTP K=4 accepts more (≈3.0 vs ≈2.7 mean length) but is not faster than K=3 on either backend. Keep `MOE_BACKEND=humming SPEC_TOKENS=3`. [LIVE 2026-10-18: `scripts/flashnext-matrix`, Plan 06 tracker Evidence]
- **On Flash-Next-NVFP4 too, b12x MoE loses to humming at decode, and it cannot boot with MTP unless the drafter gets its own backend.** The drafter's experts are FP8, b12x has no FP8 MoE, and `--moe-backend` is global. `--speculative-config '{…,"moe_backend":"humming"}'` (`SpeculativeConfig.moe_backend`, `vllm/config/speculative.py:417`) fixes the boot. With `ONLINE_FP8=1 SM120_SKINNY=1`, 4 runs, decode code / long mean (min–max): humming-k3 184.1 (177.2–189.7) / 175.9 (169.9–179.1); b12x-k3 167.2 (162.9–171.0) / 167.5 (157.2–172.9); humming-k4 171.1 / 170.8; b12x-k4 158.4 / 148.5. Acceptance is equal across backends (2.51, 2.76), so the gap is the kernel. K=3 beats K=4 on code (no overlap). `ONLINE_FP8=1` is worth ~+10% decode on this checkpoint (humming-k3 code 166.9 → 184.1, separate session). [LIVE 2026-10-18: `scripts/flashnext-matrix`, Plan 06 tracker Evidence]

## SM120_SKINNY (sm120-gemm-plans overlay) is a measured no-op on FP4-Local [LIVE 2026-10-18]

- FP4-Local, `run-fp4local` defaults + `SPEC_TOKENS=3`, image `…gfe8f214`, 4 single-stream runs each
  (`scripts/flashnext-skinny-ab`). Decode code: on 198.4 (186.5–204.4) vs off 199.0 (192.0–204.4) tok/s.
  Long: 168.1 (134.5–185.3) vs 176.0 (161.4–182.6). Both within noise, and acceptance is equal (2.66 / 2.69).
  The overlay did apply (its boot line is present in the on arm only). [LIVE 2026-10-18]
- Cost: about 40 s more boot (CuTe-DSL warmup 44 vs 6 compile units, warmup 1m01s vs 39s, profiling 1m16s vs 58s). [LIVE 2026-10-18]
- Why it does nothing here: on FP4-Local the path only sees the BF16 layers (lm_head 248320×2560, HC, router);
  everything else is MXFP8/NVFP4 and never takes it (`sm120-gemm-plans/UPSTREAM` "Interplay"). Untested and the only
  case it could matter: NVFP4 without `ONLINE_FP8`. Both Flash-Next launchers default `SM120_SKINNY=0` (`run` since this A/B). [REPO]

## MTP depth on FP4-Local: K=3 is the best depth from 1 to 4 concurrent streams [LIVE 2026-10-18]

- **Single stream** (`scripts/flashnext-spec-sweep`, humming, `SM120_SKINNY=1`, image `…gfe8f214`, 4 runs per K,
  `bench/bench-logs/flashnext-spec-20261018-205442/`). Decode code tok/s mean (min–max): K=0 117.6 (117.5–117.9),
  K=1 163.8 (161.7–165.9) 1.39x, K=2 190.0 (188.6–191.8) 1.62x, K=3 192.8 (184.6–201.1) 1.64x. Long (~16k ctx):
  K=0 114.5, K=1 1.40x, K=2 185.1 (172.3–191.3) 1.62x, K=3 178.7 (153.7–192.8) 1.56x. So on one stream K=2 and K=3
  are within noise. Accept len (1 + accepted/drafts), code: 1.77 / 2.35 / 2.75. Per-position acceptance at K=3 is
  76 / 56 / 43 %. Step cost (accept len ÷ speedup, i.e. how much longer one draft+verify step is than a K=0 step):
  1.27 / 1.45 / 1.68. [LIVE 2026-10-18]
- **Concurrent** (`scripts/flashnext-conc-sweep`, `run-fp4local` defaults + `SPEC_TOKENS=K`, `SM120_SKINNY=0`, code
  prompt, unique prefix per stream so the prefix-cache hit rate was 0%, 3 rounds per cell,
  `bench/bench-logs/flashnext-conc-20261019-042515/`). Per-stream tok/s mean (min–max):
  - 1 stream: K=0 122.4 (122.3–122.4), K=1 170.5, K=2 185.6 (181.9–189.4), K=3 **198.0** (192.0–207.9), 1.62x.
  - 2 streams: K=0 107.1, K=2 163.2 (162.4–164.1), K=3 **169.2** (163.7–172.3), 1.58x.
  - 4 streams: K=0 91.4 (91.2–91.7), K=1 121.2, K=2 133.2 (131.6–135.6), K=3 **137.9** (136.2–139.3), 1.51x.

  Aggregate (all tokens ÷ wall time from the first first-token) at 4 streams: K=0 365.4, K=2 522.8 (517.9–529.2),
  K=3 532.7 (527.8–539.6), 1.46x. At 2 streams K=3 gives 335.1 vs K=0 214.1. [LIVE 2026-10-18]
- What it means:
  - Acceptance does not move with load (K=3 accept len 2.76 / 2.76 / 2.74 at 1 / 2 / 4 streams). The MTP gain that
    is lost as streams are added (1.62x → 1.51x per stream) is verify cost, not prediction: step cost goes from
    1.70 to 1.81.
  - K=3 leads K=2 by +6.7% / +3.7% / +3.5% per stream at 1 / 2 / 4 streams. At 4 streams the aggregate ranges
    overlap, so the two are tied there.
  - K=0 is never better. Four streams at K=3 (137.9 each) beat one stream at K=0 (122.4).
  - Keep `SPEC_TOKENS=3` (`run-fp4local`'s default). [LIVE 2026-10-18]
- The two runs' single-stream numbers agree (K=3 code 1.64x vs 1.62x; K=0 117.6 vs 122.4, different boots).
  [LIVE 2026-10-18]

## FP4-Local draft knobs: probabilistic drafting and a BF16 drafter KV both pay [LIVE 2026-10-18]

- `scripts/flashnext-draft-ab`, `run-fp4local` defaults + `SPEC_TOKENS=3`, image `…gfe8f214`, 4 rounds per
  cell, unique-prefix prompts, temperature 0.6, `bench/bench-logs/flashnext-draft-ab-20261019-051831/`.
  Code, per-stream tok/s mean (min–max) at 1 stream, then aggregate at 4 streams:
  - `prob`: **197.4** (192.9–205.1), then **534.9** (526.3–553.1); accept len 2.76 / 2.73.
  - `greedy` (`DRAFT_SAMPLE=greedy`): 168.6 (162.3–172.3), −15%; then 489.8 (478.5–504.3), −8%;
    accept len 2.38 / 2.54.
  - `mtp-kv-fp8` (`KV_SKIP_MTP=0`): 182.9 (176.8–188.8), −7%; then 507.6 (500.2–521.6), −5%; accept len 2.56 / 2.60.

  None of these ranges overlap `prob`'s. Long prompt (~16k ctx): all three within noise. [LIVE 2026-10-18]
- Why greedy loses: with greedy drafting the verify step treats the draft as one-hot, so at temperature
  > 0 it accepts less. At temperature 0 the two would match. Keep `DRAFT_SAMPLE=probabilistic`.
  `qwen3.8-flash-next/run` (NVFP4) still takes vLLM's greedy default; that is untested there. [SOURCE:
  `vllm/config/speculative.py:604`][LIVE 2026-10-18]
- `KV_SKIP_MTP=1` costs 7% of the KV pool (962,043 vs 1,030,760 tokens at util 0.975) and buys the
  5–8% above. Keep it. [LIVE 2026-10-18]
- Side finding: `bench/bench`'s long-prompt TTFT (~415 ms) is a **warm prefix-cache** number. It re-sends
  the identical prompt every run, so later runs hit the cache. A cold ~16k prefill is ~875 ms
  (`bench/conc`, unique prefix, 1 stream). Four long prompts arriving together get their first token
  at ~1.06 / 2.0 / 2.9 / 3.7 s: the prefills queue behind each other in 6019-token chunks. [LIVE 2026-10-18]

## `FLASHINFER_AUTOTUNE=1` is a no-op on main images: FlashInfer autotune is already on [SOURCE][LIVE 2026-10-18]

- `KernelConfig.enable_flashinfer_autotune` defaults to `None` and is filled from the optimization level.
  `-O0` sets `False` and `-O1` and up set `True` (`vllm/config/vllm.py:266,287` @ `fe8f214`). The default level is
  `-O2` (`vllm/config/vllm.py:428`), and no launcher here passes `-O`. So the knob's `--enable-flashinfer-autotune`
  changes nothing. [SOURCE]
- Every FP4-Local boot already logs `[Autotuner]: Autotuning process starts` in kernel warmup, followed by
  `Preloaded 105/105 managed cache entries`. [LIVE 2026-10-18: `flashnext-conc-20261019-042515/k3.log`]
- `qwen/run`'s knob is untouched: its default image may predate this default. [REPO]

---

## Open Questions

- **Why does FP4-Local's TTFT on the ~1k-token code prompt drop from ~145 ms to ~40 ms on some boots?** On the
  conc sweep it is low only at K=3, in all of that boot's rounds: 40 vs ~145 ms at 1 stream, 77 vs ~290 at 2,
  190 vs ~290 at 4. On the spec sweep it was low at K=2 and K=3 (39 ms) and not at K=0 or K=1 (153–159 ms); the
  long prompt stayed ~415 ms at every K≥1 there. The prefix-cache hit rate was 0% throughout the conc sweep, and
  its boot args differ only in `num_speculative_tokens`. So it is not caching and not a flag. Decode rates are
  unaffected, since both benches exclude TTFT. (2026-10-18; `flashnext-conc-20261019-042515/results.jsonl`,
  `flashnext-spec-20261018-205442/summary.md`)

- `./container/archs` has not been run against the current `df6c7ad` snapshot image (nor against
  `187a0eb`, its predecessor) — the arch
  table is asserted by the build's own patch script, not re-measured. Confirm before trusting it.
- Whether the FA2 `_vllm_fa2_C` extension is still sm_120-native on the main snapshot (the README
  verified the *patch anchors* on 2026-09-22, not the emitted SASS).
