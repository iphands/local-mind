# Plan 02 — Basalt vendor audit: does `vendor/basalt` leak, steal, keylog, or phone home?

> **Status**: in-progress
> **Created**: 2026-11-05
> **Revised**: 2026-11-05 — same day, before execution: added T0 (gate restoration) and a gate-status block; three Key Facts downgraded after self-check; T5 upgraded from "stale dependency" to a sourced CVE finding; T8 widened to cover the **Python** side of argv assembly; T6 and T11 split against the ≤1,500-line slice rule; canaries 3 → 6 (cross-file, side-channel, second-hop); explicit bypass checklist pre-committed in T4; effort corrected 10–14 → 14–19 sessions.
> **Revised**: 2026-11-05 — **re-scoped onto the owner's question** (owner-approved). After 28 commits no line of Basalt had been read, and T3 had grown five layers of verification-of-verification, the last of which OOMed the host. Cut: evidence-about-evidence work and the bug-hunting tasks that do not answer "does it leak, steal, keylog or phone home". Pulled forward: runtime observation (Plan 03). **§ Re-scope below is authoritative over every task section it names.**
> **Depends on**: N/A (shares Plan 01's `BASALT_SHA` pin; no code dependency — a pin bump invalidates this audit, see Risk 1)
> **Goal**: A per-file, evidenced verdict on whether `vendor/basalt` at the pin does anything beyond acting as a local LLM server — capability-ranked findings, and a finite matrix of every way bytes can leave the box.
> **Agent**: implementation agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## Re-scope (2026-11-05) — authoritative over the task text below

**Why.** The owner's question is whether `vendor/basalt` leaks, steals, keylogs or phones home. At this
re-scope `SERIES.md` read *"Still 0 lines of Basalt read"*; there was no `census.md` and no `verdict.md`,
and T3 had run to seven `evidence-integrity`/`audit-integrity`/`census-tooling` commits: a census, ten
mutant copies to prove it fires, a paste of their runs, a checker of the paste, a self-test of the checker
(`context/pitfalls.md` § *A checker's self-test re-ran the checker unguarded…*). None of it says anything
about Basalt. The census itself had already run clean on `real` with **278 sites, every one assigned a
task** (`wave0/t3-sentinels-real.txt`, `SITES=278`, `SITES_WITHOUT_A_TN` PASS). Its site list was never kept.

**Execution rule 7** is added to § *Execution rules that bind every task*.

| Task | Disposition | Reason |
|---|---|---|
| T0–T2 | done, unchanged | — |
| T2b | **skipped** | Provenance, not behaviour: `src/platform/` is read in T8 whatever its origin. Follow-up |
| **T3** | **narrowed**: re-run `t3-census.sh vendor/basalt real` once; commit its `sites.tsv` as `census.md`'s table; plant canaries **1–3** in a `/tmp` copy, run the same command with `tag=canary` once, record caught or missed | The census ran; the deliverable was never kept. Canaries 1–3 prove the greps see their primitives. **4–6 dropped as census canaries**: side-channel and second-hop are invisible to any grep by design, so they are covered where they can be — T6a reads `/metrics`, Plan 03 watches egress |
| T3 leftovers | **skipped** (rule 7): floor lines, content-leg `chk`, manifest column-6 fill, control re-fires, paste-checker full mode, wave0 file splits | Evidence about evidence. Existing T3 artifacts stay in the tree, frozen; unverifiable claims stay marked (`d349a96`) |
| **T13** | **moved to right after T3** | Where *our* data actually leaves |
| T4 | **narrowed**: read `image_source.py` + `security.py` line by line; the 9-class bypass list → Follow-up | The only code that connects out, so it is read. The bypass list hardens against attackers, not against Basalt |
| T5 | **skipped** → Follow-up | Memory-safety bug hunting in the decoders; does not answer the question |
| T6a, T6c | kept | Where prompts go; keylogging/credential reads directly |
| T6b | **merged into T8** | Same question as the argv table |
| T7 | **narrowed** to the template half (can a pack or request reach `Template`/`from_string`) | ReDoS is denial of service, not a leak. Follow-up |
| T8 (+T6b), T9, T10, T12 | kept | — |
| T11a–d | **one mechanical commit** (`T11`): bidi/zero-width + long-literal scans over `src/` `include/`, exceptions listed and explained | The census already covers every file for primitives; per-file human reasons for ~500 numerical files is bookkeeping |
| T14 | kept, **lighter**: three plain sentences; one matrix row per channel with evidence or "not checked"; coverage = files read ÷ files present | No fixed vocabulary or ≥15-row quota |
| Plan 03 | **runs alongside the reading**, depends on Plan 01 (the image), not on this plan | The most direct test of "phones home"/"keylogs"; an image for the pin exists and docker is reachable |

**Still binding, carried through the re-scope:** T2's three hand-offs in § T3 — external identifiers found
in the code are **never contacted** (no connect, fetch, install or `ls-remote`); "AI residue" is never the
reason a site goes `benign`; and shipped files that instruct an agent (`AGENTS.md`, comments addressed to a
model) are a census surface, which the census's `model-instruction.*` families already cover.

**Order:** T3 → T13 → T4, T6a, T6c, T8, T7, T9, T10, T12, T11 (Plan 03 in parallel) → T14.
Execution rule 1's 1,500-line commit slices stay. Execution rule 2's per-row manifest flips are dropped;
the coverage figure is computed once, at T14, from the list of files actually read.

**Estimate**, measured 2026-11-05 at the pin (`git ls-files … | xargs cat | wc -l`): core reading set
**14,861 lines** — `serve/` Python excluding tests 9,269 · `src/platform/` + `include/basalt/platform/`
3,877 · T9's named files 1,040 · `serve/web/` 675 — plus up to 7,558 in `tools/` where the census routes
there. At this plan's 400–600 lines/h and 3–4 h/session: **7–10 sessions** of reading, **11–14** in all.
`serve/` tests (14,173 lines) are an intent map, not read.

---

## Gate status

Rule A: *"skip is not pass"*, and every commit needs the gate. Recorded per step, with the command
and date, so no session has to guess whether a red step is environmental or theirs. **This block is
the artifact that carries "what the agent could and could not verify."**

| Step | Last run | State | Who can run it |
|---|---|---|---|
| 1a syntax | 2026-11-05 | **pass** (no `FAIL`) — still every tracked file, deliberately not narrowed by the 1b split | agent |
| 1b counts | 2026-11-05 | **pass** — the two expected counts are owned by `RULES.md` § *Project gate* → **Expected counts**; this row cites that owner rather than restating the pair (Rule E.7: a restated count becomes a second authority the day the number moves). This commit's measured pair is in the tracker's dated Evidence row | agent |
| 1c `versions.env` | 2026-11-05 | **pass** (`env ok`) | agent |
| 2 shellcheck | 2026-11-05 | **pass** (`shellcheck ok (product)`) — product set only since the T3 split | agent |
| 2b shellcheck (audit) | 2026-11-05 | **pass** (`shellcheck ok (audit)`) — step added by the T3 split; the hold-out rule was *linted before `git add`*, and `t3-census.sh` and `t3-inputs.sh` each cleared `shellcheck -x` at exit 0 before landing | agent |
| 3 build preflight | 2026-11-05 (re-run at T1) | **BLOCKED** — `PREFLIGHT_ONLY=1 ./build` exit 1: `permission denied … /var/run/docker.sock` | **owner only** |
| 4 `./run` dry run | — | **BLOCKED** (docker) | owner only |
| 5 flags vs image | — | **BLOCKED** (docker, and needs an image for the pin) | owner only |
| 6 server imports | — | **BLOCKED** (docker) | owner only |

**Re-read 2026-11-05, after the host's 03:05 reboot:** an agent session here is in group `docker` (gid 995, socket `root:docker 660`), and steps **3–6 ran and passed** at `d349a96`: `preflight ok`, `dry-run ok`, `flags ok (2 engine, 8 server: 142 engine flags known)`, `imports ok (32 serve modules)` (`wave0/t3-paste-check-runs.txt`). The rows above are the earlier shell's readings and stay. Re-check with `id` before treating a step as owner-only.

**Standing decision (owner, 2026-11-05)**: the owner runs every docker step; the agent writes the
command and records the pasted output. If a step stays unrunnable for a given commit, that commit's
message and the tracker say *which* step was skipped and why — a task whose behavior only a blocked
step can see is **not** `done` (Rule A.6), and this plan expects several such tasks.

**Carve-out citation (Rule A § *Named carve-out*, condition (2)) — recorded once for this plan.**
Owner-only listing: `ef79180` (`RULES.md` § *Prerequisites*, T0). First commit using the gate line's
`3–6 docker-blocked (owner)` form: `3dba113` (T1). The carve-out itself was written in `d388d17`, so T1
and T2 used the form **before** the rule existed; checked against the rule as written, they are compliant
rather than retro-fitted, because `ef79180` strictly precedes `3dba113`
(`git log --oneline -S'owner-run' -- context/plans/RULES.md` → `ef79180`; `-S'docker-blocked'` → `3dba113`).
First invocation **under** the rule: `aab418e`, whose scope evidence is `FILES=3 SCOPE_HITS=0`. Later
commits in this plan cite this line instead of re-deriving it; re-verify only if § *Prerequisites* is
edited.

**Re-run at T1 (2026-11-05, commit `[basalt][P2][T1][audit] …`)**: steps 1a (no `FAIL`), 1b
(`3 b` then-current — the plan's `.md`/`.tsv`/`.sha256` artifacts add nothing to the bash set, which is Execution
rule 5 holding in practice), 1c (`env ok`) and 2 (`shellcheck ok`) all pass with `context/audit/`
present; verbatim in `context/audit/wave0/t1-freeze-and-manifest.txt`. Step 3 fails identically to the
row above. Steps 4–6 remain unrun for every T1 commit.

---

## TL;DR

**What**: Audit all 862 tracked files of `vendor/basalt` at `dc047a7` for leak / theft / keylogging / off-box transfer — with a primitive census that bounds the search, **six** planted canaries that prove the census can see what it claims to see, line-by-line reading of everything that touches a boundary, and a finite "every way bytes leave the box" matrix the verdict is phrased against.

**Deliverables**:
1. `context/audit/manifest/` — per-file manifest, one TSV per top-level directory (fixed schema below): the artifact that makes "file by file" countable.
2. `context/audit/census.md` — every boundary call site, each dispositioned and each **assigned a task number**; plus the 6-canary run.
3. `context/audit/argv-table.md` — one row per exec/spawn/`dlopen` site in **C++ and Python**, with the per-element origin of every argument. The load-bearing artifact of the engine half (T8).
4. `context/audit/findings.md` — ranked by **capability, not intent**: 🔴 can leave the machine / 🟠 needs a second mistake / 🟡 local-only / ⚪ hygiene; each with what it does, why it isn't scary, what would make it scary, the one action.
5. `context/audit/coverage_ledger.md` — lines read vs lines present, per tier, **pasted from a command**.
6. `context/audit/verdict.md` — three plain sentences answering the literal question, the channel matrix, the depth table, and "if you do only one thing".
7. `context/audit/index.md` — the entry point, declaring the tree each artifact describes (pre-empts the 40 KB split rule).
8. Our own posture (`./run`, `Dockerfile`) as an action list — T13, in scope by owner decision.
9. **Plan 03** (owner-run runtime proof — the gate on this verdict) and **Plan 04** (the deferred engine bulk) registered in SERIES.

**Estimated effort**: **14–19 sessions.** Tier A is ~34,000 lines; at 400–600 LOC/h of genuine adversarial reading that is ~57–85 hours of reading alone, and at a realistic 3–4 hours of that work per session it lands at 14–19 sessions **before** gate overhead and **before** waiting on the owner for the docker steps and the T5 fuzz build. An earlier estimate of 10–14 was ~1.5× optimistic and is replaced. Plan 04 (the engine bulk) is a further ~92,000 lines / 20–30 sessions.

---

## Scope

**In**: `vendor/basalt` at `dc047a77beeddf1e91234d983a91d6bf929e9619`, byte-verified against the commit by T1 — all 862 tracked files. Plus, by owner decision, our own `./run`, `lib/common.sh`, `Dockerfile`, `versions.env` (T13).

**Out** (registered, not silently skipped):
- **Engine bulk line-by-line** — `src/kernels/` 38,745 l · `src/core/` 19,976 l · `src/program/` minus `serve/` 15,432 l · `include/` 18,224 l. In this plan each gets the census, a vendored-dependency check, a unicode/stego scan and a **recorded per-file reason**; none is read line by line. That is **Plan 04**.
- **Runtime observation** (strace/tcpdump/netns, keylogger probes, `strings`/`nm`/`readelf` on the built binary, venv hook files) — **Plan 03**, owner-run. See Risk 4.
  *(Re-scope 2026-11-05: still out of **this** plan, but no longer last — Plan 03 runs alongside the reading, against the built image.)*
- The model pack's weight **bytes** (its loaders that parse them are in scope). Upstream commits after the pin. Anything reachable only through the CUDA driver or GPU we do not control.

> **Scope condition on the verdict — this repository has single-reviewer provenance everywhere.**
> Measured 2026-11-05: **0 of 14** sampled merged PRs carry any review artifact, only **8.1 %** of the
> 2,194 commits carry a PR suffix at all, and our own pin reached `main` as a fork PR merged in
> **212 seconds** with no review (`wave0/t3-github-pr-metadata.txt` §§ 2, 4). So "no formal review
> artifact" is **not** a finding about any individual commit, file or contributor here and must not be
> used as one; it is a property of how this repository is run, and it applies uniformly to every line
> T3–T14 reads. The correct use is the inverse: because no review beyond one person's eyes stands
> behind any of it, **the reading done in this plan is the only review these bytes will ever get**,
> which raises the required depth rather than singling anything out. Do not soften the 212 s into
> "same-day", and do not let a contributor's account age answer the process point — it answers the
> *identity* point only.

**Depth decision — owner-attributed** (2026-11-05; the owner asked for "file by file, line by line", was shown that blanket line-by-line over `src/`+`include/` is 40–70 focused sessions, and chose):

> **Two-stage: boundary-first now, kernel sweep as a follow-up you can start later.**

**Tiers** — every manifest row carries exactly one; this is what stops "grep found nothing" being laundered into "audited":
- **Tier A** `read@T<n>` — every line read.
- **Tier B** `triaged:<reason>@T<n>` — not read line by line; the row names why (`no-primitives`, `numerical-kernel`, `vendored-third-party`, `test-only`, …).

### Execution rules that bind every task

1. **≤1,500 lines of Tier A reading per commit-slice.** A task larger than that is split into lettered subtasks (`T6a`/`T6b`/`T6c`, `T11a`–`T11d`), **one commit each**, each flipping its own manifest rows. No single-commit 8,000-line tasks.
2. **Tick unit ≠ commit unit**: flip *all* the manifest rows a slice covers in that slice's commit.
3. **Never write inside `vendor/basalt/`.** `./build` refuses a dirty checkout, *untracked files included*, so a scratch artifact there breaks gate step 3 for every later commit. Scratch work lives in `/tmp/opencode/basalt-audit/`.
4. **Commands end with an unconditional sentinel** (`; echo "— done rc=$?"`) so an empty result is distinguishable from a failed command. See `context/pitfalls.md` — this plan already lost one Key Fact to the opposite mistake.
5. **Audit commands are written inline** (plan/tracker/Evidence), never as helper scripts: gate step 1 classifies by **shebang first**, so even an extensionless `#!/usr/bin/env bash` helper in `context/` counts toward the gate's "Expected counts" line — owner: `RULES.md` § *Project gate* → **Expected counts** — and a count above it without a matching RULES bump fails the gate. If a helper is unavoidable it lands with a bump to that RULES.md paragraph in the same commit.
6. **Every `context/audit/` file opens with a stamp:** `Target dc047a7 / tree 3f7b69ae — if that is not the tree you are looking at, you are in the wrong audit.`
7. **Evidence is one command and its output, pasted once** (re-scope, 2026-11-05). No checker for a paste, no self-test for a checker, no re-verification of a control already shown to fire. A negative control is required only for a *new detector*, run once, recorded once. A review finding about the *evidence layer* is noted in the tracker and fixed only if it changes a finding about Basalt or a figure the verdict cites. **Stop rule:** a task that reaches 3 commits without reading or running Basalt code stops and asks the owner before a fourth.

**The one freeze-check** (used by Resume step 1 and by every wave exit — there is deliberately only one).
**`git diff` is not the tree.** `server/basalt/.gitignore` line 1 is `/vendor/`, so anything a probe
leaves inside `vendor/basalt` is untracked *and* ignored, and `git diff <pin>` structurally cannot see
it. Thirteen `__pycache__` files that this plan's own PoC harness wrote sat behind a string of
`AUDIT TREE OK` lines on 2026-11-05 and surfaced only as the census universe hash (875 files against a
pinned 862). Three legs, all required — contents, file count, and anything git calls untracked or
ignored:

```bash
cd vendor/basalt
GIT_NO_LAZY_FETCH=1 git diff --quiet dc047a77beeddf1e91234d983a91d6bf929e9619 \
  && echo "AUDIT TREE OK (tracked contents)" || echo "AUDIT TREE DRIFTED — STOP"
N=$(find . -type f -not -path './.git/*' | wc -l)
[ "$N" -eq 862 ] && echo "TREE FILE COUNT OK (862)" || echo "TREE FILE COUNT=$N — untracked additions — STOP"
U=$(git status --porcelain --ignored | grep -c .)
[ "$U" -eq 0 ] && echo "TREE CLEAN (no untracked/ignored)" \
  || { echo "TREE NOT CLEAN: $U entries — STOP"; git status --porcelain --ignored | head; }
cd ../..
```

The count leg is the one that catches a *deleted* tracked file's replacement or a stray artifact; the
status leg names the offending paths, which the count alone cannot. `862` is the frozen universe, owned
by `manifest/*.tsv` row count and by `t3-census.sh`'s `UNIVERSE_COUNT_EQ_PIN` — if you bump the pin,
all three move together.

---

## Context

The owner's question, verbatim: *"I want to look file by file, line by line for any malicious code.... is this project doing anything to leak, steal, keylog, send data anywhere on the network that is NOT just the project acting as a nice LLM server/runner?"*

Recon says **clean, actively maintained, security-conscious**: 2194 commits 2026-09-24→2026-11-04, ~30 contributors, no network library in our build files, a deliberately SSRF-guarded image fetcher whose own tests attack it, SHA-pinned CI with no secrets. So the failure mode this plan must avoid is not "missed backdoor", it is **(a)** declaring an all-clear via a search structurally incapable of finding the real risk, and **(b)** spending 40–70 sessions on quantised-matrix kernels while the actual exposure sits in a 2019 image decoder and an optional API key.

### Pre-Identified Bug/Issue

**1 — CONFIRMED FINDING (P0): a vendored image decoder fed by request bytes, with a ≥30× memory amplification and no per-decode memory ceiling.**

> **Corrected 2026-11-05 — the version claim in every earlier draft of this section was FALSE,
> and so was its evidence.** This section said the copy was **2.21 (2019-02-25)** and that
> "`STB_IMAGE_RELEASE` is absent because that macro first appears in 2.26 — the missing macro
> *is* the version evidence." **`STB_IMAGE_RELEASE` is not a macro in any stb_image release**;
> stb records its version only in the banner comment and the changelog (the one version-ish
> macro here is the unrelated ABI constant `STBI_VERSION 1` at `:374`), so a 0-hit grep on it
> carried *no* information. The `sed -n '60,62p'` "quote" landed **mid-list** on the
> 2.23/2.22/2.21 rows of the `RECENT REVISION HISTORY` block, whose top entry (line 52) is
> 2.30. Method failure recorded in `context/pitfalls.md`. The falsified claim is kept here, in
> this note, so nobody re-derives it. Everything below that argued CVEs are missing *because the
> copy is old* is void — see the replacement table.

`vendor/basalt/src/vision/stb_image.h`, 7,988 lines, `sha256 594c2fe35d49488b4382dbfaec8f98366defca819d916ac95becf3e75f4200b3`.
It is **2.30 (2024-05-31)** — the banner and the head of its own history block, and a floor of
**≥2.27 provable from code rather than comments**:

```console
$ grep -n 'stb_image - v' vendor/basalt/src/vision/stb_image.h
1:/* stb_image - v2.30 - public domain image loader - http://nothings.org/stb

$ sed -n '50,54p' vendor/basalt/src/vision/stb_image.h        # TOP of RECENT REVISION HISTORY
      2.30  (2024-05-31) avoid erroneous gcc warning
      2.29  (2023-05-xx) optimizations
      2.28  (2023-01-29) many error fixes, security errors, just tons of stuff

$ grep -n 'define STBI_MAX_DIMENSIONS\|stbi__pnm_is16' vendor/basalt/src/vision/stb_image.h
796:#define STBI_MAX_DIMENSIONS (1 << 24)     # introduced 2.27
963:static int      stbi__pnm_is16(stbi__context *s);   # 16-bit PNM, attributed 2.27
7624:static int stbi__pnm_is16(stbi__context *s)
```

Identity to a pristine upstream 2.30 is **not** established offline — only the banner, the
changelog and the structural markers, plus this `sha256` (the only `stb_image.h` on this disk,
so there is no local oracle). Comparing that hash against upstream's `v2.30` tag blob is an
**owner step**; a fork may also patch a vendored header, so hash-equality is not guaranteed even
at a true 2.30. `stbi__pnm_is16` proves the *file* is ≥2.27 but is `#ifdef`-ed out of this
binary; the ≥2.27 floor that is *live* comes from `STBI_MAX_DIMENSIONS`.

It is compiled into the engine and fed remote-supplied bytes:

```console
$ sed -n '7,14p;34p' vendor/basalt/src/vision/preprocess.cpp
#define STB_IMAGE_IMPLEMENTATION
#define STBI_ONLY_JPEG
#define STBI_ONLY_PNG
#define STBI_ONLY_BMP
#define STBI_ONLY_GIF
#define STBI_NO_STDIO
#include "stb_image.h"
    uint8_t* px = stbi_load_from_memory(data, n, &w, &h, &c, 3);
```

Path in: request `image_url` → `serve/engine/image_source.py` (32 MiB / 60 s) → `src/program/serve/vision.cpp:45` → `basalt::vision::decode_image` → `stbi_load_from_memory`.

Which fixes matter **depends on which decoders are compiled in**, and four are (`STBI_NO_STDIO` also guarantees no decoder opens a file):

| Compiled **in** | Status of the post-2019 work once credited to "the copy is old" (re-verdict 2026-11-05) |
|---|---|
| **JPEG** | CVE-2022-28041, CVE-2022-28042 (fixed 2.28) and the nothings/stb#1178 resampler OOB (post-2.26) are **inherited as fixed by construction** — this copy is 2.30. Not independently re-verified here. |
| **GIF** | GHSL-2023-145/148/150 were reported **against 2.28**, i.e. before this copy. Of the four formats this is the one whose per-frame descriptor could disagree with the screen descriptor, so it is the most legitimate residual concern — but it is 2.28+, not 2.21. |
| **BMP** | the 2.28 `psize` hardening is **present in the shipped file**, at two sites: `:5564` `if (psize == 0) {` and `:5601` `if (psize == 0 \|\| psize > 256) { STBI_FREE(out); … }`; the 2.28-era `extra_read` field is at `:5419/:5458/:5496`. |
| **PNG** | the 2.28 IDAT limit is present (`IDAT section larger than 2^30` string at `:5182`). |
| Compiled **out** | TGA, PNM/PGM, PSD, PIC, HDR, PBM — so CVE-2021-42715 (HDR), CVE-2021-42716 (16-bit PGM OOB read) and GHSL-2023-147 (TGA uninitialised disclosure) **do not apply to our build**. Do not re-raise them. |

The honest claim is therefore narrow and checkable: **the copy is 2.30, the latest public stb_image release, so whatever upstream fixed by ≤2.30 is fixed here; whatever is still open upstream as of 2.30 is inherited here.** Per-CVE adjudication offline was not done and must not be asserted either way.

Mitigating, and genuinely good code — hoodie read it before I did: `preprocess.cpp:24-33` calls
`stbi_info_from_memory` and refuses on `(int64_t) w * h > max_pixels` **before any pixel buffer
exists**, with an actionable message, and `serve/engine/vision.py:25` mirrors the same
`IMAGE_MAX_PIXELS = 64 << 20` as `kMaxImagePixels` (`preprocess.hpp:37`). Four nested ceilings
bound the damage: **32 MiB** compressed per image (`image_source.py:30`), **256 MiB** per request
body (`handler.py:33`), **64 MP** declared pixels, `1<<24` per side (stb's own default, never
overridden here). So: **can 32 MiB become gigabytes? Yes, roughly 1 GiB — and no, not
unboundedly.** What is missing is not a version bump but (a) a **re-check of the decoded
dimensions**, since the guard trusts the header while `:41` sizes `out.rgb.assign(px, px + w*h*3)`
from the dims `stbi_load_from_memory` rewrote at `:34`, (b) any **per-decode memory ceiling or
timeout** (`STBI_MALLOC` is not overridden, so a guard-legal but expensive image costs what it
costs, on a box whose design is system RAM holding the expert tier), (c) `preprocess.cpp:20`
`const int n = (int) std::min<size_t>(len, 0x7fffffff)` which **silently truncates** an oversized
buffer instead of refusing it, and (d) `stbi_set_flip_vertically_on_load*` **never being called**
(0 call sites outside the vendored header) — if the encoder/mmproj path expects bottom-up, every
image is vertically mirrored, and neither a fuzzer nor the PPL/KLD gate would notice without a
vision parity case. `fit_size` (`:46-64`) has a further resource overshoot at extreme aspect
ratios; it is **unmeasured** and is recorded under T5, not here.

**2 — A config-supplied shell command runs with `shell=True`** — operator-side today; it becomes a backdoor only if config is influenceable from a request:

```console
$ sed -n '248,251p' vendor/basalt/serve/service.py
            try:
                subprocess.run(cmd, shell=isinstance(cmd, str), timeout=120, stdin=subprocess.DEVNULL)
            except (OSError, subprocess.SubprocessError) as e:
                print(f"[server] warning: before_load command failed ({e}), loading anyway", flush=True)
```

**3 — Dependencies are name-pinned, not hash-pinned** (8 `==` lines, no lockfile), and our `Dockerfile` installs them with plain `pip install`. *"No network in the code" is trivially true and irrelevant if an unpinned wheel does the work* — so the verdict is **conditional** and the condition goes in its headline, not its footnotes (T12).

### Why census-bounded, then tiered reading

A hand-picked danger grep has unprovable completeness, so its output is indistinguishable from a missed backdoor. The search is therefore bounded from both ends: a **forward census** (every import in shipped Python, every external dependency in our build files, then read each flagged site — completeness from enumeration, not guesswork), and **six canaries** (planted artifacts the census must catch; without them "we found nothing" is unfalsifiable).

**Rejected — blanket line-by-line over all 190k lines.** 300–450 focused hours on `src/`+`include/` alone, most of it on kernels with no boundary in them. The owner chose two-stage over it.

**Rejected — the pure structural argument.** "No network library links into `bin/basalt`, therefore it cannot phone home" is a beautiful one-command answer and **insufficient**: `nm -D --undefined-only` under-reports (raw `syscall()`, or `dlopen`+`dlsym`, never appear undefined), and says nothing about venv C extensions (`psutil`, `pillow`) opening their own sockets, or `.pth`/`sitecustomize` hooks. Kept as evidence, never as verdict.

### Key Facts

Values are as measured; the **How confirmed** column states the method *and its limit*, because two of these rows were downgraded after I checked my own commands.

| Fact | Value | How confirmed (and its limit) |
|---|---|---|
| Pin under audit | `dc047a77…` = `main` 2026-11-04; tree `3f7b69ae4054969ed15b9b6f02d306eb44e0ebe4` | `git rev-parse HEAD HEAD^{tree}` 2026-11-05 |
| **The pin = basalt's own `v0.2.1` + docs + `serve/`; the C++ engine is byte-identical to that tag** | 3 commits, 9 changed files, **0** under `src/`/`include/`/`cmake/`/`tools/`/`CMakeLists.txt`; engine blobs 553/553 shared, symmetric diff 0 | T2 2026-11-05, local `ls-tree` blob identity + `comm`; independently confirms Plan 01's row (GitHub `compare`), and a third method agrees (`v0.2.1..pin = 3` by local `rev-list` in the one-store probe, `audit/wave0/t2-lineage.txt` § 11b). **Consequence:** Plan 01's engine characterisation carries to this pin; the stalest read is `CMakeLists.txt` (287 commits) |
| **The two upstream names are not comparable** | `jesdga95/basalt` and `Niko1221/Strata` share **0** commit SHAs, have **no merge-base**; 844 commits match on `(tree, author-date, committer-date, subject)`. Both publish `v0.1.0`/`v0.2.1` at **different** objects (Strata: 95 tags to `v0.2.10`) | T2 2026-11-05 vs a pristine `--filter=tree:0` fetch. **Never diff these repos by SHA**; unqualified `v0.2.1` in this repo means **basalt's**. `audit/wave0/t2-lineage.txt` |
| **The audited history self-identifies as substantially AI co-authored** | **1,907 / 2,194 commits (86.9 %)** carry an AI co-authorship trailer; **19,682 / 20,111** blamed lines (97.9 %); **Canonical figures, owned by** `wave0/t2-provenance.txt` § 10; evidence `wave0/t3-ai-trailer-classes.txt`; upstream's rewritten line shows **8 / 5,637** | `git log -z --format='%H\t%B'` + blame ∩ trailer set, T2 2026-11-05. **Disclosure, not authorship**; ranks nothing by itself; the upstream figure is **not** a human-written control (`audit/wave0/t2-provenance.txt` § 10) |
| **Tier A is not all text, and the plan's T2 file list is not the manifest's** | the census is 87 rows = **80 blameable first-party** (69 `A-planned` + 11 `unassigned`) + 6 binary Tier A assets + 1 vendored `stb_image.h` (`B-vendored@T5`); the 12 census paths beyond the manifest's 75 Tier A are `CMakeLists.txt` + ten `serve/engine/` modules + `stb_image.h` | T2 2026-11-05. `CMakeLists.txt` and the ten `serve/engine/` modules are still **`unassigned` in the manifest** — they need a `File:` amendment to be owned, and until then **T11 owns zero `A-planned` rows** |
| **The audited repo publishes a second, unaudited branch** | `refs/remotes/origin/multimodel`, 105 commits ahead of the pin (merge-base `1b3f6964`); clone object hygiene otherwise clean: 2,299 commit objects, present-but-unreachable **0** | T2 2026-11-05 `for-each-ref` + `cat-file --batch-all-objects` set-minus `rev-list`. Every "the whole repo is accounted for" claim must name its ref; goes to `verdict.md` conditions and T13 |
| Working tree matches the commit | `git diff --quiet HEAD` → clean | 2026-11-05. **Limit:** run *without* `GIT_NO_LAZY_FETCH`, so it does not prove blobs were local — only that worktree == index == commit. T1 re-proves both under `GIT_NO_LAZY_FETCH=1`; `git cat-file -e` proves object *existence*, never that the working file matches it (only `git hash-object` does) |
| No hidden object source | no `.git/objects/info/alternates`, no `core.objectsfile` | 2026-11-05 |
| Partial clone | `promisor=true`, `partialclonefilter=blob:none` | `git config --local --list` 2026-11-05. A partial clone cannot **corrupt** bytes; the risk is silent skips and mid-audit drift, hence the one freeze-check |
| Size | 862 files. `src/` 353 f/106,928 l · `include/` 161/18,224 · `serve/` **41/10,431** · `serve/tests/` 53/14,173 · `tests/` 163/51,125 · `tools/` 36/7,938 · `cmake/`+`CMakeLists.txt` 3/1,569 · `data/` 5 | `git ls-files <d>/ \| xargs cat \| wc -l` 2026-11-05 |
| Boundary areas | `src/platform/` 10/2,921 · `src/program/serve/` 33/8,465 · `src/vision/` 10/9,572 (**7,988 = the vendored stb**) · `src/kernels/` 86/38,745 · `src/core/` 72/19,976 | same |
| Our image ships `bin/ serve/ tools/ data/` | 135 shipped files vs 718 build-time | `Dockerfile` `COPY --from=build` lines, read 2026-11-05 |
| **No network library appears in our build files** | no `FetchContent`/`ExternalProject`/curl/OpenSSL/`ws2_32`/`wininet`/asio in `CMakeLists.txt`+`cmake/`; only `cudart`, `cuBLAS` | build-file grep 2026-11-05. **Downgraded wording:** this is a *build-file* claim. Link truth (`readelf -d`, DT_NEEDED, RUNPATH, hashing every `.so`) is **Plan 03** |
| Exec/primitives concentrate in the sanctioned seam | `src/platform/dynlib.cpp:55` `dlopen("libnvidia-ml.so.1")`; `process.cpp` `CreateProcessW`/`fork`/`execvp`; `system.cpp:44` `capture_output()` = `popen` of a `shell_quote`d argv, called only from `src/program/bench_main.cpp:334,640,1048` | grep 2026-11-05 over `src/`+`include/`. **Limit:** that grep set omitted `posix_spawn`, `execl/execlp/execle/execvpe`, `os.exec*`/`os.system`/`os.spawn*`, and any non-exec `fork` child — T8 redoes it wide and covers `serve/` too |
| **The argv is assembled in Python, consumed in C++** | `serve/engine/lifeline.py:122` `popen`, `serve/engine/process.py:265` `lifeline.popen([engine_bin, "--serve", *args])`, `serve/engine/vision.py:344` `[engine_bin,"--gpu-plan",…]`, `serve/service.py:250` `shell=isinstance(cmd,str)` | read 2026-11-05. Neither half alone answers "can a request reach an `argv`" — **T8 owns the joint proof** |
| Engine env reads (C++ only) | `CUDA_VISIBLE_DEVICES`, `CUDA_MODULE_LOADING` | `getenv(` over `src/`+`include/` 2026-11-05. **Limit:** `serve/`'s `os.environ`/`os.getenv`/`platform.node`/`gethostname` never swept — T3 adds that section |
| One outbound channel in the server | `serve/engine/image_source.py`; `OpenerDirector` with *no* proxy/redirect/ftp/file handler; hand-written non-public v4/v6 blocklist incl. `169.254.169.254`; 32 MiB / 60 s; UA `local-server` | read 2026-11-05 |
| **`connect()` uses the pre-checked literal addresses — it does not re-resolve** | `_PinnedConnection.connect()` iterates `self.addresses` ("the addresses that were checked") calling `_connect(ip, self.port, left)`, while the Host header and TLS name/cert check stay on the original hostname (`image_source.py:422-460`) | read 2026-11-05. **This largely answers U5** — rebinding-by-re-resolution is structurally handled; T4 still must prove the address *list* is produced only from validated literals and test the non-DNS spoof routes |
| No build-time execution hooks in the repo | no `setup.py`/`setup.cfg`/`pyproject.toml`/`MANIFEST.in`/`package.json`; 8 pinned names, no hashes | `git ls-files`, `cat requirements.txt` 2026-11-05. Image venv hooks are **unknown** → Plan 03 |
| No git supply-chain vector | no `.gitmodules`; `.git/hooks` samples only, no `core.hooksPath`; `.gitattributes` only `eol=lf`/`-text`, no `filter=`/`smudge=` | 2026-11-05 |
| CI is hardened | every action SHA-pinned, `persist-credentials: false`, `permissions: contents: read`, no secrets | `cat .github/workflows/*.yml` 2026-11-05 |
| Profile path is concatenated | `src/program/generate/configure.cpp:40` `std::string(BASALT_SOURCE_DIR) + "/data/profiles/" + key + ".prof"` | read 2026-11-05 → T9 proves `key` is not request-reachable |
| **No by-name GPU peer/remote references** | zero `nccl\|nvshmem\|cudaIpc\|remote_expert` tree-wide; removal commit `97ff061` "engine: remove the dead remote-expert tier, its reduction option and kernels" | name-grep 2026-11-05. **Downgraded wording:** a four-string name grep cannot see `cudaMemcpyPeer`, `cudaDeviceEnablePeerAccess`, `cuMemExportToShareableHandle`, `libfabric`, or a dead tier's address field surviving under a new name. T9 greps those APIs *and reads `97ff061` as a diff* |
| Cross-card sharing is hand-rolled, no NCCL | `src/program/serve/loans.cpp`, `loan_moves.cpp`, `src/program/multi_gpu_plan.cpp`, `src/program/image_loan.cpp` | `git ls-files \| grep -iE 'multi_gpu\|loan'` 2026-11-05 |
| Model files are not executable input | GGUF parsed by a hand-written minimal v3 reader (`tools/gguf_reader.py`); no `pickle`/`torch.load`/`np.load`; no `nvrtcCompile`/`cuModuleLoad` | greps 2026-11-05 |
| **Docker is unreachable from the agent session** | socket `root:rtkit 660`; session `uid=1000 groups=1000,27`; group `docker` = gid 991, no members; `PREFLIGHT_ONLY=1 ./build` exit 1 | 2026-11-05 → gate steps 3–6 are **owner-only**, so *whether an image for the pin exists is an owner-side fact we do not have* |
| `shellcheck` was missing at plan creation | RULES.md recorded it at `/usr/local/bin` (0.11.0, 2026-11-04); no binary there or on PATH. **T0 fixed it**: official v0.11.0 release tarball (sha256 `8c3be12b…`, contents = LICENSE/README/`shellcheck` only) installed to `~/.local/bin`, verified `--version` + a negative control that fired SC2115/SC2086 | 2026-11-05 |

### Rejected Claims

Looked like findings, are not — do not re-investigate.

- **`serve/util/telemetry.py` is not telemetry.** NVML-via-ctypes + psutil/`/proc` sampling for the local `GET /metrics`, background thread, **no socket import**. Misleading name. T6a still owns whether `/metrics` *discloses* it remotely. (Its inverse is a real search target — see T3 "diagnostics-shaped".)
- **`__threadfence_system()` is not `system()`.** Hundreds of naive `system(` hits in `src/kernels/cuda/*.cuh` — it is a CUDA fence. Use `\bsystem\(`.
- **A partial clone cannot corrupt bytes.** `blob:none` fetches lazily; materialised blobs are content-addressed. The risk is silent skips and drift.
- **The `data/*.bin` blobs are gone.** `0efaa0a` "data: drop the shipped profiles and draft vocabulary"; **0 `.bin` files at HEAD**.
- **The base64 sites are not obfuscation.** `serve/api/responses.py:91,403` = the OpenAI Responses `encrypted_content` round-trip; `image_source.py:167` = `data:` URL decode. Confirm in T6a; a `b64decode` hit is not a payload.
- **`docs/community/benchmarks.json` is not server input.** Written by `basalt-bench`, validated by `tools/check_benchmarks.py` in the `pages` workflow; no `serve/` module reads it. Confirm once in T12.
- **The TGA/PNM/HDR/PSD stb CVEs do not apply to our build** — those decoders are `STBI_ONLY_*`-excluded. Only JPEG/PNG/BMP/GIF are live.

---

## Step-by-Step Tasks

**Wave 0 exit gate**: tree frozen and manifest complete over all 862 files (0 unaccounted); **6/6 canaries caught**; the census's flagged-site table each carrying a task assignment; the pin proven against public upstream; and **T9's file list frozen to a named list with a line count**. No reading task starts before all five hold.

### T0: Restore the project gate

**File**: `context/plans/RULES.md` (fix the stale prerequisite in place — house style: stale knowledge is corrected where it lives, dated)

**What to do**: *(mostly done at plan creation, owner-approved)* `shellcheck` v0.11.0 installed user-local to `~/.local/bin` from the official GitHub release; record the path change in RULES.md § Project gate step 2 and § Prerequisites, dated, with the hash and the negative-control result. Then state plainly, in the Gate status block above, that steps 3–6 are owner-only for this session and why. If the owner later declines to keep it, replace the step-2 row with: *"gate knowingly degraded for P2 — owner decision, dated"*. That sentence, not silence, is the deliverable.

**Verify**: `shellcheck --version` → `version: 0.11.0`; gate step 2 → `shellcheck ok`; RULES.md names `~/.local/bin/shellcheck`.
**Expected observation**: **Confirms if** step 2 passes and RULES.md no longer asserts a path that does not exist. **Refutes if** the binary vanishes again mid-plan (then every later commit is knowingly degraded — say so). **Within noise if** a different minor version, if it is ≥0.11.0 and the recorded reason names it.

**Commit**: `[basalt][P2][T0][gate] restore shellcheck user-local and correct the gate prerequisite`

### T1: Freeze the target and manifest all 862 files

**File**: `context/audit/index.md`, `context/audit/manifest/<top-level-dir>.tsv`

**What to do**: Emit one row per tracked file with a **fixed, headerless schema** — column order is `path TAB blob-id TAB sha256 TAB LOC TAB tier TAB primitives TAB wave TAB status TAB reviewed-in`. **No header row** (so `wc -l` equals the file count and `cut -f1` is a clean path set). Blob ids are SHA-1 over git's `blob <len>\0` framing — a *different* identifier from the sha256 column, never to be compared with it. Record `BASALT_SHA`, `HEAD^{tree}`, the `remote.origin.*` config, and the `alternates`/`objectsfile` absence. Prove materialisation loudly under `GIT_NO_LAZY_FETCH=1`. `index.md` declares the artifact tree, the stamp line, and the tree these files describe.

**Verify** (note the two `cd`s — the manifest lives in the *project*, not in `vendor/`):
```bash
# SUPERSEDED IN PART (2026-11-05, T1 follow-up): these commands report loop rcs, not results, and the
# `wc -l … | tail -1` row count under-reads if a TSV ever loses its final newline. Run instead the
# counted-sentinel set in `context/audit/manifest/index.md` § Checks plus
# `context/audit/scripts/ledger-grid.sh` (figures) and `context/audit/wave0/t1-grid-controls.txt`
# (its negative controls). Kept verbatim below as this task's original wording — do not run it as the check.
cd /home/iphands/prog/ai/local-mind/server/basalt/vendor/basalt
GIT_NO_LAZY_FETCH=1 git ls-tree -r dc047a77 --format='%(objectname) %(path)' \
  | while read -r h p; do [ "$(git hash-object -- "$p")" = "$h" ] || echo "MISMATCH $p"; done; echo "— blob check done rc=$?"
GIT_NO_LAZY_FETCH=1 git diff --quiet dc047a77 && echo "AUDIT TREE OK"; echo "— rc=$?"
cd .. && git status --porcelain --ignored | head; echo "— vendor status rc=$?"
cd /home/iphands/prog/ai/local-mind/server/basalt && wc -l context/audit/manifest/*.tsv | tail -1; echo "— expect 862 total"
```
**As executed (T1, 2026-11-05)** — two things the task text left open, now decided where a later
session will look:

1. **Tier A was seeded at T1 from this plan's own `File:` fields** (`A-planned@T<n>`: 75 rows /
   19,659 l), plus `src/vision/stb_image.h` as `B-vendored@T5` (1 row / 7,988 l) — the one Tier B
   assignment the plan already justifies. **Everything else (786 rows / 191,600 l) was left
   `unassigned`** for T3. The
   asymmetry is deliberate: Tier A membership is already evidenced by `### T4/### T5/…` above, whereas
   a Tier B reason is not, and a directory-level Tier B default would be an unevidenced claim. Seeding
   before the census runs is what stops a negative census result from reclassifying a boundary file —
   hence the invariant, written in `context/audit/manifest/index.md`, that **an `A-planned@T<n>` row
   may only ever become `read@T<n>`**.
2. **The Verify commands above report loop rcs, not results.** Run the printed-count forms —
   `MISMATCH_COUNT=`, `VENDOR_DIRTY=`, `PREFIX_DRIFT_COUNT=`, `awk 'END{print NR+0}'` for the row total
   (not `wc -l … | tail -1`, which reports a single file's count if the glob ever matches one file, and
   under-reads by 1 if a TSV loses its final newline). Full set, with a negative control on the
   hand-edit check: `context/audit/manifest/index.md` § Checks; raw output in
   `context/audit/wave0/t1-freeze-and-manifest.txt`.

**Expected observation**: **Confirms if** 0 `MISMATCH`, `AUDIT TREE OK`, vendor porcelain-clean, 862 rows. **Refutes if** any mismatch, any file that *fails* under `GIT_NO_LAZY_FETCH=1` (that is the silent skip this exists for), or row count ≠ `git ls-files | wc -l` — blocker, not drift. **Within noise if** nothing. One mismatch is a blocker.

**Commit**: `[basalt][P2][T1][audit] freeze the vendor/basalt pin and manifest all 862 files`

### T2: Prove the pin is public upstream's — and who authored the boundary files

**File**: `context/audit/manifest/index.md` (provenance section)

**What to do**: Confirm `dc047a7` is what public `jesdga95/basalt` `main` says it is — checked **against GitHub, never against our own clone** (Rule E) — by comparing GitHub's tree SHA to our `HEAD^{tree}`; check tags (`v0.2.1` is the nearest release per `versions.env`) and whether any are signed, recording "GitHub offers no signature" as a **limit** rather than a clean result; confirm no local commits or untracked files in vendor. **Then the cheap, disproportionate-signal half** (~30 contributors in six weeks — do not treat HEAD as one author): `git log --format='%ae %ad' -- <boundary file>` for `serve/engine/image_source.py`, `serve/security.py`, `serve/service.py`, `src/platform/*`, `src/vision/*`, `data/*`, `serve/web/*`. Flag any boundary file whose history includes a **one- or two-commit contributor** or an email domain off the project's cluster (`gmail`/`users.noreply`/known handles). Attribution is not accusation; it is where a reviewer looks first.

**Verify** (what actually ran; `gh` is **not installed** in this environment — the tracker's
environment list was wrong and is corrected below):
```bash
# 1 · the witness: GitHub's own tree object for the pin, compared against our clone in BOTH directions
curl -sS https://api.github.com/repos/jesdga95/basalt/commits/dc047a77beeddf1e91234d983a91d6bf929e9619   # .commit.tree.sha
curl -sS "…/repos/jesdga95/basalt/git/trees/3f7b69ae4054969ed15b9b6f02d306eb44e0ebe4?recursive=1"          # 942 entries, truncated=false
git -C vendor/basalt ls-tree -r dc047a77 | awk -F'\t' '{split($1,a," "); print a[1]"\t"a[2]"\t"a[3]"\t"$2}'  # mode, type, blob, path
#   → print API_BLOBS, LOCAL_BLOBS, INTERSECT, ONLY_IN_API, ONLY_IN_LOCAL, BLOB_SHA_MISMATCH separately.
#     The three cardinalities are NOT optional: a field-swap bug once printed BLOB_SHA_MISMATCH=0 over
#     two DISJOINT key sets. A zero over an empty intersection is not a pass.
#     SENSITIVITY CONTROL — run the identical comparison against the first parent (bbf2f5bb, tree
#     c7fbe4ef…): it must return BLOB_SHA_MISMATCH=1 (serve/util/telemetry.py). 0 would mean the
#     comparator cannot see a change and every 0 above is meaningless.
# 2 · tags/ancestry/signature (the plan asked; the answers are: annotated-but-unsigned, and no v0.3.5)
git -C vendor/basalt for-each-ref refs/tags --format='%(refname:short) %(objecttype)'   # all `tag`, none lightweight
git -C vendor/basalt merge-base --is-ancestor v0.2.1 dc047a77; echo "rc=$?"            # rc=0; 3 commits since the tag
git -C vendor/basalt log --show-signature -1 --format='%h %G?'                          # %G?=E, "No public key" → a LIMIT
# 3 · the author census (87 rows = 75 Tier A + 12 files this task names that the manifest leaves unassigned)
{ find context/audit/manifest -maxdepth 1 -name '*.tsv' -type f -exec awk -F'\t' '$5 ~ /^A-/ {print $1"\t"$4"\t"$5}' {} +
  find context/audit/manifest -maxdepth 1 -name '*.tsv' -type f -exec awk -F'\t' '$5 !~ /^A-/ && ($1=="CMakeLists.txt" || $1 ~ /^serve\/engine\// || $1 ~ /^src\/vision\//){print $1"\t"$4"\t"$5}' {} +
} | LC_ALL=C sort -u > /tmp/opencode/basalt-audit/t2/rows-plan.tsv
bash context/audit/scripts/t2-census.sh /tmp/opencode/basalt-audit/t2/rows-plan.tsv out.tsv err.log 87 vendor/basalt dc047a77beeddf1e91234d983a91d6bf929e9619
#   → ROWS_IN=87 ROWS_OUT=87 BINARY_SKIPPED=6 EMPTY_FIELDS=0 NOT_OK=0 ERR_BYTES=0 VALIDATION=PASS
#     CONTROL: run it with expect_rows=99 → exit 7. It must refuse a wrong-sized input: without that
#     gate the same producer printed VALIDATION=PASS over an EMPTY row list and exited 0.
#   → `git log --format='%ae' -- <boundary paths> | sort | uniq -c` for the domain census; parse
#     `uniq -c` output on a TAB, not the default FS, or a multi-word author name lands in the email column.
```
**Corrections to the task text above, found by running it (do not "fix" the artifacts to match these):**
`src/server/` **does not exist** in this tree — `git ls-tree -d dc047a77:src/server` is a fatal. The C++
side of the boundary is `src/platform/` (10 files) + `src/vision/` (10) + `include/basalt/platform/`;
the serving layer is Python under `serve/`. `gh` is not installed (use `curl` to `api.github.com`).
`v0.2.1` is the nearest release but the pin is **untagged** `main`; `versions.env` pins
`BASALT_SHA` and **names `v0.2.1` in a comment only**, asserting *"the engine is identical to v0.2.1"* —
a claim T2 verified **true**: 3 commits, 9 changed files (all `README.md`, `docs/`, `serve/`), **0** under
`src/`/`include/`/`cmake/`/`tools/`/`CMakeLists.txt`, and **553/553** engine blobs identical by identity.
So Plan 01's characterisation of the C++ engine carries to this pin (stalest at `CMakeLists.txt`, 287
commits) and basalt's own `v0.2.1` (`1b3f6964`) is an unambiguous anchor for T2b — unlike Strata's
same-named tags, which are different objects whose targets have already moved once.
The signature exists and GitHub reports it valid — but it is **GitHub's own key on a GitHub-created
squash merge**, unverifiable in this clone, which is a recorded limit, not the "GitHub offers no
signature" limit this task predicted.

> ⚠ **Methods corrected 2026-11-05, after T2 ran them — read before re-running anything upstream-shaped
> here.** (a) **Do not** probe a peer repository's commit API for our SHAs: GitHub resolves that route
> against the repo's **object store**, not its default branch, and this upstream has rewritten its
> mainline, so `200`/`422` proves nothing about shared history (it produced a false fork boundary).
> (b) **Do not** count or test objects inside a **partial clone you have already queried** — lazy-fetch
> fabricates shared ancestry; one run "found" 710 shared commits that a pristine clone does not contain.
> (c) The sound tests are: `git fetch --filter=tree:0 --no-tags origin <branch>` into scratch for a
> pristine history (verify `in-pack 0 → N`, no `objects/info/alternates`); the **promisor full-40-hex
> probe** (`cat-file -e <full-sha>^{commit}` → `upload-pack: not our ref` = not served on any advertised
> ref); and matching commits on the **content key `(tree, author-date, committer-date, subject)` — never
> by SHA between `jesdga95/basalt` and `Niko1221/Strata`, whose SHAs do not correspond at all.** Evidence
> and controls: `context/audit/wave0/t2-lineage.txt` §§ 0–4; `context/pitfalls.md` items 10–11 and
> *Cross-repo git forensics*.
**Expected observation**: **Confirms if** GitHub's tree SHA equals `3f7b69ae4054969ed15b9b6f02d306eb44e0ebe4`, and boundary files are dominated by the project's core authors. **Refutes if** the tree SHAs differ or vendor carries a non-public commit — **stop the plan**, nothing downstream is meaningful. An unusual one-commit author on a boundary file is a **finding to read first**, not a refutation. **Within noise if** missing signatures (documented limit), or a noreply-domain contributor that matches an existing GitHub handle.

**Commit**: `[basalt][P2][T2][audit] prove the pinned commit matches upstream and census boundary authors`

> **Landed 2026-11-05 as two commits** (both reviewers independently, same reason): the first is this
> task as scoped plus corrections to its own published claims; the second,
> `[basalt][P2][T2][audit] record the ancestor history rewrite and the AI-attribution corpus`, carries
> the *finding's use* — `distilled.md`, `high_level.md`, T2b, T3's scope additions and the
> version-ambiguity fixes — so that a later re-argument of the AI framing cannot take T2's provenance
> evidence down with it.

### T2b: Content lineage of the boundary — is the seam code *original*, or imported from Strata?

> **Re-scope 2026-11-05: skipped** — provenance, not behaviour (§ Re-scope). Follow-up.

**File**: `context/audit/wave0/t2b-content-lineage.txt` + a `lineage` column in
`context/audit/wave0/t2-author-census.tsv` (**a sidecar joined by path — never a 5th column in T3's
contract, never in `ledger-grid.sh`**, whose byte-identity checks cover the manifest)

**Why this exists (the four measurements that made it)**: T2 established that `jesdga95/basalt` and
`Niko1221/Strata` share **0** commit SHAs, have **no merge-base**, and are nonetheless one history in two
rewritten variants (**844** content-key pairs). That makes *commit*-level absence a **weak** signal: a
squashed upstream import reads exactly like original work. T2's seam ranking therefore rests on a
question it cannot answer — whether `src/platform/`'s bytes are basalt-era originals or inherited code
whose upstream commits were squashed away.

**What to do**: For each of the **80** first-party censused files, at the pin and at basalt's `v0.2.1`
(`1b3f6964`, engine-byte-identical to the pin — verified), take `git rev-parse <ref>:<path>` and ask
whether that **blob SHA** appears anywhere in the pristine Strata history (`git -C <pristine>
rev-list --objects --all` built once, then a set join; **blob SHAs are content addresses and are the one
thing that does compare across these repos**). Classify each file `byte-shared` / `byte-unique` /
`blob-changed-but-lineage-linked`, and record for each of the 7 seam commits from
`t2-boundary-authorship.txt` § 6 whether its **`git patch-id --stable`** has a twin in Strata.

**Calibration rule that governs the output**: lineage may **order reading and raise priority**; it may
**never lower it and never reduce depth**. `census.md` dispositions stay **lineage-independent** — a
`byte-shared` file is not more trusted, only older. Write the join into a sidecar so a later reader can
delete the annotation without losing a disposition.

**Verify**:
```bash
P=/tmp/opencode/basalt-audit/strata-pristine          # re-verify pristine first
git -C $P cat-file --batch-all-objects --batch-check='%(objecttype) %(objectname)' \
  | awk '$1=="blob"{print $2}' | LC_ALL=C sort -u > /tmp/opencode/basalt-audit/strata-blobs.txt
B=$PWD/vendor/basalt
while read -r f; do
  h=$(git -C $B rev-parse "dc047a77:$f")
  printf '%s\t%s\n' "$h" "$f"
done < /tmp/opencode/basalt-audit/files80c.txt > /tmp/opencode/basalt-audit/pin-blobs80.txt
echo "PIN_BLOBS=$(grep -c . /tmp/opencode/basalt-audit/pin-blobs80.txt) \
STRATA_BLOBS=$(grep -c . /tmp/opencode/basalt-audit/strata-blobs.txt) \
SHARED=$(LC_ALL=C comm -12 <(cut -f1 /tmp/opencode/basalt-audit/pin-blobs80.txt | LC_ALL=C sort) \
  /tmp/opencode/basalt-audit/strata-blobs.txt | grep -c .)"
```
**Expected observation**: **Negative control required, and it must fire**: a **Tier A binary** asset's
blob and a `docs/` image's blob — those were committed before the fork, so they *should* be shared; if
they are not, the Strata blob set is wrong (partial-clone or filter artefact) and nothing in this task is
readable. **Confirms if** the control shares ≥1 blob and the seam files' class is stated per file with
`|A|`, `|B|`, `|A∩B|` printed. **Refutes if** every seam file is `byte-shared`: then § 6's read order
loses its "no external existence" argument entirely and T3's priorities must be re-derived from content
alone. **Within noise if** a seam file is `blob-changed-but-lineage-linked` — expected for any file that
was edited after the fork.

**Commit**: `[basalt][P2][T2b][lineage] join the boundary files to Strata by blob identity and patch-id`

### T3: The primitive census, and six canaries that prove it can see

> **Re-scope 2026-11-05: narrowed** — census re-run once, `sites.tsv` kept as `census.md`, canaries **1–3** only (§ Re-scope). The text below is the original scope.

**File**: `context/audit/census.md`

**What to do**: Enumerate boundaries **exhaustively rather than by guesswork**, shipped files and build-time files treated separately. Python: every `import`/`from` in every shipped module, plus the holes — `importlib.import_module(var)`, `__import__`, `getattr`-assembled calls, chr()/base64-assembled names. C++/CUDA: external deps from our build files. **Output contract — each row: `path:line TAB primitive TAB disposition TAB assigned-TN`**; a census row with no task number is unowned work and the Wave 0 gate fails. Sections: network syscalls · DNS · **exec/spawn wide** (`system`/`popen`/`fork`/`exec*`/`posix_spawn`/`os.exec*`/`os.system`/`os.spawn*`/`subprocess` incl. `shell=`) · `dlopen`/`dlsym` · raw `syscall()` · file writes outside expected paths · **fingerprint reads** (C++ `getenv`; Python `os.environ`/`os.getenv`/`platform.node`/`socket.gethostname`/`/etc/machine-id`/mac address/cpuinfo) *paired with* "is there a sink nearby?" — fingerprint-gated activation is how targeted malice hides · credential stores (`~/.ssh`, `~/.aws`, keyring, browser stores, wallet dirs) · `/proc`/`/sys` · persistence (cron, systemd, autostart, rc files) · **input capture** (`/dev/input`, `EVIOCGRAB`, X11 `XRecord`/`XInput`, `uinput`, `SetWindowsHookEx`, `GetAsyncKeyState`) · **parsers that accept network bytes** (image decoders, GGUF/mmproj readers, HTTP framing/chunked-body, JSON, tokenizer) — the class a danger grep cannot see · **vendored third-party** sources · **orphan config keys**: any key in the config schema or the GGUF/profile key space naming `host`/`port`/`url`/`token`/`endpoint` that has plumbing but **no sink** — the highest-signal malice residue available, because it is a network feature with the call site removed and the field kept · **diagnostics-shaped outbound**: search by *shape*, not keyword — a module whose job is assembling a report and handing it to something (a plausible `post_report()` in a plausible `util/`) will not contain `socket()`.

**Canaries (required, separately evidenced — target 6/6)**: copy the tree to `/tmp/opencode/basalt-audit/canary/` and plant, in innocuous homes including one CUDA kernel, one numerical header and one test file:
1. a base64 blob decoding to a URL; 2. a literal `https://` to a throwaway host; 3. a raw `socket()`/`connect()`;
4. **cross-file assembly** — a URL built from constants spread across **two different top-level directories** (every other plan plants single-file; a real maintainer hiding one would not);
5. **side-channel, invisible to the taxonomy** — a routine encoding a secret into **response token count or inter-token latency**: no new primitive, no syscall, no import. It tests whether the *category list* is right, which is what the verdict actually rests on;
6. **second-hop** — data written into a log field that `/metrics` later re-labels and serves: nothing leaves, nothing connects, and the value goes out anyway.

**Three additions to this task's scope, handed over by T2** (`audit/wave0/t2-boundary-authorship.txt` § 6):

1. **A calibration rule, written before there is any temptation to use it.** 97.9 % of the censused lines
   sit under commits that self-declare AI co-authorship, which supplies a ready innocent story for
   exactly the pattern this task's orphan-config-key section calls its highest-signal residue. So:
   **"AI residue" may never be the reason a row goes `benign`.** It may only be the reason a row is
   *ranked lower* after the key or call site has been traced end-to-end and no sink exists. Expect a
   large volume of inadvertent residue (hallucinated URLs, copied debug endpoints, dead config plumbing)
   and treat volume as **base rate, not evidence**; canaries 4–6 are the discriminating ones because they
   test structure rather than presence.
2. **A hard non-action.** Whatever the census turns up — implausible domains, dead package names,
   token-shaped constants — external identifiers are resolved **by inspection and documentation only.
   Never connect, never fetch, never `pip install`, never `git ls-remote` them.** A nonsense-looking
   domain may be a real one that reacts to being probed, and this audit's most valuable property is that
   nobody on the other end knows it exists. (Reading GitHub *metadata*, e.g. whether PR #47 was
   self-approved, is a different act and is in scope.)
3. **A surface the primitive taxonomy cannot see: instructions addressed to a model.** `AGENTS.md`,
   `CONTRIBUTING.md` and `.github/` are in the manifest, and upstream's `AGENTS.md` instructs the next
   agent directly. We run agents on this tree, so a shipped file an obedient agent will act on is a
   capability path with no syscall in it. Give it a census section, including **source comments phrased
   as instructions to a model**, and rank it by what it could make an agent do.

**Verify**:
```bash
cd /home/iphands/prog/ai/local-mind/server/basalt/vendor/basalt
git ls-files 'serve/**/*.py' 'tools/**/*.py' | xargs grep -hE '^\s*(import|from) ' | awk '{print $2}' | tr -d ',(' | sort -u | tr '\n' ' '; echo "— import census rc=$?"
cd /tmp/opencode/basalt-audit/canary && <the identical census commands> | grep -icE 'canary|<planted-host>|socket|<side-channel-symbol>'; echo "— canary count (target 6)"
```
**Expected observation**: **Confirms if** 6/6 flagged, including #5 and #6 — which no keyword in the taxonomy would find, so catching them proves the census was *run*, not that it was *expected* to work. **Refutes if** any is missed. **A missed canary voids the census**: fix it, re-run it, and re-derive every Tier B reason recorded since. **Within noise if** a canary is caught by an unexpected section but caught nonetheless; **never** within noise if it was spotted by hand outside the census.

**Commit**: `[basalt][P2][T3][audit] census every boundary primitive and prove it with six planted canaries`

### T4: The one outbound channel — read line by line, then run the bypass list

> **Re-scope 2026-11-05: narrowed** — read both files line by line; the bypass list below is a Follow-up.

**File**: `serve/engine/image_source.py` (all), `serve/security.py` (all), `serve/tests/test_image_sources.py` as the intent map. Work in a scratch copy with **monkeypatched DNS**, so each bypass is an executed assertion with an observed outcome, not a paragraph.

**What to do**: The server's only code that connects anywhere. The pinned-address literal already handles re-resolution (Key Facts); what remains is whether the **address set** can be steered and whether **peer classification** can be spoofed into granting local privilege (the privilege gained includes **local file read** — the more dangerous direction). **The list is pre-committed here so it is tickable, and the deliverable is the DELTA between upstream's test list and this one** — upstream's suite passing is not evidence of absence:

- [ ] decimal (`2130706433`), octal (`0177.0.0.1`), hex (`0x7f000001`) IPv4 literals
- [ ] short/`0.0.0.0/8`-style forms (`127.1`, `127.0.0.1` variants) that `ipaddress` rejects but a resolver accepts
- [ ] IPv4-mapped IPv6 `::ffff:7f00:1`, `::1`, Teredo/6to4 wrappers, `fd00::/8`
- [ ] multi-A-record sets where only *some* addresses are public, and TTL-0 rebinding between check and use
- [ ] CNAME chain ending on a private address
- [ ] userinfo confusion `http://127.0.0.1@evil.example/`
- [ ] `Host`/origin/forwarding-header case variants, duplicates, obs-fold line folding
- [ ] redirect (must be absent), proxy env (`http_proxy`), `file:`/`ftp:` handlers (must be absent)
- [ ] does the blocklist test the **literal** or the **resolved** address, and is the set it connects to exactly the set it validated?

**Verify**: `cd vendor/basalt && python3 -m pytest serve/tests/test_image_sources.py serve/tests/test_security.py -q 2>&1 | tail -3`; then the scratch-copy bypass run, one assertion per checkbox above, each recorded pass/fail in `context/audit/wave1/`.
**Expected observation**: **Confirms if** every checkbox has an executed result and none yields a connect outside the operator's allow-list, and no header variant converts a remote peer into a local one. **Refutes if** any single item connects or gains local read → **🔴**, tracker row + `pitfalls.md`. **Within noise if** an item is unreachable at the parser before reaching policy (record where it dies); never within noise if untested.

**Commit**: `[basalt][P2][T4][audit] read the image fetcher and execute the SSRF and peer-spoof bypass list`

### T5: The decode path — resource bounds and first-party amplification (stb is 2.30, current)

> **Re-scope 2026-11-05: skipped** — decoder memory-safety hunting does not answer the question. Follow-up.

**File**: `src/vision/preprocess.cpp` and the rest of `src/vision/` (1,584 l, Tier A); `src/program/serve/vision.cpp`; `src/vision/stb_image.h` as **Tier B `vendored-third-party`** — diffed as a dependency version, not read as first-party.

**What to do**: (a) Read the first-party half properly — that is where the non-diffable risk is: `(int) std::min<size_t>(len, 0x7fffffff)`, the `w*h > max_pixels` guard, and `out.rgb.assign(px, px + (size_t)w*h*3)`. **Check the guard is evaluated against the same product the `assign` consumes** — a guard on `w*h` protecting an allocation sized `w*h*3`, or one computed in `int64_t` while a consumer uses `int`, is a real bug independent of stb's own CVEs. Confirm where `max_pixels`/`kMaxImagePixels` comes from and **the maximum a request can actually produce**. (b) Build a **CPU** ASan/UBSan harness in a scratch copy driving `decode_image` over **JPEG, PNG, BMP and GIF only** — seed the corpus with **public PoCs for CVE-2022-28041/28042, GHSL-2023-145/148/150 and the 2.28 BMP offset hardening**, not only random mutations, and run it with `max_pixels` set to the request-reachable maximum so **the harness tests the code rather than being shielded by the guard**. (c) Confirm whether the server path calls `stbi_set_flip_vertically_on_load` (GHSL-2023-146 / CVE-2023-45662 is in `stbi__vertical_flip`). (d) Write the recommendation: a vendored bump to 2.30 in an **upstream** change is the real fix; ours is a `Dockerfile`-side patch, which is a build change and therefore its own plan.

**Verify**:
```bash
cd /tmp/opencode/basalt-audit/stb && g++ -std=c++17 -O1 -g -fsanitize=address,undefined \
  fuzz_decode.cpp preprocess.cpp -o fuzz_decode && ./fuzz_decode corpus-jpeg-png-bmp-gif/; echo "— fuzz rc=$?"
grep -c 'STBI_ONLY' vendor/basalt/src/vision/preprocess.cpp; echo "— expect 4"
```
**Expected observation**: **Confirms if** zero ASan/UBSan reports across the four formats at the request-reachable `max_pixels`, and the guard is shown to bound the same product the `assign` consumes. Then the finding stands as 🟠 *stale vendored dependency with known CVEs in compiled-in decoders, mitigated by an upstream guard we verified*. **Refutes if** **any** ASan/UBSan report on a malformed JPEG/PNG/BMP/GIF — that is memory corruption reachable from an unauthenticated LAN socket and it **overtakes every exfil question as the plan's headline**. **Within noise if** an absurd allocation refused by the `max_pixels` guard (the guard working; record it) — never an OOM *inside* a decoder.

**Commit**: `[basalt][P2][T5][audit] bound the decode path's memory and check the pixel guard against its own consumer`

### T6a–T6c: The server surface, in three ≤1,500-line slices

> **Re-scope 2026-11-05:** T6a and T6c kept; **T6b merged into T8**.

**T6a — logging and metrics disclosure**: `serve/util/logbuf.py`, `serve/util/telemetry.py`, the logging/metrics parts of `serve/service.py` and `serve/handler.py`. Where do prompts and completions actually go — the engine `--log` file, `logbuf`, `--request-lines`, temp/cache under `--image-dir`, the vision cache — does any survive unload, and is it world-readable in the container? Does `GET /metrics` disclose host identifiers (GPU name, driver, CUDA version, CPU model, core count) to a **remote** caller (an identifier leak, ranked honestly as such, not a backdoor)? Confirm `telemetry.py`'s name is a misnomer by reading it, and confirm the `responses.py` base64 is the `encrypted_content` round-trip.

**T6b — exec trust boundaries → feeds the T8 argv table**: trace `before_load` (`serve/config.py` → `service.py:250`) and prove it is reachable only from operator config/CLI, **never from a request body or from a model pack's GGUF metadata** — `serve/engine/modelfile.py` already maps `general.sampling.*` into request defaults, so check nothing else maps metadata into an exec-capable setting. Same for `--engine-bin`, `--script`, `--config`, `--tokenizer`. Every row you produce lands in `argv-table.md`.

**T6c — the negative sweep (one command, not a reading task)**: T3's input-capture and credential-store sections re-run over `serve/` with the sentinel convention, plus a check that nothing in `serve/` opens `/dev/input*`, `XRecord`, or a keyring.

**Verify**: `grep -nE 'open\(|\.write\(|NamedTemporary|mkdtemp' serve/service.py serve/handler.py serve/util/logbuf.py | head -30`; `grep -rn 'before_load' serve/ --include='*.py' | grep -v tests`; `python3 -m pytest serve/tests/test_metrics.py serve/tests/test_security.py -q 2>&1 | tail -2`; the T6c sweep — all ending in sentinels.
**Expected observation**: **Confirms if** prompt content is written only where an operator configured it; `before_load` and the exec flags have no request- or pack-reachable source; the negative sweeps are empty. **Refutes if** any prompt/completion persisted outside operator config, any request- or pack-influenced value reaching `subprocess`, or a non-empty input-capture/credential sweep. **Within noise if** hardware identifiers on `/metrics` (rank 🟡/🟠 by audience; never drop silently); token counts and timings in logs, which is normal.

**Commit (one per slice)**: `[basalt][P2][T6a][audit] trace server logging and metrics disclosure` · `[basalt][P2][T6b][audit] prove before_load and the exec flags have no request-reachable source` · `[basalt][P2][T6c][audit] sweep serve for input-capture and credential reads`

### T7: Template injection and tokenizer ReDoS on a single-GPU box

> **Re-scope 2026-11-05: narrowed** to the template half; tokenizer ReDoS is a Follow-up.

**File**: `serve/chat_template.jinja` (9,993 B), `serve/prompt.py`, `serve/api/*.py`, `tools/tokenizer.py`

**What to do**: `jinja2` is a real dependency applied to request-supplied message fields: determine whether user content reaches a **template** (injection → code exec) or only render **data** (safe), and whether a **model pack's metadata can override the template** — that combination is code execution delivered by a downloaded file. Then `tools/tokenizer.py:67` `regex.compile(QWEN35_PATTERN)`: find catastrophic-backtracking candidates; this is a **single-tenant GPU box**, so one hostile request that hangs the tokenizer is an availability finding.

**Verify**: `grep -rnE 'Environment|from_string|Template\(|render' serve/ --include='*.py' | grep -v tests`; the timed pathological-input scan of `QWEN35_PATTERN`; `— scan rc=$?`.
**Expected observation**: **Confirms if** templates load from file paths only and user content is passed strictly as context; scan well under 1 s. **Refutes if** any request- or pack-supplied string reaches `Template`/`from_string` (🔴), or tens of seconds on one request. **Within noise if** linear-but-slow scanning on inputs the server's own byte caps already bound to sub-second.

**Commit**: `[basalt][P2][T7][audit] check chat-template injection and tokenizer ReDoS exposure`

### T8: The argv table — one row per exec site in C++ **and** Python

**File**: `src/platform/` (10 files, 2,921 l, Tier A) + `include/basalt/platform/`; deliverable `context/audit/argv-table.md`

**What to do**: The engine's raw OS calls are legally confined to this seam (enforced by `tools/check_platform_boundary.sh`), so ~3k lines here license everything the engine half claims. Read `system.cpp`'s `capture_output` + `shell_quote` (total for empty string, `$(…)`, backtick, newline, `0x00`?), `process.cpp`'s `fork`/`execvp`/`CreateProcessW`/`spawn_piped`, `dynlib.cpp`'s `dlopen` (is the soname constant? a `dlopen` path is exactly what `nm -D` cannot see), `file.cpp`/`mapped_file.cpp`/`direct_file.cpp` (path construction that could escape the working directory).

**Then the load-bearing artifact.** The question "can a request reach an `argv`?" cannot be answered from this directory, because **the argv is assembled in `serve/` and consumed here**. `argv-table.md`: one row per exec/spawn/`dlopen` site **in both languages**, columns `path:line` · the literal argv vector · per-element origin (`constant` / `--flag` / config key / **request field**) · verdict. Site enumeration must use the wide set (`system`/`popen`/`fork`/`exec*`/`posix_spawn`/`os.exec*`/`os.system`/`os.spawn*`/`subprocess` incl. `shell=`/`dlopen`/`dlsym`) — the recon grep that produced today's short list omitted half of it.

**And the trap**: `tools/check_platform_boundary.sh` passing proves only **compliance with its own deny-list**. Read the script's regexes and state in the plan's own words what they do **not** match; that gap, not the pass, is the finding.

**Verify**:
```bash
cd vendor/basalt && tools/check_platform_boundary.sh && echo "— seam script rc=$? (compliance with its own deny-list, nothing more)"
grep -rnE 'capture_output|spawn_piped|spawn_and_wait|dlopen|execvp|posix_spawn|os\.exec|os\.system|subprocess\.' src/ include/ serve/ --include='*.cpp' --include='*.hpp' --include='*.py' | grep -v '/tests/\|test_' | wc -l
```
**Expected observation**: **Confirms if** every row's argv elements are all `constant`/`--flag`/config-key, and the script's deny-list gaps are named. **Refutes if** **any** argv element traces to an HTTP request field or a parsed pack — **stop and escalate**; do not finish the plan past that. **Within noise if** an argument built from an operator CLI flag (`--expert-profile`, `--image-dir`): operator privilege already implies it (🟡, with the note).

**Commit**: `[basalt][P2][T8][audit] build the cross-language argv table and read the boundary script's limits`

### T9: The census-assigned engine files — scope frozen at Wave 0 exit

**File**: **the rows assigned T9 in `context/audit/census.md`** — not a prose description. Known now: `src/program/serve/loans.cpp`, `loan_moves.cpp`, `src/program/multi_gpu_plan.cpp`, `src/program/image_loan.cpp`, `src/program/generate/configure.cpp`, `src/vision/mmproj.cpp`.

**What to do**: **At Wave 0 exit, freeze T9's remainder into a named file list with a line count; if it exceeds 3,000 lines, split it into numbered subtasks in SERIES *before* reading starts** — a task whose scope is "whatever T3 flagged" is unsizable and will silently become Plan 04. Then read them line by line and settle: `configure.cpp:40`'s `"/data/profiles/" + key + ".prof"` — can `key` reach it from a request? The cross-card **loans** path with the *API* names, not just the tier names: `cudaMemcpyPeer`, `cudaDeviceEnablePeerAccess`, `cuMemExportToShareableHandle`, `shm_open`, `libfabric`, `socket`, `AF_UNIX` — confirm sharing is intra-process (host memory + CUDA streams) with no cross-host endpoint. **Read `97ff061` ("remove the dead remote-expert tier") as a diff** — what fields, flags and config keys survived it, and do any of them still reach the census's **orphan-key** section? A plumbed-but-sinkless `host`/`port`/`url` field is the single most diagnostic remnant available. `mmproj.cpp`'s `pread`-into-buffer header parse is a second request-adjacent parser: same scrutiny as T5.

**Verify**: the API grep above per file (sentinel-terminated); `git show 97ff061 --stat` plus the surviving-key grep against the config schema and the GGUF/profile key space.
**Expected observation**: **Confirms if** zero cross-host/cross-process endpoints in loans/multi-GPU, `key` provably operator-only, and no surviving plumbed-but-sinkless network-named key. **Refutes if** any peer/IPC/shared-memory endpoint exists across hosts, `key` is request-reachable, or a dead tier's network field is still parsed from config or a pack. **Within noise if** intra-process CUDA event/stream sync and `mmap` of model files (that is how it loads weights).

**Commit**: `[basalt][P2][T9][audit] read the census-assigned engine files and the remote-tier removal diff`

### T10: The opaque bytes — the `.prof`, fonts, images

**File**: `data/profiles/qwen4exp-48x512.prof` (196,628 B, `file` says only "data"), `data/bench/*.ids`, `serve/web/fonts/*.woff2`, `assets/*.png`, `docs/img/frontend.gif`, `serve/chat_template.jinja`, `serve/web/{app.js,app.css,index.html}`, `THIRD_PARTY_LICENSES`

**What to do**: Where a payload actually hides, and cheap. Entropy-scan and hexdump the `.prof`; grep **every** binary asset for embedded literals (`http`, `://`, `/bin/`, `eval`, printable runs > 16 chars) — a WOFF2 or PNG yielding shell literals is the whole game. Then read the `.prof` **parser** and confirm no field is consumed as an offset/length without bounds and nothing in the file can redirect execution. Re-run (do not inherit) the web-asset external-reference check. **T2 raised this file's priority, and the reason is narrower than it first looked.** `qwen4exp-48x512.prof` (960 "lines", 196,628 B, magic `\x01\0\0\0…`, not git-lfs — `.gitattributes` only sets `eol`) sits on the engine's *default lookup path*: `src/program/generate/configure.cpp:40` concatenates `BASALT_SOURCE_DIR + "/data/profiles/" + key + ".prof"`, and `docs/configuration/index.md:153` / `docs/model-file.md:82` name that path as the default — a **path convention, not provenance**. **The only writer of the format in the tree is the running server's adaptive tier**: `basalt::core::write_expert_profile` (`src/core/expert_cache.cpp:99`), called from `src/program/serve/tier_profile.cpp:16` under `--expert-profile-save` (`src/program/cli/arguments.cpp:233`; periodic at `src/program/serve/commands.cpp:217`; guarded at `serve/tiers.cpp:555`; `docs/model-file.md:87-89` documents producing one by serving a representative prompt mix and QUITting), and the probe tool explicitly **compares, never writes** (`src/program/probe_main.cpp:64`). So there is **no offline, in-repo, deterministic generator** — no tool, script, test or CI job under `tools/`, `tests/` or `.github/` emits a `data/profiles/*.prof`. Consequence: the shipped bytes cannot be regenerated deterministically from anything in this tree; reproducing them needs an unpinned server run whose prompt mix, model build and hardware are recorded nowhere. The file is **handed-in**. *(Corrected 2026-11-05: this scope line first asserted "no writer/generator for `.prof` exists anywhere in the repo", which is **false** — my `git grep` was scoped `-- '*.py' '*.sh'`, so a C++ writer was structurally outside the pathset and the empty result measured the pathspec, not the repo. See `pitfalls.md` item 15.)* **T10's pass condition, stated as a falsifier:** the file must be fully accounted for by the same code path that wrote it — the parser consumes **all 196,628 bytes** with none unexplained (the header, the pair array, any trailing region), and the shipped expert pairs are consistent with the layout of the model this pack targets (`qwen4exp-48x512`). "High entropy" is not a pass; an unaccounted byte region is a **finding**, and if the structure is not fully explained, say so rather than defaulting to `benign`.

**Verify**: the entropy one-liner; `git ls-files | grep -E '\.(woff2|png|gif|svg|prof|ids)$' | xargs -I{} sh -c 'strings -a "{}" | grep -iE "http|://|/bin/|eval" && echo "  ^^ IN {}"'; `echo "— asset scan rc=$?"`.
**Expected observation**: **Confirms if** no asset yields URL/path/`eval` literals and `.prof` structure is fully explained by its parser. **Refutes if** any literal in any binary asset, or an unbounded offset. **Within noise if** high entropy from a genuinely compressed/float table **whose structure the parser confirms** — never accept "high entropy" as benign without the parser reading.

**Commit**: `[basalt][P2][T10][audit] entropy-scan the shipped binary assets and read the profile parser`

### T11a–T11d: Tier-B triage of the engine bulk, one commit per directory

> **Re-scope 2026-11-05: one mechanical commit (`T11`)** — the scans below over `src/` `include/`, exceptions listed and explained; no per-file reasons, no per-directory commits.

**File**: manifest rows in `context/audit/manifest/src.tsv`, `include.tsv`; ledger rows in `context/audit/coverage_ledger.md`. **Split by directory — one commit each — with the `comm → 0` check recorded after each**, so a session that dies mid-task leaves a committed record and a resumption point (514 files in one commit is the exact volume failure this rule exists to prevent).

**What to do**: **Not** line-by-line — that is Plan 04; this makes the deferral *accountable rather than invisible*. Per directory, two passes: **(a) mechanical**, command-generated manifest fill (primitives hit, triage reason, coverage) in one commit; **(b) human judgment on the exception list only** — files with a primitive hit, non-ASCII identifiers, a long literal, or a vendored-suspect signature. Specifically: any primitive not already dispositioned; any **second vendored third-party source** (is `stb_image.h` alone? look for `stb_rect_pack`, `stb_truetype`, `miniaudio`, `xxhash`, single-file libs); any obfuscation marker (RTL/bidi `U+202E`/`U+202D`, zero-width `U+200B`–`U+200F`/`U+2060`–`U+2064`, homoglyph identifiers, long hex/base64 runs).

**Verify** (`LC_ALL=C` on **both** sides — locale collation otherwise fakes a mismatch):
```bash
cd vendor/basalt && git ls-files src/ include/ | xargs grep -lP '[\x{202E}\x{202D}\x{200B}-\x{200F}\x{2060}-\x{2064}]' 2>/dev/null; echo "— bidi scan rc=$?"
git ls-files src/ include/ | xargs grep -lnE '[0-9a-fA-F]{40,}|[A-Za-z0-9+/]{60,}={0,2}' | head; echo "— long-literal scan rc=$?"
cd /home/iphands/prog/ai/local-mind/server/basalt
LC_ALL=C comm -13 <(LC_ALL=C cut -f1 context/audit/manifest/src.tsv context/audit/manifest/include.tsv | LC_ALL=C sort) \
                 <(LC_ALL=C git -C vendor/basalt ls-files src/ include/ | LC_ALL=C sort) | wc -l; echo "— expect 0"
```
**Expected observation**: **Confirms if** 0 unaccounted files, 0 bidi/zero-width hits, every long literal named and explained. **Refutes if** an unaccounted file, an undocumented second vendored library, or a bidi-override/zero-width character in source (a classic homoglyph-smuggling marker — a finding regardless of intent). **Within noise if** long hex runs that are quantisation tables or IEEE constants with a nameable purpose.

**Commit (one per directory)**: `[basalt][P2][T11a][audit] triage src/kernels with a recorded reason per file` … `T11d … include/`

### T12: Dependency provenance and the hash gap

**File**: `requirements.txt` (8 names), our `Dockerfile` pip step, `THIRD_PARTY_LICENSES`, `docs/community/benchmarks.json`

**What to do**: For each pinned name confirm it resolves to the **genuine** project (maintainer, homepage, plausible release cadence for that version — a typosquat check, not a name check) and that upstream published that version. Confirm no install-time execution **in the repo**; the venv's `.pth`/`sitecustomize.py`/`usercustomize.py` check is a **built-image** check → write the command, hand to the owner (U8, Plan 03). Confirm the `pages.yml` `benchmarks.json` surface never reaches server code. Then write the **hash gap** into the verdict headline as the condition it is: unpinned wheels mean the audited source is **not provably** the built artifact — name the fix (`pip-compile --generate-hashes` → `pip install --require-hashes`), which is itself a plan per RULES.md's gate table.

**Verify**: `grep -c '==' vendor/basalt/requirements.txt` → 8; per-name version check; `grep -rn 'benchmarks.json' vendor/basalt/serve/; echo "— empty means the server never reads it rc=$?"`.
**Expected observation**: **Confirms if** 8 canonical names at real versions, zero install hooks in the repo, `serve/` never reads the community file. **Refutes if** a non-canonical name, an unpublished version, or an install hook. **Within noise if** a stale `homepage` field on a canonical project (🟪 hygiene, record the difference).

**Commit**: `[basalt][P2][T12][audit] verify dependency provenance and record the unpinned-hash gap`

### T13: Our own posture — the part the owner can change today

> **Re-scope 2026-11-05: moved to right after T3.**

**File**: `run`, `lib/common.sh`, `Dockerfile`, `versions.env` (ours) — **in scope by owner decision 2026-11-05**

**What to do**: Both reviewers independently judged this the audit's highest-value output, because it is where *our* data actually leaves or stays. `--host 0.0.0.0` + `-p 0.0.0.0:8700:8700` with an **optional** API key (`security.exposed_without_key` *flags* a non-loopback bind without a key — determine what `./run` actually **does** about it, and whether `--allowed-host cosmo.lan` widens or restricts); what `/metrics` discloses to a LAN peer; the container's network mode and whether the engine gets egress at all (the cheapest control: **no egress** — which Plan 03 proves); prompt retention in the container's writable layer and whether `--image-dir`/vision cache persists across boots; `--engine-bin`/`--script`/`--config` as trust boundaries we hold.
>
> ⚠ **At a bump, `git log <old-pin>..<new-pin>` is the right comparison; `git log Niko1221/Strata/main..<new-pin>`
> is the trap.** `versions.env`'s comment names `v0.2.1` as the engine anchor — use **basalt's** `v0.2.1`
> (`1b3f6964`, tree `515676c1`), never Strata's same-named tag (a different object, and its target has moved
> once), and
> re-run T2's blob-identity check (`audit/wave0/t2-provenance.txt` § 6's engine-path comparison) rather
> than any diff against the forked-from project: the two repos share 0 SHAs and have no merge-base, so
> `A..B` there is just `B` and reads as "all of it is new work" (`audit/index.md` § *If you are about to run
> something against upstream*). Check the same T13 bump for a **signed** release tag and for Software
> Heritage holding `jesdga95/basalt` — both are `Revisit if` triggers in `high_level.md` § *Attesting the
> Basalt pin*.

**Verify**: `img=$(bash -c 'source lib/common.sh; load_pins; image_for_pin'); export IMAGE_OVERRIDE=${img:-stub:none}; DRY_RUN=1 ./run 2>&1 | grep -oE '\-p [^ ]+|--host [^ ]+|--api-key.*|--network [^ ]+'; DRY_RUN=1 ./run 2>&1 | tail -1; echo "— rc=$?"`
**Expected observation**: **Confirms if** a concrete ordered action list, each item the exact flag and its one-line tradeoff. **Refutes if** nothing to change — then say that, don't manufacture recommendations. **Within noise if** an exposure deliberate and documented in `versions.env`/`README.md` as owner-chosen.

**Commit**: `[basalt][P2][T13][audit] review our own run and Dockerfile posture and write the action list`

**Wave 4 exit gate** = the plan closes: verdict written, harvested, moved.

### T14: The channel matrix, the verdict, the harvest

> **Re-scope 2026-11-05: lighter** — three sentences, one matrix row per channel (evidence or "not checked"), coverage = files read ÷ files present; no row quota.

**File**: `context/audit/verdict.md`; findings → `context/pitfalls.md`; confirmed facts → `context/distilled.md`; plan + tracker → `completed/`

**What to do**: Open with **three sentences in the owner's nouns, not ours**: no keylogger / nothing takes your credentials / nothing sends your data out except the reply to whoever asks the server — *and the condition* (unpinned wheels; engine bulk triaged not read). Then the **channel matrix**, ordered by **human consequence**, one glossed line per row ("DNS — could it exfil your prompts via domain names?"): outbound socket · DNS · subprocess spawn · files written outside expected paths · env exfil · persistence (cron/systemd/autostart) · keylog/input capture · credential stores · GPU/NVML identifiers · logs & prompts on disk · build-time pip hooks · venv hooks · model-pack-parsed control flow · IPC/cross-host · **our own config**. Cell text is **not free-form** — exactly `proved it can't (static, T4) — runtime: Plan 03` / `read it, not there (T6a)` / `didn't look → Plan 04`. Free cells are where "inspected" quietly becomes "closed". Never mark a row `proved it can't` on static reasoning alone when Plan 03's runtime leg could settle it. Then the **depth table** (lines read vs present, per tier, named gaps → Plan 04). Then the capability-ranked findings. **End with "if you do only one thing"; put the honesty limit second-to-last** — the hedge must not get the final word: *"this cannot disprove a logic bomb reachable only under inputs we did not run."* No unexplained jargon: spell out SSRF/ReDoS in-line.

**Verify**: matrix rows ≥ 15; `grep -n 'didn.t look' context/audit/verdict.md` — each names a plan; coverage figure traceable to a pasted command; full gate; Rule C move; Rule D harvest check.
**Expected observation**: **Confirms if** every row has evidence or a named plan, and a reader who has never heard of SSRF understands the first three sentences. **Refutes if** any row implies closed without evidence, or the coverage number was typed rather than derived. **Within noise if** wording. The structure is not negotiable: **no matrix, no verdict.**

**Commit**: `[basalt][P2][T14][audit] write the channel matrix and verdict and harvest the findings`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `vendor/basalt/src/vision/stb_image.h` + `preprocess.cpp` | vendored decoder **at 2.30, current** — the risk is **resource amplification (≥30× analytic, unmeasured) and first-party guard arithmetic**, not a stale version; all four formats compiled in | **P0** |
| `vendor/basalt/serve/engine/image_source.py` | Only outbound channel; executed bypass list, delta vs upstream's tests | **P0** |
| `vendor/basalt/src/platform/` (10 files) + `context/audit/argv-table.md` | Every raw OS call, and the C++↔Python argv proof | **P0** |
| `context/audit/manifest/` + `census.md` | The completeness apparatus; if weak, the verdict is theatre | **P0** |
| `vendor/basalt/serve/security.py` | Peer classification → remote clients gaining local read | P1 |
| `vendor/basalt/serve/service.py` (`before_load` `:250`) | `shell=True` reachability from config vs request/pack | P1 |
| `vendor/basalt/src/program/serve/` (8,465 l) + `97ff061` diff | Request path, cross-card loans, orphan network keys | P1 |
| `vendor/basalt/serve/chat_template.jinja` + `tools/tokenizer.py` | Template injection via pack metadata; ReDoS DoS | P1 |
| `vendor/basalt/data/profiles/qwen4exp-48x512.prof` + parser | Only opaque shipped binary | P1 |
| `requirements.txt` + our `Dockerfile` | Unpinned wheels = conditional verdict | P1 |
| `run`, `lib/common.sh` | Our posture: LAN bind, optional key, egress | P1 |
| `tools/check_platform_boundary.sh` | Its deny-list's gaps, not its pass | P2 |
| `vendor/basalt/serve/util/telemetry.py` | Confirm the misnomer; `/metrics` disclosure only | P2 |

---

## Open Questions / Risks

1. **Risk: a pin bump mid-audit silently invalidates everything.** `./build` re-checks-out vendor detached at `BASALT_SHA` and refuses a dirty tree. *Mitigation*: the single freeze-check command (Scope) is Resume step 1 and runs at every wave exit; mismatch = blocker row, never a silent continue. *How we'll settle it*: the check's output in each wave's Evidence.
2. **Risk: Tier B is read as "clean" instead of "not read".** *Mitigation*: per-file reasons, `comm → 0`, and the depth table naming `src/kernels`/`src/core`/`include/` as Plan 04's, never as passed. *How we'll settle it*: a verdict whose coverage figure was typed rather than derived fails T14.
3. ~~**Risk: the census's *category list* is wrong, which three same-shaped canaries cannot detect.** *Mitigation*: canaries #4–#6 (cross-file, side-channel, second-hop) are deliberately invisible to the taxonomy; a miss voids the census and forces re-derivation of every Tier B reason. *How we'll settle it*: 6/6, evidenced per canary.~~ **RESOLVED (re-scope 2026-11-05):** canaries 4–6 dropped as *census* canaries — no grep can see a side-channel or a second hop by design. The category-list risk is carried by reading (T6a `/metrics`) and by Plan 03's observed egress, which does not depend on a taxonomy.
4. **Risk: a static verdict mistaken for the final answer.** Every blind spot (`dlopen`+`dlsym`, raw `syscall()`, venv C extensions, the link's true DT_NEEDED) closes only in Plan 03. *Mitigation*: Plan 03 registered as the runtime gate on this verdict; matrix rows say `runtime: Plan 03`. *How we'll settle it*: Plan 03's close, or the owner explicitly accepting a static-only verdict — recorded as their decision.
5. **Risk: the hash gap makes "we audited what we run" false.** *Mitigation*: named in the verdict headline; fix is a hashed lock (itself a plan). *How we'll settle it*: owner decision, T12.
6. **Question: is `stb_image.h` the only vendored third-party source?** *How we'll settle it*: T11's vendored-library sweep; any second one gets the same version-diff treatment, not a read.
7. **Question: can a request influence any `argv` in either language?** The load-bearing unknown of the engine half. *How we'll settle it*: T8's argv table; a `request field` origin in any element stops and escalates the plan.
8. **Question: can a model pack alter server behaviour?** Its metadata already reaches request defaults via `modelfile.py`; the chat template is the dangerous version. *How we'll settle it*: T7; if a pack can override the template, the model-pack scope exclusion is revisited.
9. **Risk: T9 is unsizable at plan time.** *Mitigation*: its file list freezes at Wave 0 exit with a line count; >3,000 lines splits into numbered subtasks in SERIES before reading starts. *How we'll settle it*: the frozen list is a Wave 0 exit artifact.
10. **Risk: tasks larger than a session die mid-flight with nothing committed.** *Mitigation*: the ≤1,500-line commit-slice rule; T6 and T11 pre-split. *How we'll settle it*: one commit per slice, each carrying its manifest rows.
11. **Risk: audit helpers break the gate** (shebang-first counting vs the "Expected counts" line — **owner: `RULES.md` § *Project gate* → Expected counts**, which carries both numbers; membership is *derived* by 1b's class-and-path glob, and 2b's printed list is the check on who is in the set, so this row does not restate either). This row cites that owner rather than restating the pair: it held `3 b`/`3 b` for a day after the count moved to `5 b`, which is precisely Rule E.7's failure mode — "an agent who sees it disagree with the live tool concludes drift rather than staleness"). *Mitigation*: inline commands; any unavoidable helper lands with the RULES counts bump in the same commit, and **a new script is linted before it is `git add`ed** — an untracked file is invisible to every gate step. *How we'll settle it*: gate step 1b in each commit's Evidence, and its identity guard (`FAIL(1b) missing product bash: …`) firing rather than a count matching by luck.
12. **Risk: blocked docker steps become silent passes.** *Mitigation*: the Gate status block names which steps are owner-only; a task whose only verifier is blocked is not `done`. *How we'll settle it*: commit messages that name skipped steps.
13. **Risk: effort overrun (14–19 sessions, T5 on the critical path behind an owner-run build).** *Mitigation*: T11's mechanical pass is command-generated; the verdict can be issued at Tier-B completeness with the depth table explicit, and Plan 04 picked up later. *How we'll settle it*: the coverage ledger's time series.
    *Re-scope 2026-11-05:* the overrun happened in T3's evidence layer, not in reading. Execution rule 7's stop rule is the mitigation; the measured estimate is in § Re-scope.

---

## Verification Checklist

- [x] T0: `shellcheck --version` → 0.11.0, gate step 2 → `shellcheck ok`, and RULES.md's step-2 prerequisite states the real path *(verified 2026-11-05: `~/.local/bin/shellcheck`, sha256 `8c3be12b…`, negative control fired SC2115+SC2086, gate step 2 passed)*
- [x] T1: the one freeze-check prints `AUDIT TREE OK`; 0 `MISMATCH` under `GIT_NO_LAZY_FETCH=1`; 862 manifest rows; vendor porcelain-clean *(verified 2026-11-05: `AUDIT TREE OK`, `MISMATCH_COUNT=0`, `awk 'END{print NR+0}'` = **862** across 12 TSVs, `VENDOR_DIRTY=0`, `comm` vs `ls-tree` = 0; all in `context/audit/wave0/t1-freeze-and-manifest.txt`)*
- [x] T2: GitHub's tree SHA for `dc047a7` equals `3f7b69ae…`; boundary-author census recorded with any outliers named *(verified 2026-11-05: `pin^{tree}` = `3f7b69ae4054969ed15b9b6f02d306eb44e0ebe4` = GitHub's tree for the commit; `API_BLOBS=862 LOCAL_BLOBS=862 INTERSECT=862 ONLY_IN_API=0 ONLY_IN_LOCAL=0 BLOB_SHA_MISMATCH=0`, canonical `sha256(path⇥blob)` `a8216c69…` equal both sides; controls C1–C8 all fired. Census: 87 rows, **80 first-party blameable files / 20,111 lines**, Jesus Gulfo 72.4 %, outliers named in `audit/wave0/t2-boundary-authorship.txt` § 6. Two published figures were **corrected after the first draft** — the blame-boundary count (a false `0`; anchored truth 560) and the file denominator (86 → 80) — see `audit/wave0/t2-provenance.txt` § 6 and `pitfalls.md` items 10–14)*
- [ ] ~~T3: **6/6** canaries flagged, including cross-file, side-channel and second-hop, each with its own evidence line~~ *re-scope 2026-11-05: canaries 4–6 dropped (§ Re-scope)*
- [ ] ~~T3: canaries **1–3** (base64 URL, literal `https://`, raw `socket()`) each appear in `sites.tsv` of a `tag=canary` run of the unedited `t3-census.sh`, each with its own evidence line~~ *invalid as written (2026-11-05): the census caught 3 and missed 1–2 — it has no URL/encoded family and drops wide-only hits (`census.md` § Canaries)*
- [x] T3: canary 3 is a census site (`net-socket`, SITES 278→279); canaries 1–2 are caught by the URL/encoded sweep, whose control is exactly **+2/−0** lines on the canary tree *(verified 2026-11-05, `census.md`, `wave0/t3-census-rerun.txt`)*
- [ ] ~~T3: every census row carries a `assigned-TN`; **T9's file list frozen with a line count**~~ *re-scope 2026-11-05: replaced*
- [x] T3: `context/audit/census.md` committed with the `tag=real` run's site table — every site assigned a task (`SITES_WITHOUT_A_TN` PASS) — and T9's file list with its line count *(verified 2026-11-05: `SITES=278`, `SITES_WITHOUT_A_TN=0`, 38 re-routed for the re-scope; T9 = 5 files, 818 lines)*
- [ ] ~~T4: all 9 bypass classes **executed** in the scratch copy with observed outcomes; delta vs upstream's test list written~~ *re-scope 2026-11-05: bypass list is a Follow-up*
- [ ] T4: `image_source.py` and `security.py` read line by line; every outbound connect path named with `path:line` and its allow-list check
- [ ] ~~T5: ASan/UBSan over JPEG/PNG/BMP/GIF at the request-reachable `max_pixels` → **zero** reports; the pixel guard shown to bound the same product `out.rgb.assign` consumes; public CVE PoCs in the corpus~~ *re-scope 2026-11-05: T5 skipped (Follow-up)*
- [ ] T6a: every prompt/completion write site is operator-configured; `/metrics` disclosure enumerated
- [ ] ~~T6b: `before_load` and `--engine-bin`/`--script`/`--config`/`--tokenizer` have no request- or pack-reachable source~~ *re-scope 2026-11-05: merged into T8 — its assertion moves to T8's box*
- [ ] T6c: input-capture and credential-store sweeps over `serve/` empty, sentinel-terminated
- [ ] ~~T7: no request- or pack-supplied string reaches `Template`/`from_string`; tokenizer scan < 1 s on a pathological 40k input~~ *re-scope 2026-11-05: ReDoS half dropped*
- [ ] T7: no request- or pack-supplied string reaches `Template`/`from_string`
- [ ] T8: `argv-table.md` complete across **both** languages, **zero** `request field` origins; the boundary script's non-matches written down — and (from T6b) `before_load` and `--engine-bin`/`--script`/`--config`/`--tokenizer` have no request- or pack-reachable source
- [ ] T9: zero cross-host/IPC endpoints in loans/multi-GPU; `configure.cpp:40`'s `key` operator-only; no surviving plumbed-but-sinkless `host`/`port`/`url` key from `97ff061`
- [ ] T10: no URL/path/`eval` literal in any binary asset; `.prof` structure confirmed by its parser
- [ ] ~~T11: `LC_ALL=C comm` → **0** unaccounted files, one commit per directory; 0 bidi/zero-width hits; vendored-library sweep done~~ *re-scope 2026-11-05: one mechanical commit*
- [ ] T11: bidi/zero-width scan over `src/` `include/` → 0 hits or each explained; every long literal named; one commit
- [ ] T12: 8 canonical names at published versions; 0 install hooks in the repo; `serve/` never reads `benchmarks.json`; hash gap in the verdict headline
- [x] T13: ordered action list with the exact flag per item *(verified 2026-11-05: `context/audit/posture.md`, 5 ordered items; evidence `wave0/t13-posture.txt`)*
- [ ] T14: ≥15 matrix rows, fixed cell vocabulary, no row `proved it can't` without evidence, coverage figure pasted not typed, "if you do only one thing" is the last line
- [ ] All: the gate passes on the final commit for the steps this session can run, and the commit names every step it had to skip and why
- [ ] All: findings harvested into `context/pitfalls.md` + `distilled.md` (Rule D) — bytes on disk, re-read
- [ ] All: Plans 03 and 04 registered in `SERIES.md`
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
