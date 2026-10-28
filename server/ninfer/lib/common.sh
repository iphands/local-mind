# shellcheck shell=bash
# Shared guards and helpers for ./build, ./run and scripts/* (sourced, not executed).
#
# Sourcing it RUNS the guards: a DOCKER that is an array, empty, or names sudo; a root
# launcher; a docker socket that does not answer. Each one exits 1 at source time, before the
# caller's first docker call can fail with a misleading "image not found".
#
# Written fresh for this project. ../vllmcustom/common/_run-lib.sh was the reference (read,
# never sourced: its docker args are vLLM-shaped, and sourcing across projects would couple two
# plan systems).
#
# Every function ends in an explicit `return 0`: under `set -e`, a function whose last
# statement is a false `[[ … ]] && cmd` returns 1 and takes the caller down with it.

NINFER_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# The container uid/gid. A constant, never `id -u`: a launcher started as root would otherwise
# hand root-owned directories to a container that writes as 1000.
RUN_UID=1000
# shellcheck disable=SC2034  # read by the scripts that source this file (--user)
RUN_GID=1000

die() {
  printf '## %s\n' "$@" >&2
  exit 1
}

# --------------------------------------------------------------- DOCKER

# DOCKER is a string split on spaces, so a wrapper still works (DOCKER=/usr/local/bin/my-docker).
# `@a` gives attribute flags (`x` for an exported string, `a`/`A` for arrays): match a/A anywhere.
if [[ -n ${DOCKER+set} && ${DOCKER@a} == *[aA]* ]]; then
  die "DOCKER must be a plain string (DOCKER=docker), not an array."
fi
IFS=' ' read -r -a DOCKER <<<"${DOCKER:-docker}"
if (( ${#DOCKER[@]} == 0 )); then
  die "DOCKER is set but holds no words; use DOCKER=docker or unset it."
fi
# sudo as a word OR as a path's last component. The operand is space-padded, so the globs are
# containment matches: an end-anchored `*/sudo` could never match the padded string.
# /opt/sudo-docker/bin/docker still passes — what follows its "sudo" is "-", not a space.
if [[ ' '"${DOCKER[*]}"' ' == *' sudo '* || ' '"${DOCKER[*]}"' ' == *'/sudo '* ]]; then
  die "DOCKER names 'sudo'. Nothing in this project needs a privilege helper." \
      "  fix:      unset DOCKER   (the default is plain 'docker')" \
      "  wrapper?  name it directly: DOCKER=/usr/local/bin/my-docker"
fi

# ----------------------------------------------------------------- root

if [[ $EUID == 0 ]]; then
  die "run it as yourself: $0" \
      "  nothing here needs root, and a root run creates root-owned directories that the" \
      "  container (uid $RUN_UID) then cannot write."
fi

# --------------------------------------------------------- socket probe

# `version` exercises both the DOCKER command and the socket. The timeout stays: a wedged dockerd
# accepts the connect and never answers. LAST lines on failure: the client block prints first.
_probe_rc=0
_probe_out=$(timeout 15 "${DOCKER[@]}" version 2>&1) || _probe_rc=$?
if (( _probe_rc != 0 )); then
  {
    echo "## docker is not reachable via '${DOCKER[*]}' (exit $_probe_rc):"
    tail -3 <<<"$_probe_out" | sed 's/^/##   /'
    if (( _probe_rc == 124 )); then
      echo "##   killed at the 15 s limit — dockerd is wedged, not down."
    elif (( _probe_rc == 127 )); then
      echo "##   '${DOCKER[0]}' is not a runnable command."
    else
      echo "##   permission denied? read who may open the socket: stat -c '%U:%G %a' /var/run/docker.sock"
    fi
  } >&2
  exit 1
fi
unset _probe_rc _probe_out

# ------------------------------------------------------------- helpers

# Source versions.env and require every pin. Not exported: callers pass what they need.
load_pins() {
  local k
  # shellcheck source-path=SCRIPTDIR source=../versions.env
  source "$NINFER_ROOT/versions.env"
  for k in NINFER_REPO NINFER_SHA CUDA_DEVEL_IMAGE CUDA_RUNTIME_IMAGE MOBY_SECCOMP_REF IMAGE \
           HF_BASE_REPO HF_BASE_REV; do
    [[ -n ${!k:-} ]] || die "versions.env does not set $k."
  done
  return 0
}

# mkdir -p as you, then refuse unless the directory is owned by RUN_UID. dockerd creates a
# missing bind source as root, so every writable mount goes through here first.
owned_dir() {
  local d=${1:?owned_dir needs a path} owner
  mkdir -p -- "$d" 2>/dev/null || die "cannot create $d."
  owner=$(stat -c '%u' -- "$d") || die "cannot stat $d."
  [[ $owner == "$RUN_UID" ]] || die "$d is owned by uid $owner, not $RUN_UID; the container could not write it." \
                                     "  fix: point the variable at a directory you own."
  [[ -w $d ]] || die "$d is not writable by you."
  return 0
}

# refuse_busy <port> <name>... — one server at a time (SERIES Standing Constraints): refuse while a
# running container has one of the given names, or anything publishes <port> on the host. vLLM
# (container `vllm`) and NInfer both need the GPU, and NInfer pins ~63 GiB of host RAM besides.
refuse_busy() {
  local port=${1:?refuse_busy needs a port} n
  shift
  local -a hits
  mapfile -t hits < <(
    {
      "${DOCKER[@]}" ps --format '{{.Names}}' --filter "publish=$port"
      for n in "$@"; do "${DOCKER[@]}" ps --format '{{.Names}}' --filter "name=^${n}\$"; done
    } | sort -u
  )
  if (( ${#hits[@]} > 0 )); then
    die "busy: running container(s) ${hits[*]} hold the GPU or :$port — one server at a time." \
        "  stop it first:  docker stop ${hits[*]}"
  fi
  return 0
}

# Print the newest $IMAGE tag whose ai.ninfer.ref label is NINFER_SHA; print nothing if none.
# `docker images` lists newest first. mapfile, not `| head -1`: head closing the pipe would make
# docker exit on SIGPIPE, which pipefail turns into a failure.
image_for_pin() {
  local -a refs
  mapfile -t refs < <("${DOCKER[@]}" images --filter "label=ai.ninfer.ref=$NINFER_SHA" \
                        --format '{{.Repository}}:{{.Tag}}' "$IMAGE")
  local r
  for r in "${refs[@]}"; do
    [[ $r == *:'<none>' ]] && continue   # an untagged leftover of an earlier build
    printf '%s\n' "$r"
    break
  done
  return 0
}
