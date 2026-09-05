#!/usr/bin/env python3
"""Restrict vLLM's bundled vllm-flash-attention build to the requested archs.

Runs as the FetchContent PATCH_COMMAND for vllm-flash-attn (see the sed in
container/Dockerfile, vllm-build stage), with the freshly cloned source tree as
the working directory. Applied once per clone, so it must be idempotent-safe:
every replacement is asserted to match exactly once and the script fails loudly
if upstream has moved the lines it edits.

Why (measured on the cu1303 image with `cuobjdump --list-elf`, 2026-09-05):

  * FA2: upstream hard-codes `FA2_ARCHS = "8.0+PTX"`, so with
    TORCH_CUDA_ARCH_LIST=12.0 the 76 FA2 kernels were emitted as sm_80 SASS +
    sm_80 PTX and no sm_120 at all -- they only run on the RTX PRO 6000 via the
    driver's PTX JIT. This makes FA2 intersect the requested archs with
    themselves, i.e. build exactly what TORCH_CUDA_ARCH_LIST asks for (sm_120).
  * FA3: `FA3_ARCHS = "9.0a"` intersects to nothing on sm120, and
    set_gencode_flags_for_srcs() with an empty list passes no -gencode, so nvcc
    fell back to its default target: 192 Hopper-only kernels, 818 MB, as sm_75.
    Unusable on any Blackwell part. FA3 is disabled outright; a no-op
    `_vllm_fa3_C` custom target is left behind because vLLM's setup.py asks
    cmake for `--target=_vllm_fa3_C` and `--install --component _vllm_fa3_C`
    unconditionally when nvcc >= 12.3. flash_attn_interface.py already wraps
    `from . import _vllm_fa3_C` in try/except, so the missing .so is a clean
    "FA3 unavailable" at runtime -- which is what sm120 reports anyway.

Only the Hopper build is removed; nothing sm120 could use is lost.
"""
import pathlib
import sys

path = pathlib.Path("CMakeLists.txt")
src = path.read_text()

EDITS = [
    # FA2: build for the requested archs, not a fixed sm_80 + PTX.
    (
        'cuda_archs_loose_intersection(FA2_ARCHS "8.0+PTX" "${CUDA_ARCHS}")',
        'cuda_archs_loose_intersection(FA2_ARCHS "${CUDA_ARCHS}" "${CUDA_ARCHS}")',
    ),
    # FA3: off. Nothing in the 9.0a-only kernel set can run on sm120.
    (
        "set(FA3_ENABLED ON)",
        "set(FA3_ENABLED OFF)",
    ),
]

for old, new in EDITS:
    n = src.count(old)
    if n != 1:
        sys.exit(
            f"vllm-flash-attn-arch.py: expected exactly 1 match, found {n}:\n  {old}\n"
            "upstream CMakeLists.txt changed -- re-check the patch against the "
            "commit vLLM pins in cmake/external_projects/vllm_flash_attn.cmake"
        )
    src = src.replace(old, new)

STUB = """
# --- added by container/patches/vllm-flash-attn-arch.py -----------------------
# vLLM's setup.py builds/installs `_vllm_fa3_C` by name whenever nvcc >= 12.3.
# With FA3 disabled above, give it an empty target so those steps are no-ops.
if(NOT TARGET _vllm_fa3_C)
  add_custom_target(_vllm_fa3_C)
  message(STATUS "_vllm_fa3_C: disabled (no sm90 target); stub target only")
endif()
"""
if "add_custom_target(_vllm_fa3_C)" in src:
    sys.exit("vllm-flash-attn-arch.py: stub already present -- patch applied twice?")
src = src.rstrip("\n") + "\n" + STUB

path.write_text(src)
print("vllm-flash-attn-arch.py: FA2 -> ${CUDA_ARCHS}, FA3 disabled (stub target added)")
