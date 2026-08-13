# shellcheck shell=bash
# Shared bash helpers for the bench-* drivers (sourced, not executed).
# Caller must set: NAME (container name), HOST (host:port), READY_TIMEOUT (seconds).

stop_server() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  # wait for the API port to actually go away before the next launch
  for _ in $(seq 1 30); do
    curl -sf "http://${HOST}/health" >/dev/null 2>&1 || return 0
    sleep 1
  done
  echo "    WARN: ${HOST}/health still answering 30s after removing ${NAME}; next launch may fail" >&2
  return 1
}

wait_ready() {  # returns 0 when /health is up; 1 on timeout or container exit
  local waited=0
  while (( waited < READY_TIMEOUT )); do
    if curl -sf "http://${HOST}/health" >/dev/null 2>&1; then return 0; fi
    if ! docker ps -q -f "name=^${NAME}$" | grep -q .; then
      echo "    container exited during load"; return 1
    fi
    sleep 5; waited=$((waited+5))
  done
  echo "    timed out after ${READY_TIMEOUT}s"; return 1
}

median() { printf '%s\n' "$@" | sort -n | awk '{a[NR]=$1} END{print (NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2}'; }
