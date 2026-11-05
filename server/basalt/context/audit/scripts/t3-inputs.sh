#!/usr/bin/env bash
# t3-inputs.sh — derive the inputs t3-census.sh refuses to run without.
# Target dc047a7 / tree 3f7b69ae.
#
# Usage:  scripts/t3-inputs.sh
#
# Everything here is a PROJECTION of context/audit/manifest/*.tsv, which is the single owner of
# path/blob/sha256/LOC/tier. t3-census.sh reads these three files instead of the manifest so the
# census's `assigned-TN` column can only ever be a projection of tier — one fact, one owner — and
# so a missing input fails loudly instead of silently re-routing every row to a section default.
#
# Writes (scratch, regenerable, never committed):
#   /tmp/opencode/basalt-audit/t3/manifest-loc.tsv   path <TAB> LOC
#   /tmp/opencode/basalt-audit/t3/tier-map.tsv       path <TAB> TN   (empty TN = not yet owned)
#   /tmp/opencode/basalt-audit/t3/tierA-files.tsv    path <TAB> tier (Tier A + B-vendored only)
set -eu
export LC_ALL=C
MAN="$(cd "$(dirname "$0")" && pwd)/../manifest"
OUT=/tmp/opencode/basalt-audit/t3
mkdir -p "$OUT"
[ -d "$MAN" ] || { echo "no manifest at $MAN" >&2; exit 2; }

# --- totals of the INPUT SET, asserted before anything is written -------------------------------
# Every projection below is built from `"$MAN"/*.tsv`, so a TSV that stops matching the glob (renamed,
# a typo, a stray `.tsv.tmp`) contributes NOTHING and says nothing: the row counts just come out
# smaller. That is the T1 `.github.tsv` failure recorded in this plan's notes — 860 rows read as a
# pass. `MANIFEST_ROWS_EQ_862` in t3-census.sh cannot backstop it: that assertion sits inside
# `if [ "$TAG" = real ]` (t3-census.sh:1435), so on a canary tag nothing downstream catches a short
# manifest at all and the census reads the short list into its assigned-TN column.
# 12 and 862 are LOCALLY OWNED here — this script is the producer of these three files and the party
# that can name their totals — so they are written out, not cited (RULES.md § Rule E item 7: a
# sentinel replaced by a pointer can no longer fail). Bump them only with the manifest, in the same
# commit, and re-derive MANIFESTS_TSV/862 from `git ls-files context/audit/manifest/*.tsv` and
# `cat | awk 'END{print NR}'` rather than trusting this line.
# Guards run BEFORE the writes on purpose: dying after the awk block would leave the PREVIOUS run's
# three files in place, and the census would read stale-but-present inputs as fresh ones.
TSVS=$(find "$MAN" -maxdepth 1 -name '*.tsv' -type f | wc -l)
ROWS=$(cat "$MAN"/*.tsv | awk 'END{print NR+0}')
echo "INPUT_TSVS=$TSVS INPUT_ROWS=$ROWS"
[ "$TSVS" -eq 12 ] || { echo "INPUT_TSVS=$TSVS — expected 12 manifest TSVs; the glob is not matching the set" >&2; exit 4; }
[ "$ROWS" -eq 862 ] || { echo "INPUT_ROWS=$ROWS — expected 862 manifest rows; a TSV lost rows" >&2; exit 4; }

awk -F'\t' '{print $1"\t"$4}' "$MAN"/*.tsv | LC_ALL=C sort > "$OUT/manifest-loc.tsv"
awk -F'\t' '{tn=""; if ($5 ~ /@T[0-9]/) { sub(/^.*@/, "", $5); tn=$5 } print $1"\t"tn}' "$MAN"/*.tsv \
  | LC_ALL=C sort > "$OUT/tier-map.tsv"
awk -F'\t' '$5 ~ /^A-planned@|^B-vendored@/ {print $1"\t"$5}' "$MAN"/*.tsv | LC_ALL=C sort > "$OUT/tierA-files.tsv"

DUP=$(cut -f1 "$OUT/manifest-loc.tsv" | LC_ALL=C sort | uniq -d | wc -l)
echo "MANIFEST_LOC=$(wc -l < "$OUT/manifest-loc.tsv") TIERMAP=$(wc -l < "$OUT/tier-map.tsv") TIERMAP_WITH_TN=$(awk -F'\t' '$2!=""' "$OUT/tier-map.tsv" | wc -l) TIERA=$(wc -l < "$OUT/tierA-files.tsv")"
echo "DUPLICATE_PATHS_ACROSS_TSVS=$DUP (expect 0: a duplicate would double-count every LOC sum)"
echo "LOC_TOTAL_FROM_MANIFEST=$(awk -F'\t' '{s+=$2} END{print s+0}' "$OUT/manifest-loc.tsv")"
echo "GENERATOR_SHA256=$(sha256sum "$0" | cut -d' ' -f1)  INPUTS_DIR=$OUT"
[ "$DUP" -eq 0 ] || { echo "duplicate paths in the manifest — fix the manifest before censusing" >&2; exit 3; }
