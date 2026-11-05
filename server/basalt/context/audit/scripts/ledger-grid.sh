#!/usr/bin/env bash
# Target dc047a7 / tree 3f7b69ae — if that is not the tree you are looking at, you are in the wrong audit.
# context/audit/scripts/ledger-grid.sh
# Regenerate the coverage figures quoted by coverage_ledger.md and
# manifest/index.md from the 12 manifest TSVs.
#
# Reports the MANIFEST, never the worktree — proving the bytes still match the
# audited pin is frozen-prefix.sha256's job (manifest/index.md § Checks). Writes
# nothing: stdout is paste material, and it lands in wave0/ before any .md quotes it.
#
# The literals below are PINNED, not measured: 862 rows and the f4 sums come from
# frozen columns 1–4 at the audited tree, the tier split from T1's seeding. A
# literal moving is tampering or a deliberate pin bump — never bookkeeping. Editing
# a number here is exactly as loud as editing a TSV, and for the same reason.
set -euo pipefail
shopt -s nullglob
cd "$(dirname "$0")/../manifest" || exit 1

M=(*.tsv)
fail=0
# Rule 6 stamp, emitted first so the wave0 file this stdout becomes opens with it.
echo "# Target dc047a7 / tree 3f7b69ae — if that is not the tree you are looking at, you are in the wrong audit."
echo "# generated $(date +%F) by context/audit/scripts/ledger-grid.sh from context/audit/manifest/ — do not hand-edit its output"
check() { # name want got
  if [ "$2" = "$3" ]; then printf '%-22s EXPECTED=%s ACTUAL=%s OK\n' "$1" "$2" "$3"
  else printf '%-22s EXPECTED=%s ACTUAL=%s FAIL\n' "$1" "$2" "$3"; fail=1; fi
}

# Glob and `find` cross-prove each other: `-type f` + `-name` see dotfiles and links
# that `*.tsv` cannot, so the pair disagrees on exactly the hidden/substituted file.
# A dot-prefixed TSV is not hypothetical — T1 wrote `.github.tsv` and every count
# in this audit read 859.
on_disk=$(find . -maxdepth 1 -name '*.tsv' -type f | wc -l)
check TSV_FILES 12 "${#M[@]}"
check TSV_FILES_FIND 12 "$on_disk"
if [ "$fail" -ne 0 ]; then
  echo "a dot-prefixed, linked or 13th TSV silently changes every count below" >&2
  exit 3
fi

rows=$(LC_ALL=C awk 'END{print NR+0}' "${M[@]}")
loc=$(LC_ALL=C awk -F'\t' '{s+=$4} END{print s+0}' "${M[@]}")
short=$(LC_ALL=C awk -F'\t' 'NF!=9{n++} END{print n+0}' "${M[@]}")
crlf=$( { LC_ALL=C grep -lU $'\r' "${M[@]}" || true; } | wc -l)
nl=0
for f in "${M[@]}"; do if [ -n "$(tail -c1 "$f")" ]; then nl=$((nl+1)); fi; done
# awk counts records, wc counts newline bytes: identical unless a TSV lost its final
# newline. Tying them keeps "a row went missing" from posing as "a newline went
# missing", which two independent zeros cannot tell apart.
nls=$(cat "${M[@]}" | wc -l)
unsorted=0
for f in "${M[@]}"; do LC_ALL=C cut -f1 "$f" | LC_ALL=C sort -c 2>/dev/null || unsorted=$((unsorted+1)); done
# Binary rows by PATH EXTENSION — the only test available to a script that reads no
# file content. The content-based backing is the iconv round-trip in wave0: same 7
# paths (comm → 0). Their f4 is a byte artifact, and it stays INSIDE the LOC pin.
binrow=$(LC_ALL=C awk -F'\t' '$1 ~ /\.(png|gif|woff2|prof)$/{n++} END{print n+0}' "${M[@]}")
binloc=$(LC_ALL=C awk -F'\t' '$1 ~ /\.(png|gif|woff2|prof)$/{s+=$4} END{print s+0}' "${M[@]}")
# Tier vocabulary is closed. A typo that still starts with a valid prefix
# (`A-planned@T99`, `A-planned@T6 `) keeps PARTITION whole and quietly adds its own
# grid line, so shape and tier↔wave agreement are checked separately.
badtier=$(LC_ALL=C awk -F'\t' \
  '$5 != "unassigned" && $5 !~ /^[AB]-[a-z]+@T[0-9]+[a-z]?$/{n++} END{print n+0}' "${M[@]}")
tiers=$(LC_ALL=C cut -f5 "${M[@]}" | LC_ALL=C sort -u | wc -l)
badwave=$(LC_ALL=C awk -F'\t' '
  $5 ~ /^[AB]-[a-z]+@T[0-9]+[a-z]?$/ { t = $5; sub(/^.*@T/, "", t); sub(/[a-z]$/, "", t); t += 0
    w = (t >= 4 && t <= 8) ? 1 : (t >= 9 && t <= 11) ? 2 : (t >= 12 && t <= 13) ? 3 : (t == 14 ? 4 : 0)
    if ($7 != w) bad++ }
  END { print bad + 0 }' "${M[@]}")
part=$(LC_ALL=C awk -F'\t' \
  '{A += ($5 ~ /^A-planned@/); B += ($5 ~ /^B-/); U += ($5 == "unassigned")} END {print A+B+U+0}' "${M[@]}")

check ROWS 862 "$rows"
check LOC 219247 "$loc"
check BAD_FIELD_ROWS 0 "$short"
check CRLF_FILES 0 "$crlf"
check NO_FINAL_NEWLINE 0 "$nl"
check RECORDS_EQ_NEWLINES "$rows" "$nls"
check UNSORTED_TSVS 0 "$unsorted"
check BINARY_ROWS 7 "$binrow"
check BINARY_LOC 4000 "$binloc"
check BAD_TIER_SHAPE 0 "$badtier"
check TIER_KEYS 11 "$tiers"
check TIER_WAVE_AGREE 0 "$badwave"
check PARTITION_ROWS "$rows" "$part"

CM='function comma(n,  s, o) { s = sprintf("%d", n); o = ""
  while (length(s) > 3) { o = "," substr(s, length(s) - 2) o; s = substr(s, 1, length(s) - 3) }
  return s o }'

# The fence is deliberately poisoned on the red path: a grid reaches markdown by
# being copied out of this block, so the warning has to travel inside it.
echo '```text'
if [ "$fail" -ne 0 ]; then echo '# SENTINEL_FAILURE — KNOWN BAD, DO NOT PASTE THIS BLOCK'; fi
echo "# audited pin dc047a7 / tree 3f7b69ae · ROWS=$rows LOC=$loc · $(date +%F)"
LC_ALL=C awk -F'\t' "$CM"'{f[$5]++; l[$5]+=$4}
  END{for (k in f) printf "%-16s %6s f %9s l\n", k, comma(f[k]), comma(l[k])}' "${M[@]}" | LC_ALL=C sort
echo
# Roll-ups, fixed order, never sorted. Tier A debt deliberately EXCLUDES
# B-vendored: stb_image.h is version-diffed, never read as first-party.
LC_ALL=C awk -F'\t' "$CM"'
  { if ($5 ~ /^A-planned@/)      { AF++; AL+=$4 }
    else if ($5 ~ /^B-/)         { BF++; BL+=$4 }
    else if ($5 == "unassigned") { UF++; UL+=$4 } }
  END { printf "%-16s %6s f %9s l\n", "A-planned",  comma(AF), comma(AL)
        printf "%-16s %6s f %9s l\n", "B-vendored", comma(BF), comma(BL)
        printf "%-16s %6s f %9s l\n", "unassigned", comma(UF), comma(UL)
        printf "%-16s %6s f %9s l\n", "TOTAL",      comma(AF+BF+UF), comma(AL+BL+UL) }' "${M[@]}"
echo '```'

if [ "$fail" -ne 0 ]; then
  echo "SENTINEL_FAILURE — do NOT paste these figures anywhere (exit 3)" >&2
  exit 3
fi
echo "ALL_SENTINELS_PASS=1 (expect 1)"
