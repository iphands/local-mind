# Plan 02 — Basalt vendor audit: does `vendor/basalt` leak, steal, keylog, or phone home? — Tracker

## Overview

- Status: 16% complete (3/19 slices done — T0 ✓, T1 ✓, T2 ✓, matching `SERIES.md`; plus one follow-up
  commit on T1's artifacts, `3c27913`, which moved no manifest row and read nothing. **Still nothing of
  Basalt's source has been read** — T2 examined git objects and commit metadata, not behavior.)
- Start date: 2026-11-05 (T0)
- Plan: `context/plans/02_basalt_vendor_audit.md` — **read its Gate status block and Execution rules first**
- Evidence: `context/audit/` — `index.md` (entry point), `manifest/` (per-dir TSVs, fixed headerless
  schema), `census.md`, `argv-table.md`, `findings.md`, `coverage_ledger.md`, `verdict.md`, plus
  `context/audit/wave<N>/` for **raw command output** (raw output goes there, never pasted into this
  tracker). Scratch builds, the canary tree and the sanitizer harness: `/tmp/opencode/basalt-audit/`.
  Every `context/audit/` file opens with the stamp naming the tree it describes.
- **Audit target**: `vendor/basalt` @ `dc047a77beeddf1e91234d983a91d6bf929e9619`,
  tree `3f7b69ae4054969ed15b9b6f02d306eb44e0ebe4`, frozen 2026-11-05.

## Resume Instructions

1. **Prove the tree has not moved** — use the plan's *one* freeze-check, nothing else:
   ```bash
   cd vendor/basalt && GIT_NO_LAZY_FETCH=1 git diff --quiet dc047a77beeddf1e91234d983a91d6bf929e9619 \
     && echo "AUDIT TREE OK" || echo "AUDIT TREE DRIFTED — STOP"
   ```
   A drifted tree is a **blocker row** here and in plan Risk 1, then a re-freeze (T1) — never a
   silent continue. A pin bump by Plan 01 or the owner is exactly how a "clean" verdict becomes false.
2. **Find where the work actually stands** — two places, in this order: the **last row of
   `context/audit/coverage_ledger.md`** (what was read, how many lines, which commit) and the
   **`reviewed-in` column state in `context/audit/manifest/index.md`**. This tracker says which
   task is next; those two say which *file* the next session should open. Trust both, then read
   `context/audit/census.md` for the assigned-site list.
3. Read the project `AGENTS.md`, `context/plans/RULES.md` **in full**, then the plan — **its Gate
   status block and Execution rules 1–6 bind every task** (≤1,500-line commit slices; never write
   inside `vendor/basalt/`; sentinel-terminated commands; inline commands not helper scripts; the
   audit-file stamp).
4. **Environment — what works and what does not in this session:**
   - Working: `python3` (3.14.10), `git` (≥2.43, `GIT_NO_LAZY_FETCH`), `g++` (T5 harness),
     `shellcheck` 0.11.0 **at `~/.local/bin/shellcheck`** (installed by T0 on 2026-11-05; RULES.md
     updated), `curl` + `api.github.com` over TLS (unauthenticated, 60 req/h).
   - **Wrong in this list until T2 (2026-11-05): `gh` is NOT installed** (`command -v gh` → nothing).
     T2's provenance ran on `curl`; the plan's T2 Verify block is rewritten to match. Do not re-add
     `gh` to the plan without installing it first.
   - **Broken for the agent: docker.** Socket `root:rtkit 660`, session `uid=1000 groups=1000,27`,
     group `docker` gid 991 with no members. Gate steps 3–6 and every image-inspection command are
     **owner-run**: write the exact command, paste it here, mark the row from their output only.
   - If `shellcheck` is missing again, gate step 2 fails for real — record it in the plan's Gate
     status block, do not mark affected tasks `done`.
5. Ordering: **Wave 0 exits only when all five hold** — tree frozen + manifest complete over 862
  files; **6/6 canaries caught**; every census row carries an assigned task number; the pin proven
  against public upstream; **T9's file list frozen with a line count**. Then T4–T8, T9–T11d, T12–T13,
  T14. A missed canary **voids the census**: re-run before recording any further Tier B reason.
  **T8's argv table** (`request field` in any element) and **T7** (pack metadata reaching Jinja)
  stop and escalate if positive — do not continue past them.
6. **Tick unit ≠ commit unit**; a slice flips *all* its manifest rows in its own commit. Coverage
   figures in `coverage_ledger.md` are **pasted from a command** — a typed figure is a Rule E
   violation and does not count.
7. Commit per slice as `[basalt][P2][T<n>][audit] <summary>` (T6a/T11a forms included), with **this
   tracker's row in the same commit**, and name any gate step the commit could not run.

## Open Unknowns

| # | Unknown | Blocks | Resolve by |
|---|---|---|---|
| U1 | Can an HTTP request field reach an `argv` element or a `dlopen` soname, **in either language**? (The argv is assembled in `serve/`, consumed in `src/platform/`.) | every "the engine cannot exec" claim | T8's argv table |
| U2 | Does request-supplied content reach Jinja2 as a **template** (exec) or only render **data**? Can a pack override the template? | the code-exec question | T7 |
| U3 | Do the JPEG/GIF/BMP/PNG fixes since 2.21 (CVE-2022-28041/28042, GHSL-2023-145/148/150, the 2.28 BMP offset hardening) actually trigger in *this* build, given the `max_pixels` guard — and is that guard evaluated against the same product `out.rgb.assign` consumes? | the decode-path ranking (P0) | T5 |
| U4 | Is `stb_image.h` the only vendored third-party source in `src/`+`include/`? | Tier B completeness | T11 |
| U5 | ~~Does `connect()` re-resolve?~~ **RESOLVED at plan creation (2026-11-05)**: `_PinnedConnection.connect()` iterates the *pre-checked* `self.addresses` and does not re-resolve; Host header and TLS check stay on the original host (`image_source.py:422-460`). Residual: is that address *set* ever built from anything unvalidated, and the non-DNS spoofs. | the SSRF verdict | T4 |
| U6 | Is the cross-card "loans" path purely intra-process? Check **API names**, not tier names: `cudaMemcpyPeer`, `cudaDeviceEnablePeerAccess`, `cuMemExportToShareableHandle`, `shm_open`, `libfabric`. | the engine comms claim | T9 |
| U7 | Does `GET /metrics` disclose host identifiers (GPU/driver/CUDA/CPU) to a LAN peer, and do we care? | the posture action list | T6a, T13 |
| U8 | `.pth` / `sitecustomize.py` / `usercustomize.py` in the built image's venv. **Unreachable from the agent session** (docker); even whether an image for the pin exists is an owner-side fact. | the install-hook claim | Plan 03 / owner |
| U9 | Did anything from the removed remote-expert tier (`97ff061`) survive as a **plumbed-but-sinkless** `host`/`port`/`url`/`token` key in the config schema or the GGUF/profile key space? | the highest-signal malice-residue check | T3 (census section) + T9 |
| U10 | Does the server path call `stbi_set_flip_vertically_on_load`? (GHSL-2023-146 / CVE-2023-45662 is in `stbi__vertical_flip`.) | T5's format exposure list | T5 |
| U11 | Was **basalt PR #47** (`9a49e6b1 … (#47)`, the outside-authored `src/platform/memory.cpp` work) self-approved, and did the seam-*creating* commits go through a PR at all or land by direct push? | a census of **review**, which outranks any blame plurality as a priority argument; public record in the audited repo | **T3 first action** on read-order items 1–4. Half of it is now **local** — `wave0/t2-boundary-authorship.txt` § 6a censuses the `(#<n>)` subject suffix with no API (178/2,194; basalt-era 4.8 %, so *absence is the base rate and proves nothing*); only **self-approval** still needs the PR page. `9a49e6b1` = `(#47)` ✓, pin `dc047a77` = `(#242)`, `c487d17f` and `7e040829` carry no suffix |
| U12 | Are the *bytes* of the seven `basalt-line only` seam files original, or a squashed import from a rewritten ancestor? Commit absence proves neither way. | the only open question that could turn the lineage finding into a code-provenance one; needs blob-SHA and `git patch-id --stable` joins against `Niko1221/Strata`, not ancestry | **T2b** (registered in the plan) |
| U13 | What is in `refs/remotes/origin/multimodel` — 105 commits ahead of the pin (merge-base `1b3f6964`), in no manifest row, no tier, unread? | the audit covers one ref at one commit; a second published branch is a scope hole, and T13's bump path runs through it | T13 at the next pin bump, or a scoped T11e if the owner wants it now |

## Progress

| # | Slice | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 0 | T0: Restore the gate (shellcheck user-local, RULES.md prerequisite) | `context/plans/RULES.md` | **done** | v0.11.0 at `~/.local/bin`, negative control fired SC2115/SC2086; gate 2 `shellcheck ok`; RULES.md steps 2 + Prerequisites corrected in place, dated. Docker (steps 3–6) recorded owner-run, not fixed |
| 1 | T1: Freeze the pin, manifest all 862 files | `context/audit/manifest/` | **done** | 862 rows / 12 TSVs / 219,247 l; `MISMATCH_COUNT=0` under `GIT_NO_LAZY_FETCH=1`; tree frozen, `VENDOR_DIRTY=0`; 862 objects all `blob` (858×100644, 4×100755 — no gitlinks, no symlinks); 0 paths outside printable ASCII; worktree `sha256sum` == object sha256 for all 862; pipeline byte-deterministic over two runs and `diff`-equal to the committed TSVs. Tier A pre-seeded 75 f / 19,659 l, `B-vendored@T5` 1 f, **786 f `unassigned` for T3**; **0 lines read**. Gate 1a/1b/1c/2 pass, 3–6 docker-blocked (owner). See `context/audit/wave0/t1-freeze-and-manifest.txt` |
| 2 | T2: Pin provenance + boundary-author census | `manifest/index.md` | **done** | **All 862 blob SHAs match GitHub's tree object `3f7b69ae` for the pin** — `INTERSECT=862 ONLY_IN_API=0 ONLY_IN_LOCAL=0 BLOB_SHA_MISMATCH=0`, `GITLINKS=0`, canonical `sha256(path⇥blob)` equal both sides; sensitivity control against the first parent returns `BLOB_SHA_MISMATCH=1` (one file: `serve/util/telemetry.py`). **Limit, recorded as a limit:** the signature is GitHub's own key (`B5690EEEBB952194`) on a GitHub-made squash merge and is unverifiable here (`No public key`); all 3 of **basalt's own** tags are annotated-**unsigned** (Strata's same-named tags are different objects); SWH has no origin for `jesdga95/basalt` and no revision for the pin → **no non-GitHub anchor for the pinned commit** (it *does* hold the ancestor Strata in 9 visits — 8 `full`, 1 `failed` — and our root commit — see `wave0/t2-lineage.txt` § 7; an earlier draft of this row said SWH "has never archived this project", which was true only of the broken `/get/` endpoint form — `pitfalls.md` item 10). **Pin is untagged `main`, not `v0.3.5`** (that tag does not exist upstream; docs/pyproject claim it → no release artifact to diff, feeds Risk 5); 3 commits since `v0.2.1`; no `SECURITY.md` anywhere. Census of **87 rows / 80 blameable first-party files** (`scripts/t2-census.sh`, which reproduces `wave0/t2-author-census.tsv` byte-identically — *the 80*: 87 rows − 6 `BINARY_SKIPPED` − 1 vendored `stb_image.h`; 69 `A-planned` + 11 `unassigned`): 20,111 blamed lines, 53 authors, core two 81 %; **560 lines (2.78 %) sit on a blame boundary and they are the same 560 credited to the root commit**, per-file identical across 7 files (blame cannot walk past a parentless commit), so this is a **lineage** quantity, not an error bar — `wave0/t2-provenance.txt` § 6. *Corrected 2026-11-05: this row first said "**0** blame-boundary lines", which was a false zero from an unanchored `grep boundary` over blame output — `pitfalls.md` item 13. The file denominator also went 86 → 80, and `tail -n +2` on this headerless TSV dropped `CMakeLists.txt` from two of my own verification commands — item 14.* `serve/engine/image_source.py` 570 l = 1 commit/1 author; `dynlib.cpp` 64 l = 1 commit/1 author; **`jeancmyk` holds the plurality of `process.cpp` 216/321, `file.cpp` 247/456, `direct_file.cpp` 255/455, each majority landing in ONE commit** — read first (T8). Tiering gaps found: `CMakeLists.txt` (287 commits) and `serve/engine/process.py` (1,419 l) are `unassigned`. Two plan errors corrected: `src/server/` does not exist; `gh` is not installed. Raw: `wave0/t2-provenance.txt`, `t2-github-tree-paths.txt`, `t2-boundary-authorship.txt`, `t2-gate-and-checks.txt` |
| 3 | T3: Census + **6-canary** control + orphan-key + diagnostics-shape | `context/audit/census.md` | pending | 6/6 or census is void; also freezes T9's list |
| 4 | T4: `image_source.py` + `security.py`; **execute** the 9-class bypass list | `serve/engine/image_source.py` | pending | deliverable = delta vs upstream tests |
| 5 | T5: stb 2.21 decode path; ASan/UBSan + CVE PoC corpus + guard arithmetic | `src/vision/` | pending | P0; harness agent-built, long runs owner-run |
| 6 | T6a: Logging + `/metrics` disclosure | `serve/util/`, service/handler | pending | ≤1,500-line slice |
| 7 | T6b: Exec trust boundaries → argv table | `serve/config.py`, `service.py` | pending | feeds T8 |
| 8 | T6c: Input-capture + credential negative sweep | `serve/` | pending | one command, sentinel-terminated |
| 9 | T7: Template injection via pack metadata + tokenizer ReDoS | `chat_template.jinja`, `tools/tokenizer.py` | pending | |
| 10 | T8: Platform seam + **cross-language argv table** + script limits | `src/platform/`, `argv-table.md` | pending | licenses all engine claims |
| 11 | T9: Census-assigned engine files + `97ff061` diff + peer-API greps | `src/program/serve/`, `multi_gpu_plan.cpp` | pending | scope frozen at Wave 0 exit |
| 12 | T10: Opaque bytes: `.prof` + fonts/images + parser | `data/`, `serve/web/` | pending | |
| 13 | T11a: Tier-B triage `src/kernels/` | 38,745 l | pending | mechanical fill + exception review, own commit |
| 14 | T11b: Tier-B triage `src/core/` | 19,976 l | pending | own commit |
| 15 | T11c: Tier-B triage `src/` remainder | ~45k l | pending | own commit |
| 16 | T11d: Tier-B triage `include/` + vendored-lib sweep | 18,224 l | pending | resolves U4 |
| 17 | T12: Dependency provenance + hash gap in the headline | `requirements.txt`, `Dockerfile` | pending | |
| 18 | T13: Our posture: `./run` + Dockerfile action list | `run`, `lib/common.sh`, `Dockerfile` | pending | owner asked for this in scope |
| 19 | T14: Channel matrix, verdict, harvest, close | `context/audit/verdict.md` | pending | no matrix → no verdict |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell. A row whose only verifier is a
docker-gated step is **not** `done`; mark it `blocked` naming the step.

## Evidence

Coverage numbers are pasted from the named command. A hand-typed coverage figure is a Rule E
violation and cannot be cited by a later plan.

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-05 | Planning recon: per-area `git ls-files \| xargs cat \| wc -l`; primitive / `getenv(` / peer-name / obfuscation / vendored-lib greps; `git diff HEAD` + per-file `cat-file -e`; `sed -n '60,62p' stb_image.h`; `_PinnedConnection.connect` read; stb CVE check against upstream + GHSL advisories | Plan Key Facts table + the P0 decode-path finding, with limits recorded | 1 | Pre-plan baseline: T1 re-checks it, it is not first-pass evidence |
| 2026-11-05 | Gate steps 1a/1b/1c/2 (after `shellcheck` v0.11.0 install) | `3 b`, `env ok`, `shellcheck ok` | 1 | Steps 3–6 blocked: docker `permission denied`, owner-only |
| 2026-11-05 | `PREFLIGHT_ONLY=1 ./build` | **exit 1** — `permission denied … /var/run/docker.sock` | 1 | The measurement that exposed a false Key Fact; see Notes |
| 2026-11-05 | T1 freeze + integrity suite: the one freeze-check; per-path `git hash-object` vs `ls-tree` (printed count); `ls-tree --format='%(objecttype) %(objectmode)' \| uniq -c`; `grep -cvP '^[\x21-\x7e]+$'` on `--name-only`; empty-blob count; `git status --porcelain --ignored \| wc -l`; worktree `sha256sum` pass joined against manifest column 3; `comm -3` manifest↔`ls-tree`; generator run twice + `diff -r`, and `diff -r` vs the committed TSVs | `AUDIT TREE OK` · `MISMATCH_COUNT=0` · 862 × `blob` (858 × `100644`, 4 × `100755`) · non-ASCII/space paths `0` · `EMPTY_BLOBS=0` · `VENDOR_DIRTY=0` · `SHA_MISMATCHES=0` · `comm` = `0` (862 = 862 = 862) · two runs byte-identical and equal to what is committed | 1 (+1 determinism re-run) | Full verbatim: `context/audit/wave0/t1-freeze-and-manifest.txt`. `NO_TRAILING_NL=12` bounds the LOC column |
| 2026-11-05 | T1 drift-check **negative control**: tamper a LOC digit in a `/tmp` copy of `cmake.tsv`, re-run the `frozen-prefix.sha256` recompute loop | `PREFIX DRIFT cmake.tsv`, `CONTROL_DRIFT_COUNT=1` — the check fires | 1 | Proves the clean `PREFIX_DRIFT_COUNT=0` on the real manifest means something (RULES.md Rule A: a silence that has never been shown to speak is not a pass) |
| 2026-11-05 | Gate steps 1a/1b/1c/2, re-run **with `context/audit/` present** (3 new `.md`, 12 `.tsv`, 1 `.sha256`) | 1a no `FAIL` · **`3 b`** · `env ok` · `shellcheck ok` | 1 | Rule 5 holds: none of T1's artifacts enters the bash set. Steps 3–6: step 3 re-measured as docker-denied; 4–6 unrun |
| — | T3 canary capture (target **6/6**, one per planted class) | — | | **Wave 0 gate** |
| — | T5 ASan/UBSan fuzz, JPEG/PNG/BMP/GIF at request-reachable `max_pixels` | — | | report count, target 0 |
| — | T11 `LC_ALL=C comm` manifest vs `git ls-files src/ include/` (target 0) | — | | after each directory slice |

## Notes / Deviations

- **2026-11-05, at plan creation — the plan's depth was the owner's choice, not the agent's.** The
  ask was "file by file, line by line"; blanket line-by-line over `src/`+`include/` (125,152 lines,
  514 files) prices at 40–70 focused sessions at 400–600 LOC/h. Shown that, the owner chose
  **two-stage: boundary-first now (Tier A ≈ 34k lines), engine bulk deferred to Plan 04**. Any future
  "audit complete" must be read as *"boundary audit complete; engine bulk triaged, not read"*, and
  the verdict must carry that wording.
- **2026-11-05 — effort estimate corrected 10–14 → 14–19 sessions.** ~34,000 Tier-A lines at
  400–600 LOC/h is 57–85 hours of pure reading; at 3–4 h of genuine adversarial reading per session
  that is 14–19 sessions *before* gate overhead and before the owner-run docker/fuzz waits. The first
  figure was ~1.5× optimistic. T5 sits on the critical path with Plan 01's owner-stall as precedent.
- **2026-11-05 — the P0 is bigger than the reviewer's version, and my first correction
  over-corrected.** `src/vision/stb_image.h` is **2.21 (2019-02-25)** (the missing
  `STB_IMAGE_RELEASE` macro is version evidence — it starts at 2.26), compiled
  `STBI_ONLY_JPEG|PNG|BMP|GIF` + `STBI_NO_STDIO`. My "narrower than claimed" holds for
  **TGA/PNM/PSD/PIC/HDR** (CVE-2021-42715, CVE-2021-42716, GHSL-2023-147 — compiled out, do not
  re-raise) but is **false for the four that are compiled in**: JPEG carries CVE-2022-28041 and
  CVE-2022-28042 (Huffman-table **out-of-bounds writes**), GIF carries GHSL-2023-145/148/150
  (reported against **2.28**, so newer than this copy), BMP carries the 2.28 `psize == 0`
  offset-overflow hardening, PNG gained the 2^30 IDAT limit in 2.28. `STBI_MAX_DIMENSIONS` *is*
  present (23 hits) but caps dimensions, not the per-decoder arithmetic where these live. The
  first-party arithmetic in `preprocess.cpp` (`(int) std::min<size_t>(len, 0x7fffffff)`,
  `out.rgb.assign(px, px + w*h*3)`) is a second, non-diffable half no version comparison can settle —
  hence T5's guard-vs-consumer check.
- **2026-11-05 — two review claims checked before being trusted; both needed correcting.**
  (a) "unversioned stb" → it is versioned 2.21, and three of its four compiled-in formats have
  named security fixes since; recorded above. (b) "partial clone is a corruption risk" → it is not:
  `git diff HEAD` clean, no `alternates`, no `core.objectsfile`; the partial clone is kept as
  **procedure** (`GIT_NO_LAZY_FETCH=1`, the single freeze-check) against silent skips only.
  A third reviewer point — that the argv is assembled in Python and consumed in C++ — was **right and
  load-bearing**: no task as originally scoped owned it, so T8 now owns a cross-language argv table.
- **2026-11-05 — I recorded a false Key Fact, caught it, and the failure mode is the one this whole
  plan exists to prevent.** I wrote "no built image exists for the pin" from
  `docker images | grep -i basalt` printing nothing. The command had **failed** — `permission denied`
  on `/var/run/docker.sock` — and the error went to stderr where `grep` filtered it. Same class as
  `git cat-file -e` succeeding *by lazily fetching* when the question was "was it already local".
  Both Key Facts are downgraded in the plan with their limits stated; the lesson and the
  sentinel-echo convention are now in `context/pitfalls.md` `[OBSERVED 2026-11-05]`.
  **An empty result from a command that was never allowed to run is not a measurement.**
- **2026-11-05 — the census design exists because a danger grep is blind to parsers by construction**,
  and the *category list* itself is unprovable by canaries drawn from that list. Hence canaries
  #4–#6 (cross-file assembly, token-count/latency side channel, log→`/metrics` second hop), which no
  keyword in the taxonomy would find. If a later session finds the census flagging only files
  already suspected, the census has failed and the verdict is void — say so here, do not proceed.
- **2026-11-05, T1 — Tier A was seeded from the plan, not from a census, on purpose.** The manifest
  columns were left blank in the task text. The senior reviewer's objection was accepted after I
  weighed it: a *directory-level* Tier B default would be an unevidenced claim, and Tier B reasons
  still wait for T3 — but Tier A membership is already evidenced by this plan's own `File:` fields
  (§T4/§T5/§T6a-b/§T7/§T8/§T10/§T12), so `A-planned@T<n>` was written for **75 rows / 19,659 l** before
  any census ran, with the invariant **"an `A-planned` row may only ever become `read@T<n>`, never
  `triaged:*`"** (`context/audit/manifest/index.md` § Invariants). Without it, T11's mechanical fill
  would run *after* a negative census, and the census would effectively have chosen what got read —
  "grep found nothing" laundered into "audited", with the manifest as the laundering vehicle. The 786
  undecided rows are `unassigned`, **not** Tier B; `unassigned` ≠ `none` is a reserved distinction.
- **2026-11-05, T1 — four commands reported green on a measurement that had not happened; two of them
  came from the reviewers.** `IFS= read -r h p` against a space-separated `git --format` (862 × `could
  not open ''`, and the same loop *had* already printed a correct 0, so the failure read as a
  regression); a `.github.tsv` that `*.tsv` cannot see (row total read **859**); `diff -r` declaring two
  empty directories "DETERMINISTIC" after the generator ran from the wrong cwd; and `sha256sum -c`
  against a sidecar of `cut -f1-4` hashes, which re-hashes whole files and so reported 12 bogus
  `FAILED`s. What caught each was a **printed number**, not an `rc` — which is why the plan's own T1
  Verify block is annotated as superseded by printed-count forms and every check here now prints its
  count inside the producer. Harvested to `context/pitfalls.md` § *Four shell idioms that report green
  on the thing they did not check*.
- **2026-11-05, T1 — a reviewer filed a false "missing coverage" finding that my own summary caused.**
  The junior review's #1 must-fix was "`cmake.tsv` is missing: 11 groups sum to 860 files / 219,185
  lines, not 862 / 219,247". It is not missing — 12 TSVs were generated and `wc -l` showed
  `cmake.tsv 2`. The 860 came from **my** per-directory list in chat, which named 11 buckets while
  quoting the 12-bucket total. Recorded because the failure mode is general and this plan is about it:
  *an abbreviated evidence table reads as missing coverage*, and a reviewer can only reason from what
  it is shown. `context/audit/manifest/index.md` now carries all 12 rows unabbreviated, with the
  `cmake/`+`CMakeLists.txt` 3 f / 1,569 l cross-check trap spelled out (`CMakeLists.txt` is in
  `root.tsv`).
- **2026-11-05, T1 — a hole the plan left open, closed in the artifact not the plan text.** T9's scope
  is "the rows assigned T9 in `census.md`", but the manifest schema has no `assigned-TN` column, so
  those rows had no home and T9 could have read files while their manifest rows stayed `pending`. The
  rule now written into `manifest/index.md`: at Wave 0 exit, the commit that freezes T9's file list
  also flips those rows to `A-planned@T9` / wave `2`, and `census.md`'s `assigned-TN` must agree with
  this file's `tier` — a row `census.md` owns that still reads `unassigned` here is unowned work and
  **Wave 0 does not exit**.
- **2026-11-05, T1 follow-up — the coverage figures were right, their labelling was not, and the check
  built to guard them could not fail.** Four things, all inside the audit's own artifacts:
  1. `coverage_ledger.md` totalled "Tier A seeded (76 f) 27,647 l", folding `B-vendored@T5`
     (stb_image.h, 7,988 l) into Tier A debt. **Tier A is 75 f / 19,659 l**; the vendored row is
     version-diffed, never first-party reading. The figure is retired where it appeared and the rule
     is stated in `coverage_ledger.md` and `manifest/index.md`.
  2. Every coverage number is now **generated** by `context/audit/scripts/ledger-grid.sh` from the 12
     TSVs — raw output `wave0/t1-ledger-grid.txt`, nine negative controls (each had to abort non-zero)
     in `wave0/t1-grid-controls.txt`. `manifest/index.md` owns the per-bucket grid;
     `coverage_ledger.md` and `audit/index.md` carry roll-ups that cite it, so every number has one
     owner. Added under Execution rule 5's "unavoidable helper" clause with **RULES.md's gate count
     bumped `3 b` → `4 b` in the same commit** — the owner chose this over keeping the generator inline
     after being shown the rule conflict.
  3. **Two of those controls failed the script, not the data**: an assertion printed by raw `awk` never
     set the fail flag (a typo'd tier reported `FAIL` while the script exited 0), and `*.tsv` **cannot
     match a dot-prefixed file**, so the check built to catch T1's own 859-row failure was blind to it.
     Fixed by routing every check through one `check` helper and pairing the glob with
     `find -type f`. The manifest-shape checks in `manifest/index.md` § Checks lost their
     `silent = ok` / `order ok` forms and now print counted sentinels; re-run from the doc exactly as
     pasted, all eleven pass.
  4. `A-planned@T6a`/`T6b` exist, so a task-id regex of the form `T[0-9]+$` silently drops six rows —
     same class as (3). Tier shape and tier↔wave agreement are now checked (`TIER_KEYS`,
     `TIER_WAVE_AGREE`: 0 violations), and the `(= git ls-tree order)` claim is verified per TSV
     (`ORDER_MISMATCH_TSVS=0`, `SUM_OF_WANTED_SETS=862`) instead of asserted.
  Not drift: nothing in columns 1–4 moved (`PREFIX_DRIFT_COUNT=0`, `ROWS=862`, `LOC=219247` before and
  after). **0 lines read, unchanged.**

- **2026-11-05 · T2 — three things the plan got wrong, corrected by running it, not by reading it.**
  (1) **`src/server/` does not exist.** T2's boundary definition names `src/platform/*`, `src/vision/*`
  **and `src/server/*`**; `git ls-tree -d dc047a77:src/server` is fatal. The real C++ seam is
  `src/platform/` (10 f) + `src/vision/` (10 f) + `include/basalt/platform/`; the serving layer is
  Python under `serve/`. (2) **`gh` is not installed** — the plan's T2 Verify and this tracker's
  environment list both assumed it; T2 ran on `curl` against `api.github.com`. (3) **The pin is not
  tag `v0.3.5`.** No such tag exists upstream (only `v0.1.0/v0.2.0/v0.2.1`, all annotated and all
  **unsigned**); the pin is untagged `main`, 3 commits past `v0.2.1`, authored **the day we pinned it**.
  Consequence carried to Risk 5: **there is no upstream release artifact to diff the pin against.**
- **2026-11-05 · T2 — two files the tier system had wrong, deliberately not retiered.**
  `CMakeLists.txt` (1,507 l, **287 commits**, 18 blame authors, 34 contributors at ≤2 commits — the
  highest-churn file in the census set) and `serve/engine/process.py` (1,419 l, 6 blame authors — the
  code that spawns the engine) are both `unassigned`. Retiering them **from a census** is exactly the
  goalpost move the `A-planned`-seeded-from-`File:`-fields invariant exists to prevent, and 1,507 l
  also breaks the 1,500-l slice cap. Assigned forward instead: **amend plan `### T11`'s `File:` field
  first, then retier from it** — proposed `T11a` (toolchain/options/gate) + `T11b` (targets/install);
  `serve/engine/process.py` belongs to T4 or T6. Plan 01 read `CMakeLists.txt:40,94-117,528,533` and
  287 commits have landed since, so that read is stale — it needs redoing whenever T11a runs.
- **2026-11-05 · T2 — what "a line of Tier A" actually is, and a 1.7× effort discrepancy.** Tier A's
  75 f / 19,659 l is **54 code files / 13,615 l + 21 non-source files / 6,044 l** (`docs/img/frontend.gif`
  alone is 2,409 of those "lines", `data/profiles/qwen4exp-48x512.prof` 960,
  `docs/community/benchmarks.json` 922). The owed-read count is **not** reduced and no row was
  retiered — invariant holds; re-tiering is T11's. Separately: **plan § Effort says "Tier A is
  ~34,000 lines" against the manifest's 19,659**, and the 14–19-session figure above was derived from
  the 34k number. Recorded so the manifest does not silently supersede the estimate; revisit it at
  T13, not in the middle of a reading slice.
- **2026-11-05 · T2 — artifact placement, a deliberate deviation.** The plan's T2 `File:` field names
  only `context/audit/manifest/index.md`, which has no room for an 87-row census, and `census.md` is
  **T3's** declared artifact (primitive census + canaries) — pre-filling it would collide with the next
  task. So: conclusions and limits in `manifest/index.md` § Upstream provenance (the declared file),
  raw substrate in `wave0/t2-*`, generator in `scripts/t2-census.sh` (**RULES.md gate bumped `4 b` →
  `5 b` in this same commit**, second use of the unavoidable-helper clause). T3 must read
  `wave0/t2-boundary-authorship.txt` § 6 before assigning dispositions.
- **2026-11-05 · T2 side effect: the "frozen" clone is no longer blob-empty.** `git blame` on a
  `blob:none` partial clone lazy-fetches; `count-objects` went to 19,352 in-pack across 752 packs
  (~53 MiB). Tree hash and `VENDOR_DIRTY=0` unaffected. `GIT_NO_LAZY_FETCH=1` is the freeze-check
  discipline, not a census constraint — the census cannot run under it.
- **2026-11-05 · T2 — four green-but-wrong results in this task's own tooling, one of them published.**
  Recorded because each produced a *plausible* number that survived into a draft: (1) **`boundary_lines=0`
  shipped into the first drafts of `t2-provenance.txt` § 6 and `t2-boundary-authorship.txt`** — an
  unanchored `grep boundary` over blame output; anchored truth is **560 lines in 7 files**, and the
  framing was wrong too (it is a lineage quantity, not an error bar). (2) **File-denominator drift,
  86 vs 80**: three denominators (80 blameable / 86 minus-vendored / 87 rows) had been used
  interchangeably across pages until this commit; every figure now names its own. (3) **`tail -n +2` on
  the headerless census TSV** dropped `CMakeLists.txt` — the largest row — from two verification
  commands, manufacturing an "80+6 ≠ 87" anomaly that took three commands to explain; the measurement
  itself had used a field filter, which is the only reason the anomaly was visible at all. (4) The
  **`/api/1/revision/<sha>/get/` sweep** returned `404` for all nine SHAs, and had been drafted as
  "Software Heritage has never archived this project" — the route was wrong; corrected, it holds the
  ancestor repo and our root commit. All four are in `pitfalls.md` items 10–14 with the commands that
  refuted them. **The process lesson, once:** the two that reached paper did so because the check that
  would have caught them was written *after* the draft. A probe family that returns one uniform value has
  measured itself, and an unanchored porcelain keyword grep is not a measurement.
- **2026-11-05 · T2 — a gate transcript line in this task's own artifact was fabricated, and was caught
  before the commit.** `t2-gate-and-checks.txt` first recorded step 1c as
  `SKIP (no versions.env yet)` labelled "SKIP, not pass". No run ever printed that: `versions.env` has been
  tracked since `1eafda7` (2026-11-05T01:20Z), and T1's stamped gate block records `env ok` for the same
  step — so the line came from what the step *would* print in a repo without pins, not from an observed
  run. The artifact now carries the real output with a dated CORRECTED block, and the trap is
  `pitfalls.md` item 17. Cost if it had shipped: this project's `SKIP, not pass` marker is the signal that
  steps 3–6 are genuinely owner-only; spending it on a line that never ran teaches a reader to distrust the
  true SKIPs. An earlier draft of this note said the step had "changed state under this task" — that was the
  same error one level up, and it is corrected here rather than deleted.
- **2026-11-05 · T2's audit-record / knowledge-layer split — deliberate.** Both reviewers reached the same split
  independently, for the same reason: the *use* of the provenance evidence (the ancestor-rewrite finding,
  the AI-attribution corpus, T2b, T3's calibration rules) is separately arguable, and if that framing is
  later re-argued or partially withdrawn, the measurements must not go with it. **As landed:** commit 1 =
  `9a56c8d` (the audit record: the six `wave0/t2-*` artifacts + `scripts/t2-census.sh` + the manifest/audit
  index updates + RULES.md + pitfalls items 10–17 + the monorepo `context/pitfalls.md`), 14 files
  +3,164/−31; commit 2 = `93b6bd5` (the knowledge layer: `distilled.md`, `high_level.md`, `AGENTS.md`,
  `SERIES.md`, this plan's Key Facts/T10/T13/T2b/T3 edits, and U11–U13), 6 files +352/−9. Neither commit
  contains `./run` or `opencode2/bin/opencode-serve`, which are the owner's in-flight edits. **T2 grew to
  five commits**, the last three being post-review corrections: **3** = `7822c1e` (3 files +46/−9, the
  diff-shape figure), **4** = `6dbb312` (7 files +190/−14, the stale `1,690`, the flat SHA ban, § 6a), **5** =
  `0c7517d` (4 files +28/−3, Rule E.7 and the owner-file qualifier). Slice total across those five: **19
  distinct files, +3,754/−40**, re-derivable with `git diff --numstat 94dfa80..HEAD` — printed here as a
  command as well as a number because this bullet cannot count its own commit: the later corrections to this
  row are outside the range quoted, so re-run it rather than trusting the figure for a slice still growing.
- **2026-11-05 · T2 post-commit correction (a third commit, `P2/T2`, small).** `9a56c8d`'s own artifact
  published its diff shape as hand-written **1,843**; the true figure is **1,861**. Cause: the measuring
  command named `server/basalt/context/coverage_ledger.md` when the file is
  `…/context/audit/coverage_ledger.md`, and a non-matching pathspec in a `--numstat --` list contributes
  nothing silently. The artifact now carries the right number **plus the command that derives it** and a
  partition check (`954 + 1,861 + 349 = 3,164` = the commit's total), and the trap is recorded as a
  corollary inside `pitfalls.md` item 15 rather than a new item. Not amended into `9a56c8d`: history here is
  append-only, so that commit permanently contains the wrong figure and the correcting commit says so.
- **2026-11-05 · T2 post-commit correction, round two (a fourth commit) — the correction itself was
  incomplete.** Both reviewers found, independently, that § 7 of the same artifact *still* closed with
  "the **1,690** is not reading volume" and cited the plan cap as a bare "(line 83)". 1,690 was this
  artifact's own first-draft figure for a 13-file pre-split slice that the two-commit split replaced — it
  was never wrong, only unreferenced, and `7822c1e` did not find it because a value-search for the number you
  know is wrong cannot find one you do not know about. Swept properly this time by asking of every numeral in
  the section *which command reproduces it at this commit*; `−33` had already died (0 hits repo-wide) and
  `1,690` / "line 83" had exactly one hit each. The section now carries a per-commit partition loop over
  `9a56c8d`/`93b6bd5`/`7822c1e` whose class sums must equal each commit's own total. Also from this round:
  `audit/index.md`'s "Never compare SHAs between these two repositories" was **flat** and contradicted
  U12/T2b, which require a blob-SHA and `patch-id --stable` join — now scoped to **commit** SHAs with the
  legitimate join keys named, so a reader who meets the ban first does not skip U12 as out of contract. And
  `t2-boundary-authorship.txt` gained **§ 6a**, T3's pre-flight: a path/tier/LOC assertion over the read
  order (`PATHS=10 MISSING=0 TIER_MISMATCH=0 LOC_MISMATCH=0`, all three control branches firing) and the
  local PR-suffix census that makes U11's second half computable with **zero API calls**
  (`WITH_PR_SUFFIX=178` of 2,194; pre-fork 13.2 %, basalt-era **4.8 %**) — published *with* its base rate,
  because at 4.8 % the absence of a suffix is the repository's normal case and proves nothing about direct
  pushing. Two of my own controls were broken before they were believed: the read-order check's LOC branch
  never fired (the injected path did not exist at the pin, so it exited at `MISSING_AT_PIN` first), and the
  published block printed an all-green `PATHS=0 MISSING=0 …` from an empty input, which is why it now refuses
  to report unless it read 10 rows.
- **2026-11-05 · T2 review round three (the sign-off round) — no blockers from either reviewer; three small
  durable changes.** (i) `RULES.md` **Rule E.7**: a project-wide rule has one **owner file**; other files cite
  it plus the consequence for their own reader rather than restating the prohibition, because a shortened
  restatement loses its qualifier and **an over-broad ban suppresses work instead of corrupting it** — it
  makes a later task skip a required check, so it fires no control and never self-reports. That asymmetry is
  why two of three copies of the SHA-comparison rule survived until a phrase grep. (ii) `audit/index.md` §
  *Where things live* now points at **§ 6a as T3's zero-API pre-flight**, per hoodie's placement ruling: the
  § *Six commands* list certifies the audit's own state and is pass/fail, so adding a non-diagnostic task
  pre-flight to it would lend that list a green it cannot earn — a pointer, not a relocation. (iii) The third
  copy of the ban, `t2-lineage.txt` § *sound methods* item 3, reworded from "SHA comparison … is meaningless"
  to "commit-SHA *identity* does not track the same work", with the reason for the qualifier stated inline.
  Generalised from hoodie's point that I searched a **phrasing** rather than a **claim**: `RULES.md` E.7's
  corollary is grep for the claim. `manifest/index.md`'s "never compare them" (git SHA-1 vs sha256 over the
  same bytes) is a different family and was left alone.
- **2026-11-05 · T2 CLOSED (both reviewers, explicitly).** Round four applied three small handoff fixes and
  one rule both reviewers had noticed only as a *tension*. (i) **`RULES.md` § Project gate now names the
  owner-only carve-out.** The gate said "no task is `done` on a run where an applicable step was skipped"
  while § *Prerequisites* says steps 3–6 are owner-run and "a commit names the gate steps it could not run"
  — resolved by precedent at T1 and again at T2, never in text, which neckbeard flagged as the setup for the
  next agent either blocking `done` forever or waiving silently. Written as **four conditions** rather than a
  permission (name the skipped steps in the established form; the step must have been listed owner-only
  *before* the commit that skips it; the diff must contain nothing the skipped step could have exercised; and
  Rule A.6 is not waivable by naming it), anything outside them being `BLOCKED` + `in_progress`. (ii) T3's
  entry path shortened from three hops to one: `audit/index.md`'s existing resume hook now names § 6a as
  T3's first action, instead of a new section nobody scrolls to. (iii) § 6a's heading changed from "before
  T3 opens a single file" to "before any **vendor source** file", which had collided with the same page's
  instruction to read `manifest/index.md` first — two imperatives competing over which file is next. (iv) The
  five-commit slice total is recorded above with its producing command. T2 closes at **five commits**;
  this row's own commit is a sixth and carries only these round-four items.
- **2026-11-05 · T2 round five — neckbeard's MUST-fix on the carve-out it had just approved.** The
  four conditions passed as wording but only three had teeth, and the fourth was the self-waiver door: "the
  task's behaviour is not something only the blocked step can see" is a claim about a counterfactual
  observation, unfalsifiable from inside the commit, so an optimistic agent walks through it while believing
  (1)–(3) did the work. Rewritten so (3) and (4) are **pasted, not asserted**: (3) runs a named command over
  the commit object and prints `FILES=` / `SCOPE_HITS=` / `PATTERNS=`, with `FILES=` mandatory because
  `SCOPE_HITS=0` over an empty list is all-green-from-absent-input; (4) became a **selection** from the four
  behaviour classes RULES already defines, and `none` is legal **only** when (3) printed `SCOPE_HITS=0`,
  binding the one free-floating condition to another condition's machine output. (2) closed a hole neckbeard
  spotted — "before the commit that skips it" is satisfiable by an edit minutes earlier in the same session,
  so the listing must now land in a **strictly earlier commit**; verified with `git log -S`, `owner-run`
  entered RULES at `ef79180` (T0) and the first carve-out use is `3dba113` (T1), so T1's precedent was
  legitimate under the rule that now formalizes it. **Building (3)'s command caught two live bugs, both in
  the rule rather than the code:** the pattern list must be applied after stripping the `server/basalt/`
  prefix, because `git show --name-only` prints monorepo-root-relative paths — unstripped, the real launcher
  commit `b2e5fa4` "[T3][run] add ./run" scored `SCOPE_HITS=0`, i.e. the check would have waved through the
  exact change it exists to catch; and the reviewer-suggested pattern `serve/config.py` **is not a path in
  this project at all** (`git ls-files` → 0), the item-15 phantom path reappearing inside a scope rule.
  Control evidence, all from the commit objects: six T2 commits `SCOPE_HITS=0`; `5f8890d` / `4ca31ae` /
  `b2e5fa4` all `1`; empty input `FILES=0` with `SCOPE_HITS=0`, which is the argument for printing `FILES`.
  Also `FILES=` counts only files inside the project dir, so `9a56c8d` reads 14 in `--stat` and 13 here — the
  monorepo's `context/pitfalls.md` is the difference, stated so the mismatch is not read as drift.

- **Negative results are expected and welcome.** The likeliest outcome of Plan 02 is *"no malware"*.
  That is only useful if the depth *achieved* is stated beside the depth *requested*, and every
  "found nothing" names what it does and does not rule out.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| **Plan 03 — runtime observation** (owner-run): boot + real chat + `/metrics` + image fetch under `--network none` **and** in a monitored netns (`tcpdump`, `ss -tunap`, `strace -f -e trace=network,connect,execve,openat,ptrace` on both `python -m serve.server` and `bin/basalt`); keylog checks (`/dev/input`, `EVIOCGRAB`, XRecord, `uinput`); `strings -a -t x` / `nm -D` / `readelf -d` (DT_NEEDED, RUNPATH, hash every resolved `.so`) on the **shipped** binary; venv `.pth`/`sitecustomize` (U8) | The only non-foolable leg: every static blind spot (`dlopen`+`dlsym`, raw `syscall()`, venv C extensions opening their own sockets, true link deps) closes only here. Split out because Standing Constraints make it owner-run and Plan 01 already stalls on owner boots, so the code verdict must land without inheriting that stall. **It is the gate on Plan 02's verdict, not a formality.** Blocked on an image for the pin — an owner-side fact, since docker is unreachable from the agent session. | Plan 03 |
| **Plan 04 — engine bulk line-by-line**: `src/kernels/` 38,745 · `src/core/` 19,976 · `src/program/` minus `serve/` 15,432 · `include/` 18,224 ≈ 92k lines, 20–30 sessions | The deferred half of "line by line". Plan 02 gives each file a census result and a triage reason; none is read. On the books so the deferral is not invisible. | Plan 04 |
| Hash-pinned deps: `pip-compile --generate-hashes` → `pip install --require-hashes` in our `Dockerfile` | Unpinned wheels mean the audited source is **not provably** the built artifact — the verdict is conditional until this exists. A pin/build change, so its own plan per RULES.md's gate table. | Plan 05-ish (owner) |
| Upstream report: stale `stb_image` 2.21 with JPEG/GIF/BMP CVEs + the `preprocess.cpp` guard question | Now a sourced, dated finding, not a hunch. Upstream's `AGENTS.md` forbids publishing without maintainer approval, so this is a draft for the owner, not an issue. | `context/audit/findings.md` → owner |
| Re-audit policy on `BASALT_SHA` bumps | Upstream is ~2194 commits in six weeks; a verdict at `dc047a7` decays fast. Decide what re-triggers Plan 02's Wave 0. | `context/high_level.md` |
| Model-pack (`.basalt`) weight-file audit | Out of Plan 02 scope; only the loaders are covered. Worth opening only if T7 shows pack metadata can alter server behaviour. | backlog |
| Host hygiene: `docker.sock` group is `root:rtkit` | An odd owner:group for a docker socket, and it is why the agent cannot reach docker. Off-scope for Plan 02 (it is not vendor code) but it surfaced during the audit and the owner may want to look. | owner |
