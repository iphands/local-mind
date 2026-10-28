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
