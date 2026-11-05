#!/usr/bin/env bash
# t2-census.sh — the author census behind context/audit/wave0/t2-author-census.tsv.
#
# Usage:
#   scripts/t2-census.sh <rows.tsv> <out.tsv> <err.log> <expect_rows> <repo> <pin>
#   rows.tsv  headerless: path <TAB> loc <TAB> tier   (build it from the manifest, see § Build input
#             in ../../plans/02_basalt_vendor_audit.md T2 Verify; expect 87 at the dc047a7 pin)
#   repo      path to the vendor clone      pin  the commit to census AT (never HEAD)
#
# WHAT IT PROVES. census_status=OK means: blame produced one attributed line per line of the file,
#   i.e. lines_blame == loc + no_trailing_nl, and no output field came back empty. That is LINE
#   ACCOUNTING AND ROW SURVIVAL ONLY — it cannot detect a wrong author. See
#   ../../context/audit/wave0/t2-provenance.txt § 5 before quoting any number this writes.
#   COLUMN NAME: `census_status` (OK | BINARY_SKIPPED | REAL_DRIFT | BLAME_FAILED) is deliberately NOT
#   called `status`: `manifest/*.tsv` column 8 is `status` (pending | read@T<n>) and the two
#   vocabularies are disjoint. Never join on the word.
#
# BLAME FLAGS: deliberately NONE. Plain `git blame --line-porcelain <pin> -- <path>`: no -w (it
#   changes the question from "whose bytes" to "whose logic"), no -M/-C, no --ignore-revs-file.
#   Confirm no blame-affecting config exists before trusting a rerun:
#     git -C <repo> config --list --show-origin | grep -iE 'blame|ignoreRevs'   # expect: no output
#
# TWO TRAPS THIS SCRIPT EXISTS TO AVOID (both produced green-but-wrong results on 2026-11-05):
#   1. `git rev-list --count --follow` is NOT a thing — rev-list has no --follow. With stderr
#      suppressed it yields an empty column while every row still prints. Hence `git log --follow`.
#   2. Suppressing stderr at all. Every git call here appends stderr to $ERRLOG and the run fails
#      if that log is non-empty.
set -uo pipefail

ROWS=$1; OUT=$2; ERRLOG=$3; EXPECT_ROWS=$4; REPO=$5; PIN=$6
BINLIST=${BINLIST:-context/audit/wave0/binary-paths.txt}

if [ ! -f "$ROWS" ] || [ ! -f "$BINLIST" ]; then
  echo "MISSING_INPUT rows=$ROWS binlist=$BINLIST (run from server/basalt)"; exit 8
fi
# Gate 0 — assert the input size INSIDE the producer. Without this an empty input produced
# "VALIDATION=PASS" and exit 0 (control C2 in wave0/t2-provenance.txt).
GOT_ROWS=$(grep -c . "$ROWS")
if [ "$GOT_ROWS" != "$EXPECT_ROWS" ]; then
  echo "ROWS_IN=$GOT_ROWS EXPECT_ROWS=$EXPECT_ROWS — INPUT WRONG, refusing to run"; exit 7
fi

: > "$ERRLOG"
is_binary() { grep -qxF "$1" "$BINLIST"; }

{
printf 'path\tloc\ttier\tcommits_total\tauthors_log\tfirst_commit\tlast_commit\ttop_author\tlines_top_author\tauthors_blame\tlines_blame\tno_trailing_nl\tcensus_status\n'
while IFS=$'\t' read -r path loc tier; do
  if is_binary "$path"; then
    printf '%s\t%s\t%s\t\t\t\t\t\t\t\t\t\tBINARY_SKIPPED\n' "$path" "$loc" "$tier"; continue
  fi
  commits=$(git -C "$REPO" log --follow --format=%H "$PIN" -- "$path" 2>>"$ERRLOG" | wc -l)
  first=$(git -C "$REPO" log --follow --reverse --format=%ad --date=short "$PIN" -- "$path" 2>>"$ERRLOG" | head -1)
  last=$(git -C "$REPO" log --follow --format=%ad --date=short "$PIN" -- "$path" 2>>"$ERRLOG" | head -1)
  lauth=$(git -C "$REPO" log --follow --format=%an "$PIN" -- "$path" 2>>"$ERRLOG" | sort -u | paste -sd'; ' -)
  # NOTE: a 0-byte file makes `tail -c 1` empty and would derive nonl=1 spuriously. None exist at
  # this pin; if a re-pin adds one, special-case `git cat-file -s` == 0 here.
  lastchar=$(git -C "$REPO" cat-file -p "$PIN:$path" 2>>"$ERRLOG" | tail -c 1)
  nonl=0; [ "$lastchar" != "" ] && nonl=1
  expected=$((loc + nonl))

  tmp=$(mktemp); tmph="${tmp}.h"
  if git -C "$REPO" blame --line-porcelain "$PIN" -- "$path" > "$tmp" 2>>"$ERRLOG"; then
    awk '/^author /{sub(/^author /,""); print}' "$tmp" | LC_ALL=C sort | uniq -c | LC_ALL=C sort -rn > "$tmph"
    total=$(awk '{s+=$1} END{print s+0}' "$tmph")
    topc=$(head -1 "$tmph" | awk '{print $1}')
    topa=$(head -1 "$tmph" | sed -E 's/^ *[0-9]+ //')
    na=$(wc -l < "$tmph")
    status=OK
    [ "$total" -ne "$expected" ] && status="REAL_DRIFT lines_blame=$total expected=$expected"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "$path" "$loc" "$tier" "$commits" "$lauth" "$first" "$last" "${topa:-none}" "${topc:-0}" "$na" "$total" "$nonl" "$status"
  else
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t\t\t\t\t%s\tBLAME_FAILED\n' \
      "$path" "$loc" "$tier" "$commits" "$lauth" "$first" "$last" "$nonl"
  fi
  rm -f "$tmp" "$tmph"
done < "$ROWS"
} > "$OUT"

# Validation. Field-count in, field-count out, no empty field on a non-binary row, every non-binary
# row OK, and stderr silent. A PASS here still only certifies accounting — see the header.
awk -F'\t' -v rows="$EXPECT_ROWS" '
  NR==1 {next}
  $13=="BINARY_SKIPPED" {b++; next}
  { r++; for (i=1;i<=13;i++) if ($i=="") { print "EMPTY_FIELD row="r" col="i" path="$1; ef++ }
    if ($13!="OK") { print "NOT_OK " $1 " " $13; st++ } }
  END {
    printf "ROWS_IN=%d ROWS_OUT=%d BINARY_SKIPPED=%d\n", rows+0, r+b, b
    printf "EMPTY_FIELDS=%d NOT_OK=%d\n", ef+0, st+0
    if (r+b != rows+0) { print "VALIDATION=FAIL_ROW_COUNT"; exit 5 }
    if (ef+st != 0)    { print "VALIDATION=FAIL_FIELDS"; exit 6 }
    print "VALIDATION=FIELDS"
  }' "$OUT" || exit $?

ERR_BYTES=$(wc -c < "$ERRLOG")
# NOTE: on a genuinely broken run VALIDATION=FAIL_FIELDS exits 6 above and this line is never reached,
# so the branch below is only observable on an otherwise-clean run whose git calls still wrote to
# stderr (a warning). It was proven to fire by injecting one line into $ERRLOG here — control C8,
# context/audit/wave0/t2-gate-and-checks.txt § 5 (ERR_BYTES=69 → exit 9). Keep that control if you
# reorder the validation: an assertion nobody has seen fail is decoration.
echo "ERR_BYTES=$ERR_BYTES"
if [ "$ERR_BYTES" -ne 0 ]; then
  echo "VALIDATION=FAIL_STDERR (first 3 lines:)"; head -3 "$ERRLOG"; exit 9
fi
echo "VALIDATION=PASS — certifies row survival and line accounting ONLY; not author identity, not blame-boundary fidelity."
