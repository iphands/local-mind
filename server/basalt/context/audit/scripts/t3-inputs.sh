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
