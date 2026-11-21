# shellcheck shell=bash
# Shared guards and helpers for ./build, ./run, ./run-hy4 and ./preload (sourced, not executed).
#
# Sourcing it RUNS the guards: a DOCKER that is an array, empty, or names sudo; a root launcher; a
# docker socket that does not answer. Each one exits 1 at source time, before the caller's first
# docker call can fail with a misleading "image not found".
#
# Written fresh for this project. ../ninfer/lib/common.sh and ../vllmcustom/common/_run-lib.sh were
# the references (read, never sourced: sourcing across projects would couple their plan systems).
#
# Every function ends in an explicit `return 0`: under `set -e`, a function whose last statement is
# a false `[[ … ]] && cmd` returns 1 and takes the caller down with it.

BASALT_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# The container uid/gid. A constant, never `id -u`: a launcher started as root would otherwise hand
# root-owned directories to a container that writes as 1000.
RUN_UID=1000
# shellcheck disable=SC2034  # read by the scripts that source this file (--user)
RUN_GID=1000

die() {
  printf '## %s\n' "$@" >&2
  exit 1
}

# --------------------------------------------------------------- DOCKER

# DOCKER is a string split on spaces, so a wrapper still works (DOCKER=/usr/local/bin/my-docker).
# `@a` gives attribute flags (`a`/`A` for arrays): match them anywhere.
if [[ -n ${DOCKER+set} && ${DOCKER@a} == *[aA]* ]]; then
  die "DOCKER must be a plain string (DOCKER=docker), not an array."
fi
IFS=' ' read -r -a DOCKER <<<"${DOCKER:-docker}"
if (( ${#DOCKER[@]} == 0 )); then
  die "DOCKER is set but holds no words; use DOCKER=docker or unset it."
fi
# sudo as a word OR as a path's last component. The operand is space-padded, so the globs are
# containment matches. /opt/sudo-docker/bin/docker still passes: what follows "sudo" is "-".
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
# accepts the connect and never answers.
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
  source "$BASALT_ROOT/versions.env"
  for k in BASALT_REPO BASALT_SHA CUDA_DEVEL_IMAGE CUDA_RUNTIME_IMAGE IMAGE; do
    [[ -n ${!k:-} ]] || die "versions.env does not set $k."
  done
  return 0
}

# mkdir -p as you, then refuse unless the directory is owned by RUN_UID. dockerd creates a missing
# bind source as root, so every writable mount goes through here first.
owned_dir() {
  local d=${1:?owned_dir needs a path} owner
  mkdir -p -- "$d" 2>/dev/null || die "cannot create $d."
  owner=$(stat -c '%u' -- "$d") || die "cannot stat $d."
  [[ $owner == "$RUN_UID" ]] || die "$d is owned by uid $owner, not $RUN_UID; the container could not write it." \
                                     "  fix: point the variable at a directory you own."
  [[ -w $d ]] || die "$d is not writable by you."
  return 0
}

# Print the nearest existing path at or above $1.
nearest_existing() {
  local d=$1
  while [[ ! -e $d ]]; do d=$(dirname -- "$d"); done
  printf '%s\n' "$d"
  return 0
}

# owned_dir, except that DRY_RUN=1 creates nothing: check the directory if it exists, else that its
# nearest existing ancestor would let us create it.
ensure_dir() {
  local d=${1:?ensure_dir needs a path} a owner
  if [[ ${DRY_RUN:-0} != 1 ]]; then
    owned_dir "$d"
  elif [[ -e $d ]]; then
    [[ -d $d ]] || die "$d exists and is not a directory."
    owner=$(stat -c '%u' -- "$d")
    [[ $owner == "$RUN_UID" && -w $d ]] || die "$d is not a directory uid $RUN_UID can write (owner uid $owner)."
  else
    a=$(nearest_existing "$d")
    [[ -d $a && -w $a ]] || die "cannot create $d: $a is not a writable directory."
    echo "== (DRY_RUN) would create $d"
  fi
  return 0
}

# refuse_busy <port> <name>... — one server at a time (SERIES Standing Constraints): refuse while a
# running container has one of the given names, or anything publishes <port> on the host.
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

# Print a pack's section table, one "name<TAB>offset<TAB>size" line per section (bytes), or nothing
# when the header cannot be read. It never fails its caller, and does not check the magic.
# Layout (vendor/basalt/tools/model_file.py): a 64-byte header with the section count at byte 12, then
# 128-byte entries: a 64-byte NUL-padded name, a u64 offset, a u64 size. Every reader below reads to
# the end of its input: under pipefail, a writer killed by SIGPIPE would fail the pipeline.
pack_sections() {
  local p=${1:?pack_sections needs a path} n i at name off size
  n=$(od -An -tu4 -j12 -N4 -- "$p" 2>/dev/null | tr -d ' \n') || true
  [[ $n =~ ^[0-9]+$ ]] || return 0
  (( n <= 511 )) || return 0                       # (64 KiB - 64) / 128 entries at most
  for (( i = 0; i < n; i++ )); do
    at=$(( 64 + 128 * i ))
    name=$(dd if="$p" bs=64 skip=$(( at / 64 )) count=1 2>/dev/null | tr -d '\0') || true
    off=$(od -An -tu8 -j$(( at + 64 )) -N8 -- "$p" 2>/dev/null | tr -d ' \n') || true
    size=$(od -An -tu8 -j$(( at + 72 )) -N8 -- "$p" 2>/dev/null | tr -d ' \n') || true
    [[ -n $name && $off =~ ^[0-9]+$ && $size =~ ^[0-9]+$ ]] || return 0
    printf '%s\t%s\t%s\n' "$name" "$off" "$size"
  done
  return 0
}

# Print a pack's architecture id — "general.architecture" in its meta section: qwen4exp (Flash-Next),
# hy4flash, … — or nothing when it cannot be read. It never fails its caller.
pack_arch() {
  local p=${1:?pack_arch needs a path} name off size hit
  local re='"general\.architecture": *"([^"]*)"'
  while IFS=$'\t' read -r name off size; do
    [[ $name == meta ]] || continue
    (( size <= 268435456 )) || return 0            # meta is a few MiB of JSON; a wild size is a damaged table
    hit=$(dd if="$p" bs=1M iflag=skip_bytes,count_bytes skip="$off" count="$size" 2>/dev/null \
            | LC_ALL=C grep -a -o '"general\.architecture": *"[^"]*"') || true
    if [[ $hit =~ $re ]]; then printf '%s\n' "${BASH_REMATCH[1]}"; fi
    return 0
  done < <(pack_sections "$p")
  return 0
}

# Print an image's ai.basalt.patches label ("<file>:<hash> …", or "none"); nothing when the image or
# the label is absent (every image built before Plan 12). It never fails its caller.
image_patches() {
  "${DOCKER[@]}" image inspect -f '{{index .Config.Labels "ai.basalt.patches"}}' \
    "${1:?image_patches needs an image}" 2>/dev/null || true
  return 0
}

# Print the newest $IMAGE tag whose ai.basalt.ref label is BASALT_SHA; print nothing if none.
# `docker images` lists newest first. mapfile, not `| head -1`: head closing the pipe would make
# docker exit on SIGPIPE, which pipefail turns into a failure.
image_for_pin() {
  local -a refs
  mapfile -t refs < <("${DOCKER[@]}" images --filter "label=ai.basalt.ref=$BASALT_SHA" \
                        --format '{{.Repository}}:{{.Tag}}' "$IMAGE")
  local r
  for r in "${refs[@]}"; do
    [[ $r == *:'<none>' ]] && continue   # an untagged leftover of an earlier build
    printf '%s\n' "$r"
    break
  done
  return 0
}
