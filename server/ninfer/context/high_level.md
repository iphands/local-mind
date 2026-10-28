# High-Level — why these dependencies, and what we rejected

Short rationale for dependency and architecture choices. Keep entries brief: deep technical
detail belongs in `distilled.md`, failure modes in `pitfalls.md`.

<!-- Entry shape:

## <Area — e.g. HTTP framework>

The deciding constraint, in one or two sentences.

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **chosen-lib** | … | … | **Chosen** (used in: <component>) |
| alternative | … | … | Rejected — <one-line reason> |

**Revisit if:** <the condition under which this choice should be re-opened>.
-->

---

## Image — our own Dockerfile, not upstream's

Upstream's `Dockerfile` cannot build the pin (no liburing; see `pitfalls.md`), uses CUDA 13.1.2,
runs as root and carries no labels.

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **own two-stage `Dockerfile`** | NInfer by SHA, CUDA 13.4.3 by tag + digest, liburing, `USER 1000:1000`, `ai.ninfer.ref` label to find the image by | we track upstream's dependency list by hand | **Chosen** (used in: `Dockerfile`, `build`) |
| upstream `Dockerfile` | zero maintenance | does not build the pin; root; unlabelled | Rejected |

**Revisit if:** upstream's Dockerfile catches up (liburing, CUDA 13.4, non-root) — then a thin
wrapper over it may beat a fork.

## Conversion input — download the BF16 base

The `qwen3_8_flash_next` recipe needs a BF16 base plus the NVIDIA NVFP4 checkpoint as `nvidia`;
we only had the latter.

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **download `Qwen/Qwen3.8-Flash-Next` (BF16, 360 GB)** | the documented path; PLE quantized from BF16 | 360 GB of disk and download time | **Chosen** (owner, 2026-10-27) |
| shim the NVFP4 checkpoint into a base | no download | monkeypatches the builder against one commit; PLE quantized from FP8 | Rejected |

**Revisit if:** NInfer publishes a Flash-Next artifact we could pull directly, or the recipe learns
to take a quantized base.

## Converter runtime — reuse `../vllmcustom`'s image

The converter needs Python + torch + NumPy and, for its JIT PLE encoder, nvcc + g++ + ninja.

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **`../vllmcustom/container/.last-build` image** | already on disk; Python 3.12.3, torch 2.14.0 (CUDA 13.2), NumPy 2.2.6, nvcc 13.2, ninja, g++ | NInfer maintains Python 3.11; we depend on a sibling's image | **Chosen** (used in: `scripts/convert`, gate step 3 probe) |
| a dedicated converter image | Python 3.11 exactly as maintained | several GB of torch wheels on an untested CUDA 13.4 combination | Rejected (kept as a Follow-up) |

**Revisit if:** the converter fails under 3.12 / torch 2.14 (Plan 01 T4/T7), or vllmcustom's image
loses nvcc/ninja.

## Syscall policy — a seccomp profile, not `unconfined`

NInfer's PLE reader needs io_uring, which docker's default profile blocks.

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **docker default profile + `io_uring_setup`/`_enter`/`_register`** | the smallest widening that works; regenerable from a pinned moby ref | frozen at that ref — drifts on a docker upgrade | **Chosen** (used in: `seccomp/ninfer.json`, `run`) |
| `--security-opt seccomp=unconfined` | no maintenance | drops all syscall filtering on a server bound to `0.0.0.0` | Rejected |

**Revisit if:** docker's default profile starts allowing io_uring, or NInfer gains a non-io_uring
reader.
