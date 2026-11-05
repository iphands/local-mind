Target dc047a77beeddf1e91234d983a91d6bf929e9619 / tree 3f7b69ae4054969ed15b9b6f02d306eb44e0ebe4 — if that is not the tree you are looking at, you are in the wrong checkout.

# Census — every boundary call site in `vendor/basalt`, and who reads it

Plan 02 T3 as re-scoped on 2026-11-05 (plan § *Re-scope*). Every table below is computed by a script from the
census run's own files; the raw run output is `wave0/t3-census-rerun.txt`. This file answers one question for the
rest of the plan: **which lines of Basalt must a person read, and in which task.**

## How it was produced

```bash
context/audit/scripts/t3-inputs.sh                                          # manifest → census inputs
PYTHONDONTWRITEBYTECODE=1 LC_ALL=C context/audit/scripts/t3-census.sh vendor/basalt real
PYTHONDONTWRITEBYTECODE=1 LC_ALL=C context/audit/scripts/t3-census.sh /tmp/opencode/basalt-audit/canary canary
```

Generator `1bab1ec5e7933c90a80a97aa875218351052280445d585bfce6645f036128106` (the frozen cut). Real run: `SITES=278`, `SENTINEL_TOTAL=34 SENTINEL_FAILS=0`, every site carries a
task (`SITES_WITHOUT_A_TN=0`). It reproduces the committed `wave0/t3-sentinels-real.txt` line for line except the
timestamp and the toolchain line (this shell: bash 5.3.20, gawk 5.3.2).

## Canaries 1–3: can the census see what it claims?

Planted in a `/tmp` copy of the tree (never in `vendor/basalt`), at reserved `.invalid` hosts and TEST-NET
`192.0.2.1`. **Prediction, written before the run:** 3 caught by `net-socket`; 1 and 2 missed, because no census
family matches a URL literal or an encoded string.

| # | Canary | File | Census site? | Caught by the URL/encoded sweep? |
|---|---|---|---|---|
| 1 | base64 blob decoding to `https://canary-b64.invalid/beacon` | `serve/util/canary_cfg.py` | **no** (column 6 = `none`) | yes |
| 2 | literal `https://canary-literal.invalid/collect` in a header | `include/basalt/core/canary_tables.hpp` | **no** (column 6 = `none`) | yes |
| 3 | raw `socket()` + `connect()` to `192.0.2.1:443` in a CUDA file | `src/kernels/canary_reduce.cu` | **yes** — `net-socket`, routed T4 | no |

Canary run: `SITES=279` (real + 1), `TREE_DIFF_VS_REAL=+3/-0`, same generator as the real run.

**The prediction held, and it is a real blind spot.** Canaries 1 and 2 *were* seen by wide patterns
(`parser-network-bytes.wide`, `net-library-import.wide`), but the census promotes a wide hit to a site only when
the file also has a narrow hit, so **a file whose only suspicious content is a URL or an encoded string never
reaches a reader.** Fix, under execution rule 7 (one new detector, one control), not by editing the frozen
generator: the URL/encoded sweep below. Its control is the canary tree: it reports exactly the two planted lines
more than the real tree (+2 / −0), nothing else.

## The URL/encoded sweep

```bash
# url/encoded sweep — usage: urlsweep.sh <tree-root>; prints path:line:<text>, one per hit, sorted.
# Every regular file except .git, Markdown, docs/, licence texts and binary files (grep -I).
cd "$1" || exit 2
find . -type f -not -path './.git/*' -not -name '*.md' -not -path './docs/*' \
     -not -iname 'LICENSE*' -not -name 'THIRD_PARTY_LICENSES' -print0 | LC_ALL=C sort -z \
| xargs -0 grep -nIE 'https?://|wss?://|ftps?://|b64decode|base64\.(standard_|urlsafe_)?b64decode|atob\(|base64_decode' 2>/dev/null \
| sed 's|^\./||'
```

Real tree: **214 lines in 55 files** → `census-urls.tsv` (`path TAB line TAB route TAB text`). Routed by path:
`serve/`, `tools/` → T4 (is anything contacted?); `src/`, `include/` → T11 (explain each); vendored `stb_image.h` and
`.github/` → T12; web assets → T10; tests → `test-only` (listed, not read). Long base64-like literals are **not** in
this sweep: T11's long-literal scan owns them.

| Route | Lines | Files |
|---|---|---|
| T10 | 5 | 4 |
| T11 | 19 | 15 |
| T12 | 8 | 2 |
| T4 | 10 | 7 |
| test-only | 172 | 27 |

## Sites per task, after the re-scope

The census routed sites by the original plan. The re-scope removed two of those tasks, so 38 sites are
re-routed here (column 9 of `census-sites.tsv`; column 10 says why). Columns 1–8 are the census's own output,
unedited.

| Task | Census routed | After re-scope |
|---|---|---|
| T4 | 76 | 76 |
| T5 | 13 | 0 |
| T6a | 35 | 37 |
| T6b | 25 | 0 |
| T6c | 25 | 25 |
| T7 | 4 | 4 |
| T8 | 78 | 103 |
| T9 | 2 | 2 |
| T10 | 3 | 3 |
| T12 | 2 | 11 |
| T14 | 15 | 15 |
| skipped | 0 | 2 |
| **total** | **278** | **278** |

Re-routed T5 sites, each read before routing:

- `src/program/serve/vision.cpp:127` (file-write-outside) → **T6a** — where encoded image bytes are written
- `src/vision/stb_image.h:731` (fingerprint-read) → **T12** — vendored stb: one version diff vs upstream 2.30 decides it
- `src/vision/stb_image.h:732` (fingerprint-read) → **T12** — vendored stb: one version diff vs upstream 2.30 decides it
- `src/vision/stb_image.h:735` (fingerprint-read) → **T12** — vendored stb: one version diff vs upstream 2.30 decides it
- `src/vision/stb_image.h:739` (fingerprint-read) → **T12** — vendored stb: one version diff vs upstream 2.30 decides it
- `src/vision/stb_image.h:756` (fingerprint-read) → **T12** — vendored stb: one version diff vs upstream 2.30 decides it
- `src/program/serve/vision.cpp:6` (parser-network-bytes) → **T6a** — where encoded image bytes are written
- `src/vision/mmproj.cpp:1` (parser-network-bytes) → **skipped** — T5 skipped: decoder memory safety (Follow-up)
- `src/vision/preprocess.cpp:24` (parser-network-bytes) → **skipped** — T5 skipped: decoder memory safety (Follow-up)
- `src/vision/stb_image.h:141` (parser-network-bytes) → **T12** — vendored stb: one version diff vs upstream 2.30 decides it
- `src/vision/preprocess.cpp:14` (vendored-third-party) → **T12** — vendored stb: one version diff vs upstream 2.30 decides it
- `src/vision/stb_image.h:1` (vendored-third-party) → **T12** — vendored stb: one version diff vs upstream 2.30 decides it
- `src/vision/stb_image.h` (model-instruction) → **T12** — vendored stb: one version diff vs upstream 2.30 decides it

## T9's frozen file list

5 files, **818 lines** (under the plan's 3,000-line split rule):

- `src/program/generate/configure.cpp` — 326
- `src/program/image_loan.cpp` — 37
- `src/program/multi_gpu_plan.cpp` — 349
- `src/program/serve/loan_moves.cpp` — 37
- `src/program/serve/loans.cpp` — 69

