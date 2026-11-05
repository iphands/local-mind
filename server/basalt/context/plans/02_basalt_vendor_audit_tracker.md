# Plan 02 — Basalt vendor audit: does `vendor/basalt` leak, steal, keylog, or phone home? — Tracker

## Overview

- Status: 0% complete (0/19 slices: T0–T5, T6a–c, T7–T10, T11a–d, T12–T14)
- Start date: — (set when T0 starts; T0's tooling work was done at plan creation, its commit is pending)
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
   **`reviewed-in=` column state in `context/audit/manifest/index.md`**. This tracker says which
   task is next; those two say which *file* the next session should open. Trust both, then read
   `context/audit/census.md` for the assigned-site list.
3. Read the project `AGENTS.md`, `context/plans/RULES.md` **in full**, then the plan — **its Gate
   status block and Execution rules 1–6 bind every task** (≤1,500-line commit slices; never write
   inside `vendor/basalt/`; sentinel-terminated commands; inline commands not helper scripts; the
   audit-file stamp).
4. **Environment — what works and what does not in this session:**
   - Working: `python3` (3.14.10), `git` (≥2.43, `GIT_NO_LAZY_FETCH`), `gh`, `g++` (T5 harness),
     `shellcheck` 0.11.0 **at `~/.local/bin/shellcheck`** (installed by T0 on 2026-11-05; RULES.md
     updated).
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

## Progress

| # | Slice | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 0 | T0: Restore the gate (shellcheck user-local, RULES.md prerequisite) | `context/plans/RULES.md` | pending | tooling already done at plan creation; commit pending |
| 1 | T1: Freeze the pin, manifest all 862 files | `context/audit/manifest/` | pending | |
| 2 | T2: Pin provenance + boundary-author census | `manifest/index.md` | pending | gates Wave 0 |
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
