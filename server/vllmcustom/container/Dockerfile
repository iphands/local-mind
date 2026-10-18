# syntax=docker/dockerfile:1.7-labs
#
# Source-built, sm120-optimized vLLM for the RTX PRO 6000 Blackwell.
#
# Everything on the LLM-critical path (PyTorch, FlashInfer, vLLM) is compiled
# from source for sm_120 (TORCH_CUDA_ARCH_LIST=12.0). torchvision is pulled as a
# prebuilt wheel from the CUDA-minor-matched index (cu132); torchaudio is built
# from source because no cu132 wheel exists -- and a cu130 one is NOT usable:
# both torchvision and torchaudio compare their compiled CUDA major.minor with
# torch.version.cuda at import and raise on mismatch (learned the hard way:
# "PyTorch has CUDA version 13.2 whereas TorchAudio has CUDA version 13.0").
# transformers imports torchaudio at module load, so that error kills `vllm`.
#
# The pins themselves live in container/versions.env; ./build passes them all as
# --build-arg, so the ARG defaults in this file are fallbacks for a bare
# `docker build` only. The notes below are the history of each bump.
#
# Version notes (checked 2026-09-11; see README "Default version set"):
#   - vLLM v0.29.0 (tagged 2026-09-08) pins torch 2.13.0 / torchaudio 2.11.0 /
#     torchvision 0.28.0 / flashinfer 0.6.18 -- only flashinfer moved from
#     v0.28.0. That is still true of the RELEASE set. It stopped being true of
#     main on 2026-10-14: vllm#52183 moved main to torch 2.14.0 / torchvision
#     0.29.0, so the snapshot now carries its own torch pins (VLLM_MAIN_TORCH_REF
#     / VLLM_MAIN_TORCHVISION_VERSION) rather than this set's. No tagged vLLM
#     release pins 2.14.0 yet, so the release build is unaffected.
#   - CUDA 13.2.1: the constraint that actually binds the minor is the cu132
#     torchvision index, which must agree with the CUDA our source-built torch
#     reports (both torchvision and torchaudio raise at import on a major.minor
#     mismatch). It happens to also be the newest toolkit in torch 2.13's binary
#     matrix (12.6/12.9/13.0/13.2), but do not infer it from that matrix -- see
#     the note in versions.env on what torch 2.14's PyPI metadata pulls. 13.3.x
#     exists and the host driver supports it, but no torch/flashinfer/vLLM
#     release has CI on it.
#   - FlashInfer 0.6.18 (both the v0.29.0 pin and main's 0.6.18.post1) added a
#     build-time dependency policy (nvidia-cutlass-dsl) that the jit-cache
#     backend declares; flashinfer-build now installs whatever the tree
#     declares. See the comment in that stage.
#   - main snapshot (VLLM_REF=main in ./build, pinned at fe8f214, 2026-10-18;
#     was 4d9a8d6, 2026-10-15, 698e79f, 2026-10-13, df6c7ad, 2026-10-13, 187a0eb, 2026-10-10, b6d8e8a, 2026-10-06, bdd31c3, 2026-10-04, 84bcbc6, 2026-10-03, 47de9d4, 2026-10-01, 2b9b55c, 2026-09-27, a44d7b5, 2026-09-25, 36f94d5, 2026-09-24, 826e300, 2026-09-22, 468663a, 2026-09-18,
#     and 1ee4be4, 2026-09-12):
#     READ THIS BEFORE QUOTING A PIN: the "same torch 2.13.0 / torchvision 0.28.0" below
#     describes that 1ee4be4..698e79f line, NOT the current one. 4d9a8d6 moved to
#     torch 2.14.0 / torchvision 0.29.0 and flashinfer 0.7.1, and fe8f214 to
#     flashinfer 0.7.2 -- see the "698e79f -> 4d9a8d6" and "4d9a8d6 -> fe8f214"
#     paragraphs further down; the last one is the current state.
#     same torch 2.13.0 / torchaudio 2.11.0 /
#     torchvision 0.28.0; up to 826e300 flashinfer 0.6.18.post1 (cubin wheel published at
#     flashinfer.ai/whl), same requirements/build/cuda.txt, same
#     vllm-flash-attention CMakeLists (the sm120 patch below still matches),
#     same apache-tvm-ffi build range. Versus the release the runtime resolve
#     gains instanttensor, cutlass-dsl 4.7.1, quack 0.6.5, transformers>=5.10.4;
#     1ee4be4 -> 468663a only moves xgrammar to ==0.2.7 -- both checked to
#     resolve against torch 2.13.0 with uv. requirements/cuda.txt is byte-
#     identical between the two, so torch/torchaudio/flashinfer layers are
#     reused. What did change is a SYSTEM dep: vllm#56876 repinned DeepGEMM
#     (built for 12.0) to the vllm-project fork, whose deep_jit submodule needs
#     elfutils' libdw -- see vllm-build and vllm-openai. The new DeepSelect
#     extension is 10.0f-only and configures to an empty target here.
#     468663a -> 826e300 (2026-09-22): the only requirements change is
#     humming-kernels[cu13] 0.1.12 -> 0.1.15 (runtime resolve, vllm-openai
#     stage), so torch/torchaudio/flashinfer layers are reused again.
#     vllm-flash-attention moves 506341a -> 9cd61de with a byte-identical
#     CMakeLists (the sm120 patch dry-runs clean); DeepGEMM moves within the
#     same vllm-project fork (e1f418c), so the libdw handling is unchanged;
#     AllSpark (sm8x-only, never built here) is dropped from CMake.
#     826e300 -> 36f94d5 (2026-09-24): vllm#58069 moves flashinfer to 0.7.0
#     (python + cubin; cubin published at flashinfer.ai/whl) and humming to
#     0.1.16, so flashinfer-build recompiles. 0.7.0 split the AOT jit-cache
#     into a shim + per-arch provider wheel (flashinfer#4514) -- flashinfer-
#     build now builds flashinfer-jit-cache-sm120f + the shim when the tree
#     has flashinfer-jit-cache-provider/, and vllm-openai asserts the shim
#     finds it. New flashinfer runtime deps (nccl-extensions, nccl4py>=0.4.1,
#     cudnn-frontend>=1.29) resolve against torch 2.13.0 (uv dry-run of the
#     36f94d5 requirements/cuda.txt in the g468663a image, 2026-09-24). vLLM
#     extension pins are unchanged except QuTLASS compile flags (vllm#58173,
#     the torch 2.13 fix; it does build for 12.0f here).
#     36f94d5 -> a44d7b5 (2026-09-25, 59 commits): requirements/, CMake and
#     setup.py are unchanged (flashinfer stays 0.7.0), so torch/torchaudio/
#     flashinfer layers are reused and only vllm-build/vllm-openai rebuild.
#     Nothing build-side; the Flash-Next PLE overlay was re-derived because
#     vllm#57497 moved from_quant_config into qwen4_exp/common/.
#     a44d7b5 -> 2b9b55c (2026-09-27, 68 commits): requirements/ and setup.py
#     are unchanged (flashinfer stays 0.7.0), so torch/torchaudio/flashinfer
#     layers are reused and only vllm-build/vllm-openai rebuild. vllm_flash_attn.cmake
#     is unchanged (the sm120 hook below still lands). vllm#58846 moves FlashKDA
#     b59532f -> 17a037d (it builds for 12.0f here; only Kimi/GLM use it at
#     runtime); vllm#58586 drops all_reduce_mhc.cu from the stable extension
#     (now CuTe DSL). The PLE overlay's upstream file is byte-identical.
#     2b9b55c -> 47de9d4 (2026-10-01, 300 commits, past the v0.31.0 branch cut
#     so the snapshot version moves to 0.32.0.dev): flashinfer 0.7.0 ->
#     0.7.0.post1 (python + cubin; cubin published at flashinfer.ai/whl), so
#     flashinfer-build recompiles. post1 is two commits, both SM100/SM103 AOT
#     module selection (flashinfer#5544); the sm120f provider build is
#     unaffected. Runtime resolve: xgrammar 0.2.7 -> 0.2.8, openai-harmony ->
#     oss-harmony (a fork shipping the same module), raised floors on
#     setuptools (>=78.1.1, also in requirements/build/cuda.txt), requests,
#     aiohttp, pillow, filelock -- uv dry-run of the 47de9d4 requirements/
#     cuda.txt in the g2b9b55c image keeps torch 2.13.0 / torchaudio and
#     changes only flashinfer, xgrammar and oss-harmony. CMake: only
#     cpu_extension.cmake changed (vllm_flash_attn.cmake and every external-
#     project pin are untouched, so the sm120 hook below still lands).
#     47de9d4 -> 84bcbc6 (2026-10-03, 99 commits): torch 2.13.0 / torchaudio
#     2.11.0 / torchvision 0.28.0 / flashinfer 0.7.0.post1 and
#     requirements/build/cuda.txt are unchanged, so torch/torchaudio/flashinfer
#     layers are reused and only vllm-build/vllm-openai rebuild. Runtime
#     resolve: nvidia-cutlass-dsl[cu13] 4.7.1 -> 4.8.0 (vllm#59288; Qwen4Exp's
#     hc_down_silu / hyperconnection kernels are CuTe DSL, so this one is on
#     the Flash-Next path), tokenspeed-mla 0.1.8 -> 0.2.15 (vllm#59462, MLA
#     only), tokenizers >= 0.23.2, and transformers >= 5.16.1, < 5.18.0
#     (vllm#57387, vllm#59614) -- vLLM now uses transformers' own Qwen4Exp
#     config, and the cap moves the g47de9d4 image's 5.18.0 back to 5.17.0.
#     uv dry-run of the 84bcbc6 requirements/cuda.txt in the g47de9d4 image
#     keeps torch/torchaudio/flashinfer and changes only those three
#     (cutlass-dsl and its libs, tokenspeed-mla, transformers). CMake: a new
#     MiniMax-M3 fused kernel gets its own arch list, which for CUDA_ARCHS 12.0
#     is just 12.0 (as before, when it was part of the stable-extension
#     sources), and the MSA (fmha_sm100) pin moves; it only installs sources
#     for runtime JIT, nothing compiles here. vllm_flash_attn.cmake is
#     untouched, so the sm120 hook below still lands.
#     84bcbc6 -> bdd31c3 (2026-10-04, 12 commits): requirements/cuda.txt,
#     requirements/build/ and CMake are unchanged, so only vllm-build/vllm-openai
#     rebuild. vllm#59621 lifts the transformers ceiling to < 5.19.0, so the
#     fresh vllm-openai resolve takes 5.18.0 again (an in-place dry-run in the
#     g84bcbc6 image changes nothing, since its 5.17.0 still satisfies the range;
#     forcing transformers to re-resolve moves only it, 5.17.0 -> 5.18.0).
#     bdd31c3 -> b6d8e8a (2026-10-06, 124 commits): requirements/cuda.txt,
#     common.txt and build/ are unchanged (in-place dry-run: no changes), so
#     only vllm-build/vllm-openai rebuild. Build-side: vllm#48962 moves
#     DeepGEMM (e1f418c -> 1e1842a, still built for 12.0f) from per-Python
#     pybind _C.so to one TORCH_LIBRARY _C_extension.abi3.so, built by
#     DeepGEMM's own `setup.py build_ext` (a C++-only CUDAExtension; needs
#     wheel/setuptools/ninja/torch, all in this stage; its git-clean check falls
#     back to a +local version). python_api.cpp still reaches deep_jit's
#     utils/exception.hpp (via backend/cuda/*.hpp), so the libdw-dev step below
#     stays. A maintainer pushed a revert branch for #48962 the same day (no PR
#     or reason yet) -- if vllm-build fails in DeepGEMM, that is the suspect.
#     setup.py drops the optional PyO3 tool-parser Rust extension (vllm#59744;
#     no Rust toolchain needed, as before) and keeps flashinfer-jit-cache out of
#     the wheel's Requires-Dist (vllm#60052; layer 1 installs it from /wheels).
#     b6d8e8a -> 187a0eb (2026-10-10, 259 commits): torch 2.13.0 / torchaudio
#     2.11.0 / torchvision 0.28.0 / flashinfer 0.7.0.post1 and requirements/build/
#     are unchanged, so torch/torchaudio/flashinfer layers are reused and only
#     vllm-build/vllm-openai rebuild. requirements/cuda.txt drops flashinfer-cubin
#     and the flashinfer.ai/whl --extra-index-url (vllm#58765, `vllm
#     download-kernels`). Nothing here depended on that line: flashinfer-build and
#     vllm-openai install the cubin wheel themselves, from their own
#     --extra-index-url, and the wheel's Requires-Dist never carried it. The inline
#     comments in those two RUNs that say cuda.txt pins the cubin are now
#     historical; they are left as written so the RUN text (and the layer cache)
#     does not change. The new startup warning (warn_if_flashinfer_kernels_missing)
#     stays quiet: the image has flashinfer-python, -cubin and -jit-cache all at
#     0.7.0.post1. Runtime resolve: an in-place dry-run changes nothing;
#     vllm#60381 lifts the transformers ceiling to < 5.20.0, so the fresh
#     vllm-openai resolve takes 5.19.0 (forcing transformers to re-resolve in the
#     gb6d8e8a image moves only it, 5.18.0 -> 5.19.0; both Flash-Next checkpoints
#     load under 5.19.0 with their 12 QSA layers relabelled indexed_attention).
#     CMake: vllm#60259 makes GCC >= 13 a hard error for CUDA builds (DeepGEMM and
#     DeepSelect use C++20 <format>); this image's ubuntu24.04 gcc is 13.3.0.
#     vllm#60402 drops Python 3.10 (we run 3.12) and builds spinloop / fs_io_C
#     unconditionally (they were already built on 3.12). vllm_flash_attn.cmake and
#     every external-project pin are untouched, so the sm120 hook below still
#     lands, and no CUDA source was added.
#     187a0eb -> df6c7ad (2026-10-13, 104 commits): torch 2.13.0 / torchaudio
#     2.11.0 / torchvision 0.28.0 / flashinfer 0.7.0.post1 and requirements/build/
#     are unchanged -- requirements/cuda.txt and requirements/common.txt are
#     byte-identical across the window, and xpu.txt is the only requirements file
#     that moved at all -- so torch/torchaudio/flashinfer layers are reused and only
#     vllm-build/vllm-openai rebuild. Runtime resolve: an in-place dry-run changes
#     nothing and no requirement line moved, so there is nothing to --upgrade-package
#     and the fresh vllm-openai resolve takes the same set. CMake: vllm#60721 hoists
#     csrc/libtorch_stable/** into csrc/** -- paths, not content -- so vllm-build
#     reconfigures and recompiles, but the ccache mount re-keys on content and most of
#     it only re-links. No CUDA source is added or removed and the extension target
#     list is unchanged; the one non-path hunk drops a
#     target_compile_definitions(_C CUTLASS_ENABLE_DIRECT_CUDA_DRIVER_CALL) that was
#     already dead on CUDA builds (_C is ROCm-only here, as it was at 187a0eb), while
#     _C_stable_libtorch and _moe_C_stable_libtorch keep the define. vllm#60684 bumps
#     the MSA (fmha_sm100) pin and adds prefill_q8kv4/ install rules; that target only
#     installs sources for runtime JIT and compiles nothing, so it cannot change what
#     this file builds -- whether the new msa_warmup / q8kv4_prefill_adapter import
#     cleanly on sm_120 is unverified until a boot. vllm_flash_attn.cmake is
#     untouched, so the sm120 hook below still lands.
#     df6c7ad -> 698e79f (2026-10-13, 64 commits): torch 2.13.0 / torchaudio
#     2.11.0 / torchvision 0.28.0 / flashinfer 0.7.0.post1 and requirements/ are
#     unchanged -- cuda.txt, common.txt and build/cuda.txt are byte-identical over
#     the window, and the only requirements file that moves at all is the unused
#     kv_connectors_cu12.txt -- so torch/torchaudio/flashinfer layers are reused
#     and only vllm-build/vllm-openai rebuild. Two external pins moved, so this
#     vllm-build does real work: vllm-flash-attention 9cd61de -> b273a02, whose
#     CMakeLists is byte-identical and where the sm120 patch below was run against
#     the new tag (exit 0, both anchors matching exactly once), and DeepGEMM
#     1e1842a -> a981d8e (the fork's 26/09/30 2.8.1-based tag; #21 moves it from
#     TORCH_LIBRARY to TORCH_STABLE_LIBRARY, so _C recompiles and the install list
#     gains deep_gemm/locality_domain.py). libdw handling is unchanged: both tags
#     pin the same deep_jit (3732a3b), whose exception.hpp still includes
#     <elfutils/libdwfl.h> and dlopens libdw.so.1, so the libdw-dev / libdw1 steps
#     below stay. One CUDA source is added -- csrc/moe/fused_globalize_align_block_size.cu
#     joins VLLM_MOE_EXT_SRC with no arch guard and no arch-specific intrinsics, so
#     it compiles for 12.0 like the rest of _moe_C; whether it runs on sm_120 is
#     unverified until a boot. Runtime resolve: the in-place dry-run changes nothing
#     and no requirement line moved, so there is nothing to --upgrade-package; a
#     *fresh* resolve takes 7 newer packages (cuda-pathfinder 1.8.3, filelock 4.1.1,
#     instanttensor 0.2.2, markupsafe 3.0.4, numpy 2.3.5, nvidia-cudnn-frontend
#     1.31.0, starlette 1.8.0) -- that is the index ageing since the image was
#     built, not this snapshot moving them, since the pins are identical. Only
#     cudnn-frontend is on the Flash-Next path; nvidia-cutlass-dsl stays 4.8.0
#     (still PyPI's latest) and transformers stays 5.19.0, under which both
#     checkpoints load with their 12 QSA layers relabelled indexed_attention.
#     698e79f -> 4d9a8d6 (2026-10-15, 165 commits): THE FIRST REPIN TO MOVE
#     TORCH. vllm#52183 takes torch 2.13.0 -> 2.14.0 and torchvision 0.28.0 ->
#     0.29.0; the release set stays on 2.13.0/0.28.0, so main now pins its own
#     via VLLM_MAIN_TORCH_REF / VLLM_MAIN_TORCHVISION_VERSION and ./build
#     resolves them inside the VLLM_REF=main branch. Consequence, stated loudly
#     because it is the number that matters: torch-build is dead, and the stages
#     are a chain, so this is a FULL rebuild (torch hours -> torchaudio ->
#     flashinfer -> vllm -> openai), not the incremental vllm-only rebuild a
#     165-commit range would otherwise have been. ccache barely helps across a
#     torch minor bump. Budget hours, not minutes.
#     Two things ride along with the torch move:
#       - torchaudio stays 2.11.0 because main still pairs it that way (upstream's
#         own combination, so the pin did NOT move and ./preflight passes), but
#         torchaudio-build is FROM torch-build, so v2.11.0 sources now compile
#         against 2.14 headers with BUILD_RNNT/BUILD_ALIGN/BUILD_CUDA_CTC_DECODER
#         on. The CUDA-major.minor import guard is unaffected (both sides 13.2);
#         the open question is C++/ATen drift. It fails FAST in that stage --
#         after the torch hours, before flashinfer -- so it costs time, never a
#         broken runtime. NOT VERIFIED until the build runs.
#       - DeepGEMM 1e1842a..a981d8e -> 6ce1c5e and TORCH_STABLE_LIBRARY ->
#         TORCH_LIBRARY, i.e. back OFF the stable ABI. Attributed by line blame,
#         not by subject lines: the whole net change is #61510's revert; #61491
#         bumped the tag to 90a20e2 and its own revert #61703 cancelled it
#         exactly. So DeepGEMM's _C lands on torch's UNSTABLE C++ ABI at the same
#         repin that moves torch 2.13 -> 2.14, and it compiles for 12.0 here.
#         That is the one item in this bump that can fail in vllm-build or at
#         first boot -- watch that step personally.
#     Build-side, everything else: vllm_flash_attn.cmake is byte-for-byte
#     unchanged (GIT_TAG still b273a02, GIT_PROGRESS TRUE still exactly once), so
#     the sm120 patch below still applies and the FA row is pending-build -- only
#     ./container/archs proves the emitted archs. requirements/build/cuda.txt and
#     pyproject.toml's build-requires also say 2.14.0 now; both are inert here,
#     because use_existing_torch.py strips the torch pin before the build-stage
#     install and the build runs --no-build-isolation.
#     Runtime resolve (dry-run in the g698e79f image, in-place and
#     --upgrade-package per changed line, plus a whole-graph --upgrade): no
#     conflicts. flashinfer-python 0.7.0.post1 -> 0.7.1 (#60948) so
#     flashinfer-build recompiles (~35 min AOT; cubin 0.7.1 confirmed published at
#     flashinfer.ai/whl). humming-kernels[cu13] 0.1.16 -> 0.1.17 (#61556) -- ON
#     the Flash-Next path, this launcher's default --moe-backend. tokenspeed-mla
#     0.2.15 -> 2.0.2 (#61125, a MAJOR jump; MLA-only, off this path).
#     instanttensor's floor rises 0.1.9 -> 0.2.2 but the image already carries
#     0.2.2. Whole-graph re-resolve additionally ages anthropic, cuda-bindings,
#     cuda-pathfinder, cuda-python, filelock, markupsafe, numpy,
#     nvidia-cudnn-frontend, openai and pydantic-settings to newer allowed
#     versions -- that is the index moving, as ever. nvidia-cutlass-dsl stays
#     4.8.0 and transformers 5.19.0 (requirements/common.txt unchanged), under
#     which both checkpoints still load with their 12 QSA layers relabelled from
#     the raw config.json's full_attention to indexed_attention.
#     One resolve side effect worth knowing, not worth acting on: PyPI's torch
#     2.14.0 wheel metadata adds cuda-toolkit==13.0.3.0 and pulls
#     nvidia-cuda-runtime / nvidia-cuda-nvrtc DOWN 13.4.92 -> 13.0.96 / 13.0.88.
#     Do NOT conclude those wheels are in vllm-openai: layer 1 installs
#     /wheels-base/*.whl WITH deps, so what lands depends on the metadata of OUR
#     source-built torch wheel, which is unmeasured (that dry-run resolved PyPI's
#     wheel, not ours). We compile against the system CUDA 13.2 and nothing known
#     on this path loads a pip nvrtc. Treat this purely as a diagnostic: if a boot
#     ever dies inside a CUDA/nvrtc call, run
#     `python -c "import importlib.metadata as m;print(m.requires('torch'))"` in
#     the image BEFORE blaming the model. NOT VERIFIED either way.
#     4d9a8d6 -> fe8f214 (2026-10-18, 137 commits): torch 2.14.0 / torchaudio
#     2.11.0 / torchvision 0.29.0 are unchanged, so torch-build and
#     torchaudio-build are reused. flashinfer-python 0.7.1 -> 0.7.2 (vllm#61913;
#     cubin 0.7.2 published at flashinfer.ai/whl), so flashinfer-build recompiles
#     (~35 min AOT), then vllm-build and vllm-openai. flashinfer 0.7.2's own
#     build/runtime pins move only nvidia-cudnn-frontend >= 1.30.0 -> >= 1.31.0.
#     It also packages a TOP-LEVEL `b12x` module (flashinfer#5767: b12x 1.5.0
#     embedded under flashinfer.experimental.b12x, plus a compat package so
#     `import b12x` keeps working). So this image ships b12x for the first time,
#     with no extra installed. vllm/utils/b12x.py's find_spec("b12x") turns
#     True on every boot. All six submodules it imports exist in the embedded copy.
#     The b12x linears still rank behind FlashInfer, CUTLASS and Marlin in every
#     auto list, and the MoE oracle never auto-picks B12X. That is the
#     container/patches/b12x mount's problem, not this file's: see its UPSTREAM.
#     Build-side: DeepGEMM 6ce1c5e -> a7a9277 (vllm#61744), BACK ON the stable
#     ABI ("PyTorch 2.11 stable ABI and CPython 3.10 limited API"; the same PR
#     drops deep_gemm/_C*.so from check-torch-abi's unstable allow-list). That
#     reverses the 4d9a8d6 note above, and it still compiles for 12.0f here.
#     FlashMLA 0eee43b -> 5dceadb (vllm#61371, a source-tree reshuffle under
#     csrc/cuda_kernels/) supports only 9.0a/10.x, so it configures to an empty
#     target for CUDA_ARCHS 12.0, as before. vllm#62132 adds three CUDA sources,
#     each compiled for 12.0 with the rest of its extension:
#     muse_spark_mul_tanh_soft_cap and fused_muse_spark_qknorm_rope_kv_insert
#     (_C) and fused_qknorm_rope_cache (stable ext, CUDA only). Two changes are inert for
#     CUDA 13.2 / 12.0. vllm#60896 adds a CUDA < 13 else-branch for the fused GDN
#     decode archs. vllm#56549 (cmake/utils.cmake) keeps a plain arch when its
#     a/f variant is also requested, which only matters for lists like
#     "12.0;12.0f"; ours is the single plain 12.0. setup.py / pyproject.toml
#     changes are ROCm wheel naming, TheRock device extras and a typos exclude.
#     vllm_flash_attn.cmake is byte-for-byte unchanged (GIT_TAG still b273a02,
#     GIT_PROGRESS TRUE once), so the sm120 hook below still lands.
#     Runtime resolve (dry-run of the fe8f214 requirements/cuda.txt in the
#     g4d9a8d6 image, in place and --upgrade-package on both changed lines):
#     identical results. Only flashinfer-python 0.7.1 -> 0.7.2 (ours, built from
#     source) and tokenspeed-mla 2.0.2 -> 2.0.3 (vllm#61689, MLA-only, off this
#     path) move. torch, torchaudio, humming-kernels 0.1.17, nvidia-cutlass-dsl
#     4.8.0 and transformers 5.19.0 stay put.
#     Otherwise nothing in this file is snapshot-specific; ./build passes the pins.
#
# Source trees are supplied as named build-contexts (pytorch=, flashinfer=,
# vllm=) by ./build, so the giant repos never enter the default build context.
# ccache + uv caches use BuildKit cache mounts so an incremental change re-links
# in minutes instead of recompiling torch from scratch.

ARG CUDA_VERSION=13.2.1
ARG BASE_IMAGE=nvidia/cuda:${CUDA_VERSION}-cudnn-devel-ubuntu24.04

###############################################################################
# Stage: base — toolchain + python, shared by every build stage
###############################################################################
FROM ${BASE_IMAGE} AS base

ENV DEBIAN_FRONTEND=noninteractive \
    PIP_BREAK_SYSTEM_PACKAGES=1 \
    PATH=/usr/local/cuda/bin:/usr/lib/ccache:${PATH} \
    CUDA_HOME=/usr/local/cuda \
    CUDA_TOOLKIT_ROOT_DIR=/usr/local/cuda \
    CUDACXX=/usr/local/cuda/bin/nvcc \
    LD_LIBRARY_PATH=/usr/local/cuda/lib64 \
    CMAKE_C_COMPILER_LAUNCHER=ccache \
    CMAKE_CXX_COMPILER_LAUNCHER=ccache \
    CMAKE_CUDA_COMPILER_LAUNCHER=ccache \
    CCACHE_DIR=/root/.cache/ccache \
    CCACHE_MAXSIZE=40G \
    UV_LINK_MODE=copy

# Ubuntu 24.04 (noble) ships Python 3.12 natively — no PPA needed.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates python3 python3-dev python3-venv \
        git curl wget build-essential cmake ninja-build ccache \
        libopenmpi-dev libnuma-dev libssl-dev pkg-config && \
    curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR=/usr/local/bin sh
# NOTE: no nvcc symlink in /usr/lib/ccache — it would make CMake's FindCUDA infer
# the toolkit root as /usr/lib. nvcc is cached via CMAKE_CUDA_COMPILER_LAUNCHER=ccache.

# A single venv reused across stages (copied forward via /opt/venv).
ENV VIRTUAL_ENV=/opt/venv PATH=/opt/venv/bin:/usr/local/cuda/bin:/usr/lib/ccache:${PATH}
RUN uv venv --python python3 ${VIRTUAL_ENV} && \
    uv pip install --python ${VIRTUAL_ENV}/bin/python \
        "setuptools>=77" wheel "cmake>=3.26" ninja pybind11 "numpy<2.3" pyyaml typing_extensions

###############################################################################
# Stage: torch-build — PyTorch from source for sm_120
###############################################################################
FROM base AS torch-build

ARG TORCH_CUDA_ARCH_LIST=12.0
ARG TORCH_VERSION=2.13.0
# Pin a clean wheel version (e.g. 2.13.0, not 2.13.0a0+gitXXXX) so vLLM's
# `torch==2.13.0` dependency resolves to our wheel instead of pulling from PyPI.
# Everything here is semantic — it changes the artifact, so it *should* bust the
# cache. Build-speed knobs deliberately live elsewhere; see the RUN below.
ENV TORCH_CUDA_ARCH_LIST=${TORCH_CUDA_ARCH_LIST} \
    PYTORCH_BUILD_VERSION=${TORCH_VERSION} \
    PYTORCH_BUILD_NUMBER=1 \
    USE_CUDA=1 USE_CUDNN=1 USE_CUSPARSELT=1 \
    USE_FLASH_ATTENTION=1 USE_MEM_EFF_ATTENTION=1 \
    BUILD_TEST=0 USE_DISTRIBUTED=1 \
    _GLIBCXX_USE_CXX11_ABI=1

# pytorch source supplied as a build-context (recursive submodules already
# checked out on the host by ./build).
#
# .git is excluded here rather than by a .dockerignore inside the source tree:
# ./build runs `git checkout --force` against those trees, and pytorch's
# .dockerignore is a symlink to .gitignore, which does NOT exclude .git. That put
# 5.8 GB of git objects (72% of the context) into this COPY's cache key, churned
# by the `git fetch` every ./build does — invalidating torch, and with it
# flashinfer and vllm downstream. Nothing here needs git metadata:
# PYTORCH_BUILD_VERSION above is what sets the wheel version.
# The exclude only governs this COPY; BuildKit still TRANSFERS the whole context.
# So ./build hands over rsync'd copies without .git ($CTX_DIR) and never sends it
# at all. These excludes stay for CTX_DIR= (contexts straight from the trees).
COPY --from=pytorch --exclude=.git --exclude=**/.git . /src/pytorch
WORKDIR /src/pytorch
# MAX_JOBS/NVCC_THREADS arrive by secret mount, NOT as ARG/ENV. BuildKit excludes
# secret *contents* from the cache key, which is exactly the semantics wanted:
# these knobs change how fast the build runs, never what it produces. As ARG/ENV
# they were part of every layer key, so retuning MAX_JOBS forced a full
# torch+flashinfer+vllm recompile. Please don't "simplify" this back into ENV.
# The -f guard and := defaults keep a bare `docker build` (no --secret) working.
# torch >= 2.14 builds through scikit-build-core: setup.py is a shim that fails every subcommand
# but install/develop, so `setup.py bdist_wheel` is gone and 2.16 removes the shim outright.
# --no-isolation is required -- cmake/ninja/CUDA and requirements.txt live in THIS venv -- and
# `build` then checks [build-system] requires and aborts loudly rather than proceeding. --outdir
# skips dist/ deliberately: `build` does not clean it, and ./build rsyncs this tree with --delete
# --exclude=.git, so a stale dist/ would ride in and a `dist/*.whl` glob would hand layer 1 two
# torch wheels to resolve. -v records the resolved backend instead of a bare "backend exited".
RUN --mount=type=cache,target=/root/.cache/ccache \
    --mount=type=cache,target=/root/.cache/uv \
    --mount=type=secret,id=buildjobs \
    { [ -f /run/secrets/buildjobs ] && . /run/secrets/buildjobs; } ; \
    : "${MAX_JOBS:=8}" "${NVCC_THREADS:=1}" ; \
    export MAX_JOBS NVCC_THREADS CMAKE_BUILD_PARALLEL_LEVEL="$MAX_JOBS" ; \
    uv pip install -r requirements.txt && \
    mkdir -p /wheels && \
    python -m build -v --no-isolation --wheel --outdir /wheels . && \
    ls -l /wheels/torch-${TORCH_VERSION}-*.whl && \
    uv pip install /wheels/torch-*.whl && \
    ccache -s

###############################################################################
# Stage: torchaudio-build — torchaudio from source against the source torch
###############################################################################
# Separate from the flashinfer -> vllm chain so it never invalidates those
# layers. Small: one CppExtension/CUDAExtension (rnnt, align, ctc decoder), no
# submodules, no cmake. BUILD_VERSION pins the clean wheel version (setup.py
# would otherwise append +<sha>, which we have no .git for anyway) so vLLM's
# torchaudio==2.11.0 pin resolves to this wheel in vllm-openai.
FROM torch-build AS torchaudio-build

ARG TORCH_CUDA_ARCH_LIST=12.0
ARG TORCHAUDIO_VERSION=2.11.0
ENV TORCH_CUDA_ARCH_LIST=${TORCH_CUDA_ARCH_LIST} \
    BUILD_VERSION=${TORCHAUDIO_VERSION} \
    USE_CUDA=1 BUILD_RNNT=1 BUILD_ALIGN=1 BUILD_CUDA_CTC_DECODER=1 BUILD_CPP_TEST=0

COPY --from=torchaudio --exclude=.git --exclude=**/.git . /src/torchaudio
WORKDIR /src/torchaudio
RUN --mount=type=cache,target=/root/.cache/ccache \
    --mount=type=cache,target=/root/.cache/uv \
    --mount=type=secret,id=buildjobs \
    { [ -f /run/secrets/buildjobs ] && . /run/secrets/buildjobs; } ; \
    : "${MAX_JOBS:=8}" ; export MAX_JOBS ; \
    python setup.py bdist_wheel && \
    mkdir -p /wheels-torchaudio && cp dist/torchaudio-*.whl /wheels-torchaudio/ && \
    ls -la /wheels-torchaudio && \
    uv pip install --no-deps /wheels-torchaudio/*.whl && \
    python -c "import torch, torchaudio; print('torchaudio', torchaudio.__version__, 'cuda', torch.ops._torchaudio.cuda_version())"

###############################################################################
# Stage: flashinfer-build — FlashInfer from source against the source torch
###############################################################################
FROM torch-build AS flashinfer-build

ARG TORCH_CUDA_ARCH_LIST=12.0
ARG CUDA_VERSION
# Must match FLASHINFER_REF in ./build (e.g. v0.6.18 -> 0.6.18); used to pull the
# matching prebuilt flashinfer-cubin wheel instead of rebuilding it from source.
ARG FLASHINFER_VERSION=0.6.18
# 12.0f = sm120 family-conditional kernels (RTX PRO 6000 Blackwell). FlashInfer
# 0.6.x/0.7.x have no setup.py; they build via PEP 517 backends (python -m build).
# 0.7.0's provider backend takes the same value as FLASHINFER_JIT_CACHE_PROVIDER_ARCH.
ENV FLASHINFER_CUDA_ARCH_LIST="12.0f" \
    TORCH_CUDA_ARCH_LIST=${TORCH_CUDA_ARCH_LIST}

# flashinfer's own .dockerignore already excludes .git, but it is an untracked
# file in a tree ./build runs `git checkout --force` against — state the
# exclusion here too so it cannot be clobbered. See torch-build for the rationale.
COPY --from=flashinfer --exclude=.git --exclude=**/.git . /src/flashinfer
WORKDIR /src/flashinfer
# Build-speed knobs by secret mount — see torch-build above.
RUN --mount=type=cache,target=/root/.cache/ccache \
    --mount=type=cache,target=/root/.cache/uv \
    --mount=type=secret,id=buildjobs \
    { [ -f /run/secrets/buildjobs ] && . /run/secrets/buildjobs; } ; \
    : "${MAX_JOBS:=8}" "${NVCC_THREADS:=1}" ; \
    export MAX_JOBS NVCC_THREADS CMAKE_BUILD_PARALLEL_LEVEL="$MAX_JOBS" ; \
    uv pip install build "setuptools>=77" "packaging>=24" wheel ninja numpy \
        tqdm requests filelock nvidia-ml-py \
        "apache-tvm-ffi>=0.1.11,<0.2" && \
    # (0.1.11 is flashinfer 0.7.0's build-system floor and vLLM's ==pin; it also
    # satisfies 0.6.x's >=0.1.6,!=0.1.8 range.)
    # flashinfer >= 0.6.18 declares extra "provider wheel" build deps in
    # ci/cuda-versions.json (dependency_policy; today nvidia-cutlass-dsl[cu13]
    # >=4.6.2a0), returned by build_utils.get_build_dependency_requirements()
    # from the AOT backend's get_requires_for_build_wheel hook (jit-cache in
    # 0.6.18, jit-cache-provider since 0.7.0). Under
    # --no-isolation `python -m build` only *checks* them, and aborts with
    # "ERROR Unmet dependencies" (hit on the v0.29.0 bump, 2026-09-12). Ask the
    # tree instead of hardcoding, so the next policy entry does not repeat this.
    # CUDA_MAJOR selects the cu13 extra. Tags before 0.6.18 have no build_utils
    # and declare nothing extra: the import fails and the list is empty, and if
    # the helper ever breaks for another reason `python -m build` still stops
    # the build with the same Unmet error. Whatever is installed here stays in
    # this stage; vllm-openai starts from base and installs vLLM's own pin.
    _fi_build_deps=$(CUDA_MAJOR="${CUDA_VERSION%%.*}" python -c 'import build_utils; print(" ".join(build_utils.get_build_dependency_requirements()))' 2>/dev/null || true) && \
    echo "flashinfer provider build deps: ${_fi_build_deps:-<none declared>}" && \
    { [ -z "$_fi_build_deps" ] || uv pip install $_fi_build_deps; } && \
    mkdir -p /wheels && \
    # core flashinfer-python (kernels still JIT-able at runtime as a fallback)
    python -m build --no-isolation --wheel --outdir /wheels . && \
    uv pip install --no-deps /wheels/flashinfer_python-*.whl && \
    # prebuilt cubins; install before jit-cache so the AOT step can reuse them
    # instead of refetching. (vLLM's requirements/cuda.txt pins flashinfer-cubin
    # but its setup.py drops the pin from the wheel, so vllm-openai installs the
    # same wheel again for the runtime venv.) The cubin package is a
    # py3-none-any blob of kernels NVIDIA already compiled — nothing to build for
    # sm120 — so we pull the prebuilt wheel instead of rebuilding it. Building it
    # from source scrapes ~15.6k files off NVIDIA's flaky edge.urm.nvidia.com CDN
    # all-or-nothing, and a single failed file aborts the whole image build.
    # flashinfer-cubin stopped publishing to PyPI after 0.6.13, so it comes from
    # flashinfer.ai/whl — the same --extra-index-url vLLM uses in requirements/cuda.txt.
    uv pip install --no-deps --extra-index-url https://flashinfer.ai/whl/ \
        "flashinfer-cubin==${FLASHINFER_VERSION}" && \
    # AOT-precompiled sm120 kernels (the heavy compile). Two layouts:
    #   <= 0.6.x: flashinfer-jit-cache IS the AOT package; its backend compiles
    #     for FLASHINFER_CUDA_ARCH_LIST.
    #   >= 0.7.0 (flashinfer#4514): the kernels moved into one wheel per arch,
    #     built from flashinfer-jit-cache-provider with
    #     FLASHINFER_JIT_CACHE_PROVIDER_ARCH (exactly one arch; the backend
    #     overrides FLASHINFER_CUDA_ARCH_LIST with it) -> distribution
    #     flashinfer-jit-cache-sm120f, discovered at runtime through the
    #     flashinfer.jit_cache.providers entry point. flashinfer-jit-cache is
    #     now a metadata-only shim whose Requires-Dist names those providers,
    #     and its backend refuses to import without
    #     FLASHINFER_JIT_CACHE_PROVIDER_ARCHS. flashinfer only looks for
    #     providers through the shim (jit/env.py _get_aot_providers), so both
    #     wheels are needed; both land in /wheels, which is what lets the
    #     vllm-openai `uv pip install /wheels/*.whl` satisfy the shim's pin.
    #     sm120f scores as compatible with an sm120 device
    #     (cuda_binary_target_compatibility_score('sm120f', 'sm120') = (2, 0)).
    if [ -d flashinfer-jit-cache-provider ]; then \
        FLASHINFER_JIT_CACHE_PROVIDER_ARCH="$FLASHINFER_CUDA_ARCH_LIST" \
            python -m build --no-isolation --wheel --outdir /wheels ./flashinfer-jit-cache-provider && \
        FLASHINFER_JIT_CACHE_PROVIDER_ARCHS="$FLASHINFER_CUDA_ARCH_LIST" \
            python -m build --no-isolation --wheel --outdir /wheels ./flashinfer-jit-cache && \
        uv pip install --no-deps /wheels/flashinfer_jit_cache_sm*.whl /wheels/flashinfer_jit_cache-*.whl ; \
    else \
        python -m build --no-isolation --wheel --outdir /wheels ./flashinfer-jit-cache && \
        uv pip install --no-deps /wheels/flashinfer_jit_cache-*.whl ; \
    fi && \
    ls -la /wheels/flashinfer_* && \
    ccache -s

###############################################################################
# Stage: vllm-build — vLLM from source against source torch + flashinfer
###############################################################################
FROM flashinfer-build AS vllm-build

ARG TORCH_CUDA_ARCH_LIST=12.0
ARG VLLM_VERSION=0.29.0
# VLLM_VERSION_OVERRIDE, *not* SETUPTOOLS_SCM_PRETEND_VERSION_FOR_VLLM. The
# scoped form does not work here: setup.py calls setuptools-scm's get_version()
# with no dist_name (setup.py:1026 in v0.29.0), so there is nothing for the
# _FOR_VLLM suffix to match and the lookup fails — verified directly against
# setuptools-scm 10.2.1 / vcs-versioning 2.2.4. It silently appeared to work only
# while .git was in the build context, because setuptools-scm was reading the
# git tags instead. VLLM_VERSION_OVERRIDE is vLLM's own hook (setup.py:1020 in
# v0.29.0, :1261 at main 468663a and 826e300 -- same early return) and sets the *unscoped*
# SETUPTOOLS_SCM_PRETEND_VERSION internally.
ENV TORCH_CUDA_ARCH_LIST=${TORCH_CUDA_ARCH_LIST} \
    VLLM_USE_PRECOMPILED=0 \
    VLLM_VERSION_OVERRIDE=${VLLM_VERSION}

# elfutils headers for DeepGEMM. Since vllm#56876 (2026-09-15) vLLM pins the
# vllm-project/DeepGEMM fork, whose third-party/deep_jit submodule does
# `#include <elfutils/libdwfl.h>` (deep_jit/utils/exception.hpp, pulled in by
# csrc/python_api.cpp). DeepGEMM's arch list includes 12.0, and its _C target is
# `ALL`, not optional -- without the header the vLLM build fails, late. Upstream
# added `elfutils-devel` to its manylinux image in the same commit; libdw-dev is
# the noble package that owns that header (and depends on libelf-dev).
# Installed HERE rather than in base on purpose: base is the parent of every
# compile stage, so touching its apt list would recompile torch and flashinfer
# for a header only this stage reads. Harmless for v0.29.0, which never looks.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends libdw-dev && \
    test -f /usr/include/elfutils/libdwfl.h

# .git excluded (see torch-build). vLLM is the case where that matters most —
# setuptools-scm derives the version from git tags — so VLLM_VERSION_OVERRIDE
# above is load-bearing, not belt-and-braces: without it this build fails with
# "setuptools-scm was unable to detect version for /src/vllm".
# It also yields a clean 0.29.0 rather than the 0.29.0.dev0+g<sha>.d<date> that
# git derivation produced (use_existing_torch.py edits pyproject.toml before
# setup.py runs, so the tree is always dirty by then). Snapshot builds pass
# <next release>.dev<date>+g<sha7> here (./build derives it), which is the only place
# the vLLM commit survives into the image besides the ai.vllmcustom.vllm.ref label.
# Verify with `vllm.__version__` after changing either.
COPY --from=vllm --exclude=.git --exclude=**/.git . /src/vllm
WORKDIR /src/vllm
# vLLM's CMake fetches vllm-flash-attention (FetchContent, pinned sha) and builds
# it with its OWN arch rules, ignoring TORCH_CUDA_ARCH_LIST: FA2 as sm_80+PTX
# (PTX-JIT'd on sm120, never native) and FA3 as 192 Hopper-only kernels that,
# with no sm90 target, get nvcc's default sm_75 -- 818 MB that cannot run here.
# Hook a PATCH_COMMAND into the FetchContent declaration so the clone is edited
# in place: FA2 -> the requested archs, FA3 -> off. Rationale and the exact
# edits live in the patch script. The grep asserts the hook landed on the git
# branch of the declaration (the VLLM_FLASH_ATTN_SRC_DIR branch has no clone to
# patch), so a vLLM bump that rewrites this file fails here, not 40 min later.
COPY patches/vllm-flash-attn-arch.py /opt/patches/vllm-flash-attn-arch.py
RUN f=cmake/external_projects/vllm_flash_attn.cmake && \
    sed -i '/GIT_PROGRESS TRUE/a\          PATCH_COMMAND /opt/venv/bin/python /opt/patches/vllm-flash-attn-arch.py' "$f" && \
    [ "$(grep -c 'PATCH_COMMAND /opt/venv/bin/python /opt/patches/vllm-flash-attn-arch.py' "$f")" = 1 ] && \
    grep -A4 'GIT_REPOSITORY https://github.com/vllm-project/flash-attention.git' "$f"
# Don't let vLLM's pinned torch wheel clobber our source build.
# v0.23.0 replaced requirements/build.txt with a requirements/build/<platform>.txt
# split; use the cuda list. use_existing_torch.py globs requirements/**/*.txt, so it
# still strips the torch==2.13.0 pin from it, leaving our source torch in place.
RUN --mount=type=cache,target=/root/.cache/ccache \
    --mount=type=cache,target=/root/.cache/uv \
    --mount=type=secret,id=buildjobs \
    { [ -f /run/secrets/buildjobs ] && . /run/secrets/buildjobs; } ; \
    : "${MAX_JOBS:=8}" "${NVCC_THREADS:=1}" ; \
    export MAX_JOBS NVCC_THREADS CMAKE_BUILD_PARALLEL_LEVEL="$MAX_JOBS" ; \
    python use_existing_torch.py 2>/dev/null || true && \
    uv pip install -r requirements/build/cuda.txt && \
    python setup.py bdist_wheel && \
    mkdir -p /wheels && cp dist/*.whl /wheels/ && \
    ccache -s

###############################################################################
# Stage: vllm-openai — runtime image (keeps CUDA devel toolkit for flashinfer JIT)
###############################################################################
FROM base AS vllm-openai

ARG TORCHVISION_VERSION=0.28.0
# torchvision publishes a wheel per CUDA minor (cu130, cu132, ...); ./build
# derives the index from CUDA_VERSION and preflight checks the wheel exists. The
# wheel's CUDA minor MUST equal torch's: torchvision/extension.py compares them
# at import and raises. (torchaudio has the same check and no cu132 wheel, which
# is why it is built from source in torchaudio-build instead.)
ARG TORCHVISION_INDEX=cu132

# Declared before the RUN because it is used in it (unlike the label-only ARGs
# below). No cache cost: a flashinfer bump already changes the wheels this layer
# installs, so the layer is rebuilt either way.
ARG FLASHINFER_VERSION

# Runtime half of the DeepGEMM/deep_jit elfutils dependency (see vllm-build):
# deep_jit dlopen()s "libdw.so.1" to symbolize the stack trace it attaches to a
# kernel/JIT exception. A failed dlopen is tolerated (frames print as ??:0), so
# this is not needed to serve -- it is what makes a DeepGEMM failure readable.
# This stage starts from base, not vllm-build, so the -dev install there does
# not reach it. Before the wheel installs so a vLLM bump does not redo it.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends libdw1 && \
    python -c "import ctypes; ctypes.CDLL('libdw.so.1')"

# The locally-built wheels go in as two layers, split by how often they change.
# One install of everything made each vLLM bump reinstall and re-export a
# 12.4 GB layer, 8.7 GB of it torch + flashinfer (flashinfer-cubin alone is
# 6.4 GB) that the bump never touched. The wheels are bind-mounted from their
# stages rather than COPY'd, so they leave no layer of their own behind.
#
# Layer 1, torch + flashinfer + torchaudio: flashinfer-build's /wheels is torch-
# build's torch wheel plus the flashinfer wheels (vllm-build only adds vLLM on
# top). Nothing vLLM-specific is in this RUN's cache key, so a vLLM bump reuses it.
RUN --mount=type=cache,target=/root/.cache/uv \
    --mount=type=bind,from=flashinfer-build,source=/wheels,target=/wheels-base \
    --mount=type=bind,from=torchaudio-build,source=/wheels-torchaudio,target=/wheels-torchaudio \
    uv pip install /wheels-base/*.whl /wheels-torchaudio/*.whl && \
    # flashinfer-cubin: pinned in vLLM's requirements/cuda.txt, but setup.py
    # strips it from the wheel's Requires-Dist (it left PyPI after 0.6.13), so
    # vLLM's resolve never installs it and flashinfer would instead download
    # cubins into ~/.cache/flashinfer at first use, at serve time. Pull the same
    # prebuilt wheel flashinfer-build uses, from the same index.
    uv pip install --no-deps --extra-index-url https://flashinfer.ai/whl/ \
        "flashinfer-cubin==${FLASHINFER_VERSION}" && \
    { uv pip install --no-deps --index-url https://download.pytorch.org/whl/${TORCHVISION_INDEX} \
        torchvision==${TORCHVISION_VERSION} || \
      echo "WARN: torchvision ${TORCHVISION_VERSION} ${TORCHVISION_INDEX} wheel unavailable; multimodal disabled"; }

# Layer 2, vLLM (rebuilt on every bump; ~3.3 GB with the deps it adds). vLLM's
# torch==/torchaudio==/flashinfer-python== pins are already satisfied by layer 1's
# wheels, so uv leaves those alone instead of pulling PyPI's cu130 builds. Only the
# vLLM wheel is named: its neighbours in this /wheels are layer 1's, and naming
# them again by a new path would make uv reinstall them here.
RUN --mount=type=cache,target=/root/.cache/uv \
    --mount=type=bind,from=vllm-build,source=/wheels,target=/wheels \
    uv pip install /wheels/vllm-*.whl && \
    # Two resolves where there used to be one: fail here if they left the env
    # inconsistent (it passes on the single-resolve 2b9b55c image).
    uv pip check && \
    # Fail the build here, not at `vllm serve`, if any CUDA-minor check trips,
    # and confirm the cubin package is importable (flashinfer.jit.env checks
    # for the flashinfer_cubin module, not the dist name).
    python -c "import torch, torchvision, torchaudio, flashinfer_cubin; \
               print('torch', torch.__version__, torch.version.cuda, \
                     'torchvision', torchvision.__version__, \
                     'torchaudio', torchaudio.__version__, \
                     'flashinfer-cubin', flashinfer_cubin.__file__)" && \
    # flashinfer >= 0.7.0: the AOT kernels are only found if the jit-cache shim
    # discovers the sm120f provider wheel (see flashinfer-build). A miss is not
    # an error at serve time -- flashinfer silently JIT-compiles every kernel on
    # first use -- so assert it here. 0.6.x has no provider API: skipped.
    python -c "import flashinfer_jit_cache as j; \
               f = getattr(j, 'get_jit_cache_providers', None); \
               ps = f() if f else None; \
               print('flashinfer-jit-cache', j.__version__, 'providers:', \
                     'n/a (<0.7)' if ps is None else [(p.provider_id, len(p.modules)) for p in ps]); \
               assert ps is None or any(a.startswith('sm120') for p in ps for a in p.cuda_architectures), \
                      'flashinfer-jit-cache shim found no sm120 provider'"

ENV VLLM_HOST_IP=0.0.0.0 \
    HF_HUB_ENABLE_HF_TRANSFER=1 \
    DO_NOT_TRACK=1

# Stamp the version set into the image so ./push can derive its tags by reading
# the image rather than re-declaring the versions (which is how ./push's
# CUDA_VERSION drifted from ./build's). Declared here, *after* the expensive RUN,
# so a version change only rebuilds this metadata layer.
#
# ARG re-declaration is required per stage; these inherit the values ./build
# already passes with --build-arg.
ARG CUDA_VERSION
ARG TORCH_VERSION
ARG VLLM_VERSION
# The git ref vLLM was built from: a tag (v0.29.0) or, for a snapshot, the full
# commit sha. The version label alone cannot say that for a release build.
ARG VLLM_REF
ARG TORCH_CUDA_ARCH_LIST
LABEL org.opencontainers.image.title="vllm-blackwell" \
      org.opencontainers.image.description="Source-built sm120-optimized vLLM (RTX PRO 6000 Blackwell)" \
      org.opencontainers.image.version="${VLLM_VERSION}" \
      ai.vllmcustom.vllm.version="${VLLM_VERSION}" \
      ai.vllmcustom.vllm.ref="${VLLM_REF}" \
      ai.vllmcustom.torch.version="${TORCH_VERSION}" \
      ai.vllmcustom.flashinfer.version="${FLASHINFER_VERSION}" \
      ai.vllmcustom.cuda.version="${CUDA_VERSION}" \
      ai.vllmcustom.cuda.arch="${TORCH_CUDA_ARCH_LIST}"

EXPOSE 8799
# Use `vllm serve`, NOT `python -m vllm.entrypoints.openai.api_server`. Only the
# `vllm serve` CLI copies the positional model_tag onto the `model` field
# (entrypoints/cli/serve.py); the bare api_server module does not. With the module
# entrypoint, `model` stays at vLLM's default (Qwen/Qwen3-0.6B), so speculative
# decoding builds its draft from that default instead of the served checkpoint —
# breaking MTP with "Unsupported speculative method: 'mtp'".
ENTRYPOINT ["vllm", "serve"]

# Run as the base image's own unprivileged user, so nothing a serve does can leave root-owned
# files in the host-mounted cache tree. Two facts make this metadata-only rather than a RUN layer:
# the base (`nvidia/cuda:13.2.1-cudnn-devel-ubuntu24.04`, the ARG default at :178) already ships
# /home/ubuntu as 0750 ubuntu:ubuntu — measured on that tag 2026-10-13, so `install -d` here would
# be a no-op — and USER adds no filesystem bytes. Re-measure that mode/owner if BASE_IMAGE moves.
#
# HOME is set because docker does not derive it for a *numeric* user: without it a hand-run
# `docker run` (no launcher flags) gets HOME=/ and every cache path resolves against the root
# filesystem. CCACHE_DIR is overridden because :195 points it into /root, which uid 1000 cannot
# write, while the ccache wrapper stays ahead of /usr/bin on PATH (:211) — that combination does
# not break a compile (measured: `cc` exits 0, ccache degrades) but silently runs every serve-time
# host compile uncached, forever. Redirected here it caches, and lands inside the mounted tree.
#
# KEEP THESE THE LAST INSTRUCTIONS IN THE FILE: a RUN/COPY appended after USER runs as uid 1000 and
# half-fails. Gate step 7d (context/plans/RULES.md) is a host-side grep that only means something
# while this ordering holds.
ENV HOME=/home/ubuntu \
    CCACHE_DIR=/home/ubuntu/.cache/ccache
USER 1000:1000
