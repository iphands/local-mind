# shellcheck shell=bash
# Shared launcher mechanism for qwen/run, laguna/run, muse/run (sourced, not executed).
#
# Each model family keeps its own run script, because what a model needs -- its
# parsers, its sampling defaults, its patches -- is genuinely per-model. What
# they share is the plumbing: which image, which directory becomes /models, the
# docker flags, the `vllm serve` flags that are the same everywhere, and the
# launch. That is what lives here.
#
# ORDER MATTERS. Each launcher does:
#
#   NAME=... ; vllm_image          -> CONTAINER
#   resolve_model <spec>           -> MODEL / MODEL_ROOT / MODEL_FOLDER
#   <per-model tunables>
#   require_image
#   docker_core_args               -> ASSIGNS CONTAINER_ARGS
#   CONTAINER_ARGS+=( ... )           per-model
#   env_extra_args                    LAST CONTAINER_ARGS mutation
#   vllm_core_args                 -> ASSIGNS VLLM_ARGS
#   VLLM_ARGS+=( ... )                per-model
#   vllm_extra_args                   LAST VLLM_ARGS mutation
#   vllm_launch
#
# Three rules that are easy to break and hard to notice:
#
#  1. env_extra_args / vllm_extra_args must come LAST. Both are caller overrides
#     (ENV_EXTRA, EXTRA_ARGS) that work by last-wins -- docker honours the last
#     duplicate -e, argparse the last duplicate flag. Anything appended after
#     them silently wins instead, and the override stops working with no error.
#  2. Core builders ASSIGN their array; per-model code APPENDS. Call the core
#     first or your additions are thrown away.
#  3. Never append a bare (non---) token to VLLM_ARGS. --served-model-name takes
#     nargs='+', so a bare token drifting to the end is swallowed as a sixth
#     model alias rather than read as a positional.
#
# Every function here ends in an explicit `return 0`. Under `set -e` a function
# whose last statement is `[[ ... ]] && cmd` returns 1 when the test is false and
# takes the caller down with it. The same line is harmless at top level, which is
# where all of this used to live -- so the idiom looks safe and isn't.

# shellcheck source-path=SCRIPTDIR source=./_model-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_model-lib.sh"

# ---------------------------------------------------------------- image

vllm_image() {
  IMAGE=${IMAGE:-iphands/vllm-blackwell}
  IMAGE_TAG=${IMAGE_TAG:-latest}
  CONTAINER="$IMAGE:$IMAGE_TAG"
  return 0
}

require_image() {
  docker image inspect "$CONTAINER" >/dev/null 2>&1 || {
    echo "Image $CONTAINER not found. Run ./container/build first."; exit 1; }
  return 0
}

# ---------------------------------------------------------------- docker run

# Assigns CONTAINER_ARGS. Reads NAME, PORT, MODEL_FOLDER, CACHE_DIR, DETACH.
docker_core_args() {
  : "${NAME:?}" "${PORT:?}" "${MODEL_FOLDER:?}" "${CACHE_DIR:?}"
  local -a _runmode
  # DETACH=1 -> run backgrounded (-d) for scripted benchmarking (bench-wrapper,
  # scripts/muse-*-sweep); interactive (-it) otherwise.
  if [[ -n "${DETACH:-}" ]]; then _runmode=(-d); else _runmode=(-it); fi
  CONTAINER_ARGS=(
    run "${_runmode[@]}" --rm
    --name "$NAME"
    # Force the `vllm serve` entrypoint (we pass `serve` as the first arg below).
    # Required for speculative decoding: only `vllm serve` copies the positional
    # model path onto the `model` field; the bare api_server module leaves `model`
    # at vLLM's default (Qwen/Qwen3-0.6B), so the draft loads that default and
    # fails with "Unsupported speculative method". --entrypoint REPLACES the image
    # ENTRYPOINT, so this works whether or not the image has been rebuilt.
    --entrypoint vllm
    --gpus device=0                                # RTX PRO 6000 only — never the 4060
    -e CUDA_DEVICE_ORDER=PCI_BUS_ID
    -e VLLM_WORKER_MULTIPROC_METHOD=spawn
    -e SAFETENSORS_FAST_GPU=1
    -e VLLM_LOG_STATS_INTERVAL=1                   # log tok/s + acceptance every 1s (tuning)
    -p "0.0.0.0:${PORT}:8799"
    -v "$MODEL_FOLDER":/models:ro
    -v "$CACHE_DIR":/root/.cache:rw
    --ipc=host
    --shm-size=32g
    --ulimit memlock=-1
    --ulimit stack=67108864
  )
  return 0
}

# Appends ENV_EXTRA to CONTAINER_ARGS. Call last -- see rule 1 above.
env_extra_args() {
  local _kv
  # Word-splitting is the point: ENV_EXTRA is a space-separated KEY=VALUE list.
  # shellcheck disable=SC2086
  for _kv in ${ENV_EXTRA:-}; do CONTAINER_ARGS+=( -e "$_kv" ); done
  return 0
}

# ---------------------------------------------------------------- vllm serve

# _core_add --flag [value...]
# Appends unless CORE_OMIT names the flag, in which case the flag AND its values
# are dropped together. That is the supported way for a model to decline a core
# flag: do not try to shadow it by appending a second copy, because vLLM's
# JSON-valued flags (--override-generation-config, --hf-overrides, ...) use
# custom argparse actions that in some versions merge dicts rather than replace.
_core_add() {
  local _o
  for _o in ${CORE_OMIT[@]+"${CORE_OMIT[@]}"}; do
    [[ $_o == "$1" ]] && return 0
  done
  VLLM_ARGS+=( "$@" )
  return 0
}

# Assigns VLLM_ARGS with the flags every model family uses. Flags whose VALUE
# differs per model (--override-generation-config, --default-chat-template-kwargs)
# are deliberately NOT here -- they stay in each launcher where they can be read
# next to the model's own notes.
vllm_core_args() {
  : "${MODEL:?}" "${GPU_MEM_UTIL:?}" "${MAX_MODEL_LEN:?}" "${MAX_NUM_SEQS:?}" \
    "${MAX_BATCHED_TOKENS:?}" "${ATTN_BACKEND:?}"
  VLLM_ARGS=( /models/"$MODEL" )
  _core_add --served-model-name cosmo-6000 cosmo-proxy claude-heavy claude-light claude-haiku-4-5-20251001
  _core_add --host 0.0.0.0
  _core_add --port 8799
  _core_add --async-scheduling
  _core_add --dtype "${DTYPE:-auto}"
  _core_add --gpu-memory-utilization "$GPU_MEM_UTIL"
  _core_add --max-model-len "$MAX_MODEL_LEN"
  _core_add --pipeline-parallel-size 1
  _core_add --tensor-parallel-size 1
  _core_add --trust-remote-code
  _core_add --enable-prefix-caching
  _core_add --enable-chunked-prefill
  _core_add --enable-prompt-tokens-details   # usage reports cached_tokens (bench logs prefix-cache hits)
  # Puts a real "metrics" object on every response: time_to_first_token_ms,
  # generation_time_ms, queue_time_ms, mean_itl_ms, tokens_per_second. Off by
  # default, and without it the field is present but null -- which is why
  # llama-proxy was estimating a prefill/decode split from a 20/80 guess and
  # reporting 113k tok/s of prefill on a server that does ~14.5k. Costs nothing;
  # it only requires that --disable-log-stats is not set, and we never set it.
  _core_add --enable-per-request-metrics
  _core_add --max-num-seqs "$MAX_NUM_SEQS"
  _core_add --max-num-batched-tokens "$MAX_BATCHED_TOKENS"
  _core_add --attention-backend "$ATTN_BACKEND"
  return 0
}

# NVFP4 / MoE / CUDA-graph A/B knobs. qwen and laguna only; muse deliberately
# leaves the NVFP4 GEMM to vLLM (see its SPEED notes) and never calls this.
#
# NVFP4 GEMM backend: deprecated in v0.23 ("Use --linear-backend"), the
# VLLM_NVFP4_GEMM_BACKEND env is now gone from vllm/envs.py entirely. v0.27.0 also
# promoted flashinfer_b12x into the LinearBackend choices (vllm/config/kernel.py),
# so the old env-only special case for it is obsolete — everything takes the flag.
nvfp4_backend_args() {
  [[ -n "${MOE_BACKEND:-}" ]] && VLLM_ARGS+=( --moe-backend "$MOE_BACKEND" )
  [[ -n "${NVFP4_BACKEND:-}" ]] && VLLM_ARGS+=( --linear-backend "$NVFP4_BACKEND" )  # vLLM lowercases and maps - to _
  [[ -n "${CUDAGRAPH_MODE:-}" ]] && VLLM_ARGS+=( --compilation-config '{"cudagraph_mode":"'"$CUDAGRAPH_MODE"'"}' )
  [[ -n "${FLASHINFER_AUTOTUNE:-}" ]] && VLLM_ARGS+=( --enable-flashinfer-autotune )
  return 0
}

# Appends EXTRA_ARGS to VLLM_ARGS. Call last -- see rule 1 above.
vllm_extra_args() {
  # shellcheck disable=SC2206
  [[ -n "${EXTRA_ARGS:-}" ]] && VLLM_ARGS+=( ${EXTRA_ARGS} )
  return 0
}

# ---------------------------------------------------------------- launch

# The echo is the record of what actually ran: it is a complete serialization of
# the command, and it is what the refactor test diffs. Keep them adjacent.
vllm_launch() {
  echo docker "${CONTAINER_ARGS[@]}" "$CONTAINER" serve "${VLLM_ARGS[@]}"
  docker "${CONTAINER_ARGS[@]}" "$CONTAINER" serve "${VLLM_ARGS[@]}"
}
