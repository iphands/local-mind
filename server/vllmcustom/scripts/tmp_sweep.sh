#!/usr/bin/env bash
# The ae525e9 sweeps, back to back, on run-fp4local (launcher defaults, incl. TRITON_DEFAULT_FP_FUSION=1):
#   1. concurrency sweep, K=3, 1 and 4 streams, 3 rounds   -> compare flashnext-conc-20261019-042515
#   2. VLLM_MARLIN_USE_ATOMIC_ADD off vs on, K=3, 4 runs    -> summary.md says if the on arm got the env
# Each script boots its own server and leaves nothing running. Neither will stop a server
# that is already up (it refuses instead); run with REPLACE=1 to allow that.
set -uo pipefail
cd "$(dirname "$0")" || exit 1

rc_conc=0 rc_marlin=0

echo "## [1/2] flashnext-conc-sweep  KS=3 CONC=1,4 ROUNDS=3"
KS=3 CONC=1,4 ROUNDS=3 ./scripts/flashnext-conc-sweep || rc_conc=$?

echo "## [2/2] flashnext-marlin-atomic-ab  RUNS=${RUNS:-4}"
./scripts/flashnext-marlin-atomic-ab || rc_marlin=$?

echo
echo "## conc sweep exit $rc_conc, marlin A/B exit $rc_marlin (3 = the on arm never got the env)"
echo "## results: newest bench/bench-logs/flashnext-conc-*/ and flashnext-marlin-atomic-ab-*/ (summary.md in each)"
exit $(( rc_conc || rc_marlin ))
