# Pitfalls & Gotchas — ninfer

Every bug, gotcha and trap — **especially anything that took more than one attempt to fix.** Read
before new work. Tag entries that actually bit `[OBSERVED YYYY-MM-DD]`; untagged entries are
hazards seeded from reading, not incidents.

Cross-cutting entries also go up to the monorepo's `../../../context/` (empty today — create
`pitfalls.md` there on first use); project-specific ones stay here.

<!-- Entry template (~200 words per half; quote the symptom verbatim):

# <Pitfall title>

## Problem
[OBSERVED YYYY-MM-DD] What happened, the exact error/output, why it was confusing, what was
tried first and why that failed.

## How to avoid
The fix, the rule that prevents it, and how to detect it early next time.

## Sources
- <project/sub-project>: `<short_filename>` (`<function_name>`)
-->

---

# Upstream's Dockerfile cannot build the pinned NInfer commit (no liburing)

## Problem
Seeded from reading, 2026-10-27 — not yet observed. `cmake/Dependencies.cmake:5` @ `64d10fbf`
requires `liburing>=2.5` (`pkg_check_modules(URING REQUIRED …)`, added in `8cea711b`,
2026-10-13), but upstream's `Dockerfile` was last changed in `74671a4e` (2026-10-12) and installs
neither `liburing-dev` (build stage) nor `liburing2` (runtime stage). Configure should fail at
`pkg_check_modules`. It also uses CUDA 13.1.2 where 13.4 is maintained, runs as root, and has no
labels.

## How to avoid
Build with this project's `Dockerfile`, never upstream's: it installs `liburing-dev`/`liburing2`,
pins CUDA 13.4.3 by digest, ends `USER 1000:1000`, and labels `ai.ninfer.ref`. On a NInfer pin bump,
diff upstream's `cmake/Dependencies.cmake` against our package lists — a new `REQUIRED` there is a
new package here.

## Sources
- ninfer: `Dockerfile`; upstream `cmake/Dependencies.cmake:4-23`, `Dockerfile`

# Our NVFP4 checkpoint is not a conversion base — don't "fix" the converter to take it

## Problem
Seeded from reading, 2026-10-27. Pointing `tools.convert --model` at
`models/vllm/nvidia/Qwen3.8-Flash-Next-NVFP4` looks like it saves a 360 GB download, but the
`qwen4_exp` builder rejects every non-BF16 base tensor, expects fused `mlp.experts.gate_up_proj`
banks (the NVFP4 checkpoint has per-expert `U8` weights), and its PLE table is `F8_E4M3`, not
BF16. A shim that monkeypatches the builder would be tied to one commit and would NF4-quantize PLE
from FP8 instead of BF16 (rejected by the owner, 2026-10-27).

## How to avoid
The NVFP4 checkpoint is the `--source nvidia=` input; the base is `Qwen/Qwen3.8-Flash-Next` at
`HF_BASE_REV` (BF16). `scripts/convert` checks the base's expert-bank and PLE headers for BF16
before starting a container.

## Sources
- ninfer: `scripts/convert` (header preflight); upstream `tools/convert/qwen4_exp.py` (`Builder.add`, `bank_source`)

# Known upstream serving bugs at the pin: grammar 503, prefix-reuse decay

## Problem
Seeded from upstream issues, 2026-10-27. #399: one grammar violation in ~20k constrained requests
takes the engine to HTTP 503 (`/health` → `{"status":"unavailable"}`) until restart. #394 (Flash-Next
on an RTX PRO 6000): prefix reuse degrades once the shared pinned host context budget
(`--host-context-mib`, default 8 GiB + 8 StateImages) is full.

## How to avoid
A 503 from `/health` means restart the container, not retry. For long agent sessions with heavy
prefix reuse, a larger `--host-context-mib` is the lever (a Plan 01 Follow-up, unmeasured). Re-check
both issues on any pin bump.

## Sources
- upstream: issues #399, #394; `docs/serving.md` § Endpoints, § Server options

# moby's default seccomp profile is no longer at `profiles/seccomp/default.json`

## Problem
[OBSERVED 2026-10-27] Fetching
`https://raw.githubusercontent.com/moby/moby/docker-v29.8.0/profiles/seccomp/default.json` (the
path every older guide uses, and the one Plan 01 T2 was written against) returns **404**;
`daemon/profiles/seccomp/default.json` too. Since docker v29 the profile lives in the separate
`github.com/moby/profiles` module and is only *vendored* into moby, at
`vendor/github.com/moby/profiles/seccomp/default.json`. Found with the GitHub trees API
(`/repos/moby/moby/git/trees/<commit>?recursive=1`, filter on `seccomp`).

## How to avoid
`scripts/seccomp-profile` fetches the vendored path and fails loudly (`curl -f`) on a 404. On a
docker upgrade, find the tag whose `^{}` commit equals `docker version --format
'{{.Server.GitCommit}}'` (`git ls-remote --tags https://github.com/moby/moby | grep <ver>`), and
if the download 404s again, search that commit's tree before guessing a path.

## Sources
- ninfer: `scripts/seccomp-profile` (`SRC_PATH`)

# `docker build --check` exits 1 on empty-default `FROM` ARGs

## Problem
[OBSERVED 2026-10-27] `PREFLIGHT_ONLY=1 ./build` stopped with rc 1 after `docker build --check`
reported `InvalidDefaultArgInFrom` twice ("Default value for ARG ${CUDA_DEVEL_IMAGE} results in
empty or invalid base image name"). `--check` treats warnings as failure, and the empty defaults
were deliberate — every pin comes from `versions.env` through `--build-arg`.

## How to avoid
Keep the defaults empty (a bare `docker build` must fail, not build something unpinned) and say so
with `# check=skip=InvalidDefaultArgInFrom` directly under `# syntax=…` (parser directives must
be the first lines). Do not "fix" it with a default image: that is a second copy of a pin. Also: a
global `ARG` reaches only `FROM` lines — redeclare it (no default) inside a stage that reads it.

## Sources
- ninfer: `Dockerfile` (header), `build` (`PREFLIGHT_ONLY`)

# A ModelOpt `hf_quant_config.json` can say `MIXED_PRECISION` for an NVFP4 checkpoint

## Problem
[OBSERVED 2026-10-27] Plan 01 T4 was written to check that the NVIDIA checkpoint's
`hf_quant_config.json` "says NVFP4". It does not: `quantization.quant_algo` is
`MIXED_PRECISION` (modelopt 0.46), because MTP experts are `FP8_BLOCK_SCALES` and one module is
`FP8`. The NVFP4 fact lives one level down, per module, in `quantization.quantized_layers`
(`model.language_model.layers.N.mlp.experts` → `{"quant_algo": "NVFP4", "group_size": 16}`).
A top-level check would have refused the right checkpoint.

## How to avoid
Check per-module entries (`scripts/convert` requires all 48 main-expert layers at `NVFP4`) or,
better, the tensors themselves. NInfer's converter ignores this file and reads tensors.

## Sources
- ninfer: `scripts/convert` (`check_headers`, nvidia branch)
