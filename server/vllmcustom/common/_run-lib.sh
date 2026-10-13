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

# ------------------------------------------------------------ run identity

# The container's uid/gid, and the cache directory it may write. One constant feeds
# both so the flag and the path can never drift apart. Never derive either from
# `id -u`: the launcher can be started under sudo, and a root-run launcher would then
# hand a root-owned cache tree to a container pinned to 1000 — an EACCES minutes into
# boot naming neither cause.
RUN_UID=${RUN_UID:-1000}

# Only the docker CLI needs privilege on this host: the socket is root-owned and the
# invoking account is in no docker group. Everything a launcher *writes* is written as
# the invoking user, which is the point. A string, split on spaces: DOCKER=docker works
# on a host where the account IS grouped.
# `@a` returns attribute FLAGS, not a type letter: an exported plain string reports `x`,
# an array reports `a`, an exported array `ax`. `== a` — what this shipped as in T1 — is
# therefore wrong twice over: it would reject an exported array and, if widened to mere
# truthiness, it rejects `DOCKER=docker` from the environment, which is the only way
# anyone sets it. Match an a/A anywhere in the flags. Measured on bash 5.2, 2026-10-13.
if [[ -n ${DOCKER+set} && ${DOCKER@a} == *[aA]* ]]; then
  echo "## DOCKER must be a plain string (DOCKER='sudo -n docker'), not an array." >&2
  exit 1
fi
IFS=' ' read -r -a DOCKER <<< "${DOCKER:-sudo -n docker}"

# Refuse a sudo'd launcher HERE, at source time, rather than inside
# docker_core_args: every launcher touches the socket before it (qwen3.8-flash-next/run
# resolves its image by label, require_image inspects it), so a guard placed later is
# reached only after the first docker call has already failed with a misleading
# "image not found". File scope, so $0 is the launcher as the user typed it.
if [[ $EUID == 0 ]]; then
  {
    echo "## run it as yourself — drop the sudo:  $0"
    echo "##   it needs no root: it runs 'sudo -n docker' by itself."
    echo "##   why sudo breaks it: a root launcher creates the cache dir root-owned, and the"
    echo "##   serve runs as uid $RUN_UID, so it dies EACCES on its first cache write."
    echo "##   nothing here wants sudo: ./container/build refuses root too (Plan 01 T5) and sudo's"
    echo "##   for the socket itself, exactly like this launcher does."
  } >&2
  exit 1
fi

# Prove dockerd answers before anything downstream can misreport why it did not. Without
# this, a dead socket or a missing NOPASSWD rule surfaces as `Image … not found. Run
# ./container/build first` (require_image) or `No image built from vLLM $VLLM_MAIN_SHA`
# (qwen3.8-flash-next/run:185) — the second sends you to rebuild 12 GB for a socket problem.
# `version` is the cheapest call that exercises the sudo prefix AND the socket; `info` is
# slower and can fail for reasons that are not "socket dead". `sudo -n` never prompts, but
# a wedged dockerd blocks a unix-socket connect indefinitely, hence the timeout.
if ! _DOCKER_PROBE=$(timeout 15 "${DOCKER[@]}" version 2>&1); then
  {
    echo "## docker is not reachable via '${DOCKER[*]}' — stopping here so nothing below guesses."
    # LAST lines, not first: `docker version` prints its whole Client block to stdout
    # before it fails on the socket, so the first lines are always `Client:`/`Version:`
    # and the diagnosis is the error at the end. tail rather than head|sed because a
    # `| head` here would sit in a live errexit scope: head closes the pipe, sed dies of
    # SIGPIPE, pipefail turns it into 141, errexit kills the script at that line and the
    # hints below never print. Do not "simplify" this back into a pipeline.
    tail -3 <<<"$_DOCKER_PROBE" | sed 's/^/##   it said: /'
    echo "##   reproduce it:  ${DOCKER[*]} version"
    echo "##   'a password is required' -> the NOPASSWD sudoers rule for docker is gone."
    echo "##   'Cannot connect'/'denied' -> dockerd is down, or the socket needs sudo (this host:"
    echo "##   it is root-owned and the account is in no docker group, so 'sudo -n docker' is it)."
    echo "##   nothing at all above -> the probe was killed by its 15 s timeout: dockerd is wedged."
  } >&2
  exit 1
fi

# ---------------------------------------------------------------- image

vllm_image() {
  IMAGE=${IMAGE:-iphands/vllm-blackwell}
  IMAGE_TAG=${IMAGE_TAG:-latest}
  CONTAINER="$IMAGE:$IMAGE_TAG"
  return 0
}

require_image() {
  "${DOCKER[@]}" image inspect "$CONTAINER" >/dev/null 2>&1 || {
    echo "Image $CONTAINER not found. Run ./container/build first."; exit 1; }
  return 0
}

# ---------------------------------------------------------------- docker run

# Assigns CONTAINER_ARGS, CACHE_HOST_DIR and CACHE_WAS_NEW.
# Reads NAME, PORT, MODEL_FOLDER, CACHE_DIR, DETACH, RUN_UID.
docker_core_args() {
  : "${NAME:?}" "${PORT:?}" "${MODEL_FOLDER:?}" "${CACHE_DIR:?}"

  # Created BEFORE the mount, because dockerd auto-creates a missing bind source as
  # root. Plan 01 T1 put a `uid-$RUN_UID` leaf here and T11 took it back out: mounting
  # the cache root whole is what lets a serve read the cache already on disk instead of
  # orphaning 35 GiB of warm Inductor and HF state beside a cold empty directory. The
  # path is the operator's own $CACHE_DIR now, but it is still owned by whoever created
  # it, and the container writes as RUN_UID — so assert ownership, never trust the name.
  # Owner only, never group: write access comes from the uid, and demanding gid == RUN_UID
  # would reject a perfectly usable directory on any host where uid 1000's primary group
  # is not 1000 — with advice you cannot follow.
  CACHE_HOST_DIR="$CACHE_DIR"
  CACHE_WAS_NEW=0
  [[ -w $CACHE_DIR ]] || {
    echo "## $CACHE_DIR is not writable by uid $EUID — it does not exist, or is not yours." >&2
    echo "##   fix: create it, or point CACHE_DIR= at a directory you own." >&2
    return 1
  }
  # mkdir, not `install -d -m`: install chmods a directory that ALREADY EXISTS, so a
  # cache tree deliberately locked to 0700 would be widened back to 0750 on every boot.
  # mkdir sets the mode only when it creates. Its error is swallowed, so the `-d` test
  # below is what tells "already there" apart from "cannot be made".
  if mkdir -m 0750 "$CACHE_HOST_DIR" 2>/dev/null; then
    CACHE_WAS_NEW=1
  elif [[ ! -d $CACHE_HOST_DIR ]]; then
    echo "## cannot create $CACHE_HOST_DIR, and it is not a directory either." >&2
    echo "##   fix: remove that path, or point CACHE_DIR= somewhere else." >&2
    return 1
  fi
  CACHE_UID=$(stat -c '%u' "$CACHE_HOST_DIR" 2>/dev/null) \
    || { echo "## cannot stat $CACHE_HOST_DIR" >&2; return 1; }
  if [[ $CACHE_UID != "$RUN_UID" ]]; then
    echo "## $CACHE_HOST_DIR is owned by uid $CACHE_UID; the container writes as uid $RUN_UID." >&2
    echo "##   fix: chown $RUN_UID $CACHE_HOST_DIR, or point CACHE_DIR= at a directory you own." >&2
    return 1
  fi
  # Mounting the root means the serve writes into whatever is ALREADY in $CACHE_DIR, and
  # the check above cannot see that: it stats the leaf, and the leaf is yours. The pre-split
  # tree is root-owned mode 0755 — readable, never writable — so the boot prints the normal
  # warm-start line and then dies EACCES minutes later on its first write into huggingface/
  # or the FlashInfer JIT dir. Hence six stats on the directories a serve actually opens,
  # not a 244k walk on the launcher's hot path, and hence a refusal rather than a warning:
  # a warning scrolls away above the 20-60 min silence that follows, and the damage it
  # prevents is PARTIAL — writes into new paths succeed while writes into the root-owned
  # ones fail, leaving a half-warm tree worse than the cold one. CACHE_PROBE_DIRS= disables
  # this; the only launcher that sets it is run-legacy, which serves as root by design.
  local -a _probe _ro=()
  local _d
  read -r -a _probe <<<"${CACHE_PROBE_DIRS-huggingface flashinfer vllm torch ccache uv}"
  for _d in "${_probe[@]}"; do
    if [[ -e $CACHE_HOST_DIR/$_d && ! -w $CACHE_HOST_DIR/$_d ]]; then _ro+=("$_d"); fi
  done
  if (( ${#_ro[@]} )); then
    echo "## REFUSED: $CACHE_HOST_DIR holds directories the serve cannot write: ${_ro[*]}" >&2
    echo "##   That is the pre-split tree, still root-owned from the containers that ran before the uid pin." >&2
    echo "##   Warm it once instead of re-earning it — nothing moves and nothing re-downloads:" >&2
    echo "##     sudo chown -R $RUN_UID $CACHE_HOST_DIR" >&2
    echo "##   This tree measured 205953 files / 35.05 GiB on 2026-10-13: minutes of NFS metadata, not bytes." >&2
    echo "##   Let it finish before retrying. Done when this prints 0:" >&2
    echo "##     find $CACHE_DIR -user root | wc -l" >&2
    return 1
  fi

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
    # Pinned identity. The image declares no USER directive yet (Plan 01 T3 adds it),
    # so until that rebuild this flag is the ONLY assertion that the serve is
    # unprivileged — a hand-run `docker run` or `docker exec` is still root.
    --user "$RUN_UID:$RUN_UID"
    -e HOME=/home/ubuntu                           # set, not inferred from /etc/passwd
    --gpus device=0                                # RTX PRO 6000 only — never the 4060
    -e CUDA_DEVICE_ORDER=PCI_BUS_ID
    -e VLLM_WORKER_MULTIPROC_METHOD=spawn
    -e SAFETENSORS_FAST_GPU=1
    -e VLLM_LOG_STATS_INTERVAL=1                   # log tok/s + acceptance every 1s (tuning)
    -p "0.0.0.0:${PORT}:8799"
    -v "$MODEL_FOLDER":/models:ro
    # Was /root/.cache, which is 0700 and unreadable to uid 1000. The pre-split tree
    # above this path is root-owned and stays that way by owner choice — see README
    # § Caching.
    -v "$CACHE_HOST_DIR":/home/ubuntu/.cache:rw
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
  # ATTN_BACKEND may be EMPTY (set, but blank): that means "let vLLM pick", and
  # the flag is simply not emitted. qwen3.8-flash-next/run needs that because
  # its sparse-attention indexer constrains the backend choice and the model
  # card's numbers were all taken with the image's auto-selection.
  : "${MODEL:?}" "${GPU_MEM_UTIL:?}" "${MAX_MODEL_LEN:?}" "${MAX_NUM_SEQS:?}" \
    "${MAX_BATCHED_TOKENS:?}" "${ATTN_BACKEND?ATTN_BACKEND must be set (empty = vLLM default)}"
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
  [[ -n "$ATTN_BACKEND" ]] && _core_add --attention-backend "$ATTN_BACKEND"
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

# The echo is the record of what actually ran, privilege prefix included. It is a
# serialization, not a quoting-preserving one: VLLM_ARGS carries JSON with spaces
# (--override-generation-config), so re-pasting the line re-splits that one argument.
# There is no test in this repo that diffs it — the DRY_RUN short-circuit + golden file
# that would do so is still only proposed (context/pitfalls.md). Anything greping this
# line must anchor on '^([^ ]+ )*docker ' now, not '^docker '.
vllm_launch() {
  # Printed last, immediately before the long silence it explains, so it is what is
  # still on screen when the boot looks stuck. docker_core_args set the flag.
  if [[ ${CACHE_WAS_NEW:-0} == 1 ]]; then
    echo "## cache: $CACHE_HOST_DIR  (NEW — cold start as uid $RUN_UID)"
    echo "##        est. 20-60 min to 'startup complete' (unmeasured): HF metadata + FlashInfer JIT."
    echo "##        kernel names scrolling past means it is alive; Ctrl-C restarts the JIT from zero."
    echo "##        first serve on this cache root — README § Caching explains what lands in it."
  else
    echo "## cache: $CACHE_HOST_DIR"
  fi
  echo "${DOCKER[@]}" "${CONTAINER_ARGS[@]}" "$CONTAINER" serve "${VLLM_ARGS[@]}"
  "${DOCKER[@]}" "${CONTAINER_ARGS[@]}" "$CONTAINER" serve "${VLLM_ARGS[@]}"
}
