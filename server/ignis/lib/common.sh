# shellcheck shell=bash
# Shared guards and helpers for the launchers and scripts/* (sourced, not executed).
#
# Sourcing it RUNS the guards: a DOCKER that is an array, empty, or names sudo; a root launcher;
# a docker socket that does not answer. Each one exits 1 at source time, before the caller's first
# docker call can fail with a misleading message.
#
# Written fresh for this project. ../ninfer/lib/common.sh and ../vllmcustom/common/_run-lib.sh
# were the references (read, never sourced: sourcing across projects would couple three plan
# systems).
#
# Every function ends in an explicit `return 0`: under `set -e`, a function whose last statement
# is a false `[[ … ]] && cmd` returns 1 and takes the caller down with it.

IGNIS_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# The container uid/gid. A constant, never `id -u`: a launcher started as root would otherwise
# hand root-owned directories to a container that writes as 1000. Not the image's own uid 10001:
# upstream's fix for a cache that uid can write is `sudo chown 10001`, and nothing here uses sudo.
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
  source "$IGNIS_ROOT/versions.env"
  for k in IGNIS_IMAGE IGNIS_VERSION IGNIS_SRC_REF \
           FN_REPO FN_REV FN_ARTIFACT FN_MTP \
           Q27_REPO Q27_REV Q27_ARTIFACT Q27A_REPO Q27A_REV Q27A_ARTIFACT; do
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

# refuse_busy <port> <name>... — one server at a time (SERIES Standing Constraints): refuse while a
# running container has one of the given names, or anything publishes <port> on the host. vLLM
# (container `vllm`), NInfer and Ignis each want the whole RTX PRO 6000.
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

# check_gpu <index> — the launchers name --profile rtxpro6000, so the card they hand the
# container must be the RTX PRO 6000; host GPU 1 is an RTX 4060 that is never used here.
check_gpu() {
  local i=${1:?check_gpu needs a GPU index} name
  name=$(nvidia-smi -i "$i" --query-gpu=name --format=csv,noheader 2>&1) \
    || die "nvidia-smi cannot read GPU $i: $name"
  [[ $name == *'RTX PRO 6000'* ]] \
    || die "GPU $i is '$name', not an RTX PRO 6000; the launchers pin --profile rtxpro6000." \
           "  fix: GPU=<index of the RTX PRO 6000> (nvidia-smi -L)"
  return 0
}

# hf_hint <dir> <repo> <rev> <file>... — the README's download command for those files, printed
# for a refusal. Files named positionally: `hf download` and `huggingface-cli download` both take
# them, so the hint does not depend on either tool's --include syntax.
hf_hint() {
  local dir=${1:?} repo=${2:?} rev=${3:?}
  shift 3
  printf '  hf download %s --revision %s --local-dir %s \\\n' "$repo" "$rev" "$dir"
  printf '    %s\n' "$*"
  return 0
}

# require_files <dir> <hint> <file>... — refuse unless every file is in <dir>. <hint> is the
# download command to print (hf_hint's output); README.md § Download the weights has the rest.
require_files() {
  local dir=${1:?require_files needs a directory} hint=${2:?require_files needs a hint} f
  shift 2
  local -a missing=()
  for f in "$@"; do [[ -f $dir/$f ]] || missing+=("$f"); done
  if (( ${#missing[@]} > 0 )); then
    {
      printf '## missing in %s:\n' "$dir"
      printf '##     %s\n' "${missing[@]}"
      echo '##   download them first (README.md § Download the weights), e.g.:'
      while IFS= read -r f; do printf '## %s\n' "$f"; done <<<"$hint"
    } >&2
    exit 1
  fi
  return 0
}
