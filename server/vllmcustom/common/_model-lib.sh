# shellcheck shell=bash
# Model spec -> (MODEL, MODEL_ROOT, MODEL_FOLDER). Sourced, not executed.
#
# One resolver for everything that has to turn "the model the user named" into a
# bind mount plus a container-side path. Used by qwen/run, laguna/run, muse/run.
#
# Caller must set: nothing.  Optional: MODEL_ROOT (default <repo>/models/vllm).

# Repo root, resolved LOGICALLY, and from BASH_SOURCE rather than $0 ($0 is the
# launcher, which lives one directory deeper).
#
# `cd && pwd` here is load-bearing and must not be "cleaned up" to realpath or
# `pwd -P`: models/ is a symlink to /mnt/noir/scratch/ai/llm/models, so the
# logical path is .../vllmcustom/models/vllm and the physical one is
# /mnt/noir/scratch/ai/llm/models/vllm. resolve_model compares this prefix
# against a `cd && pwd` of the caller's argument, which is also logical. Mix the
# two and the prefix test below never matches: every invocation silently falls
# into the dirname branch, /models narrows to the model's own parent, and a
# sibling drafter (meta-models/...) vanishes from the container.
_MODELLIB_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# resolve_model <spec>
#   <spec> is either a directory (relative or absolute) or a name relative to
#   MODEL_ROOT, which may contain slashes ("meta-models/Muse-Glimmer-30B").
#
# Sets MODEL        path under MODEL_ROOT; becomes /models/$MODEL in the container
#      MODEL_ROOT   absolute; rewritten to the model's parent if it lives outside
#      MODEL_FOLDER what gets bind-mounted at /models (always == MODEL_ROOT)
#
# Mounting the whole root rather than just the model's directory is deliberate:
# it is what lets a speculative drafter stored beside the target model be visible
# under the single /models mount.
resolve_model() {
  local _arg=$1 _abs
  MODEL_ROOT=${MODEL_ROOT:-$(cd "$_MODELLIB_ROOT/models/vllm" && pwd)}
  if [[ -d "$_arg" ]]; then
    # A full path, as ./scripts/muse-spec-* pass. Keep it relative to the root
    # when it lives under it; otherwise fall back to mounting its own parent.
    _abs=$(cd "$_arg" && pwd)
    case "$_abs/" in
      "$MODEL_ROOT"/*) MODEL=${_abs#"$MODEL_ROOT"/} ;;
      *) MODEL_ROOT=$(dirname "$_abs"); MODEL=$(basename "$_abs") ;;
    esac
  else
    MODEL=$_arg
  fi
  [[ -d "$MODEL_ROOT/$MODEL" ]] || {
    echo "no model at $MODEL_ROOT/$MODEL"
    echo "pass a path, or set MODEL_ROOT=. Available:"
    find "$MODEL_ROOT" -maxdepth 2 -name config.json -printf '  %h\n' 2>/dev/null | sed "s|$MODEL_ROOT/||"
    exit 1; }
  # shellcheck disable=SC2034  # read by docker_core_args and by muse/run
  MODEL_FOLDER=$MODEL_ROOT
  return 0
}
