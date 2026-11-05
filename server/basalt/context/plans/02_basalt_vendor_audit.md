# Plan 02 — Basalt vendor audit: does `vendor/basalt` leak, steal, keylog, or phone home?

> **Status**: pending
> **Created**: 2026-11-05
> **Revised**: 2026-11-05 — same day, before execution: added T0 (gate restoration) and a gate-status block; three Key Facts downgraded after self-check; T5 upgraded from "stale dependency" to a sourced CVE finding; T8 widened to cover the **Python** side of argv assembly; T6 and T11 split against the ≤1,500-line slice rule; canaries 3 → 6 (cross-file, side-channel, second-hop); explicit bypass checklist pre-committed in T4; effort corrected 10–14 → 14–19 sessions.
> **Depends on**: N/A (shares Plan 01's `BASALT_SHA` pin; no code dependency — a pin bump invalidates this audit, see Risk 1)
> **Goal**: A per-file, evidenced verdict on whether `vendor/basalt` at the pin does anything beyond acting as a local LLM server — capability-ranked findings, and a finite matrix of every way bytes can leave the box.
> **Agent**: implementation agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## Gate status

Rule A: *"skip is not pass"*, and every commit needs the gate. Recorded per step, with the command
and date, so no session has to guess whether a red step is environmental or theirs. **This block is
the artifact that carries "what the agent could and could not verify."**

| Step | Last run | State | Who can run it |
|---|---|---|---|
| 1a syntax | 2026-11-05 | **pass** (no `FAIL`) | agent |
| 1b counts | 2026-11-05 | **pass** (`3 b`, unchanged by this plan's markdown) | agent |
| 1c `versions.env` | 2026-11-05 | **pass** (`env ok`) | agent |
| 2 shellcheck | 2026-11-05 | **pass** (`shellcheck ok`) — after T0 re-installed it | agent |
| 3 build preflight | 2026-11-05 | **BLOCKED** — `PREFLIGHT_ONLY=1 ./build` exit 1: `permission denied … /var/run/docker.sock` | **owner only** |
| 4 `./run` dry run | — | **BLOCKED** (docker) | owner only |
| 5 flags vs image | — | **BLOCKED** (docker, and needs an image for the pin) | owner only |
| 6 server imports | — | **BLOCKED** (docker) | owner only |

**Standing decision (owner, 2026-11-05)**: the owner runs every docker step; the agent writes the
command and records the pasted output. If a step stays unrunnable for a given commit, that commit's
message and the tracker say *which* step was skipped and why — a task whose behavior only a blocked
step can see is **not** `done` (Rule A.6), and this plan expects several such tasks.

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
- The model pack's weight **bytes** (its loaders that parse them are in scope). Upstream commits after the pin. Anything reachable only through the CUDA driver or GPU we do not control.

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
5. **Audit commands are written inline** (plan/tracker/Evidence), never as helper scripts: gate step 1 classifies by **shebang first**, so even an extensionless `#!/usr/bin/env bash` helper in `context/` counts toward "Expected counts: `3 b`" and fails the gate. If a helper is unavoidable it lands with a bump to that RULES.md paragraph in the same commit.
6. **Every `context/audit/` file opens with a stamp:** `Target dc047a7 / tree 3f7b69ae — if that is not the tree you are looking at, you are in the wrong audit.`

**The one freeze-check** (used by Resume step 1 and by every wave exit — there is deliberately only one):

```bash
cd vendor/basalt && GIT_NO_LAZY_FETCH=1 git diff --quiet dc047a77beeddf1e91234d983a91d6bf929e9619 \
  && echo "AUDIT TREE OK" || echo "AUDIT TREE DRIFTED — STOP"
```

---

## Context

The owner's question, verbatim: *"I want to look file by file, line by line for any malicious code.... is this project doing anything to leak, steal, keylog, send data anywhere on the network that is NOT just the project acting as a nice LLM server/runner?"*

Recon says **clean, actively maintained, security-conscious**: 2194 commits 2026-09-24→2026-11-04, ~30 contributors, no network library in our build files, a deliberately SSRF-guarded image fetcher whose own tests attack it, SHA-pinned CI with no secrets. So the failure mode this plan must avoid is not "missed backdoor", it is **(a)** declaring an all-clear via a search structurally incapable of finding the real risk, and **(b)** spending 40–70 sessions on quantised-matrix kernels while the actual exposure sits in a 2019 image decoder and an optional API key.

### Pre-Identified Bug/Issue

**1 — CONFIRMED FINDING (P0): a vendored image decoder from 2019, compiled with exactly the formats that have had security fixes since, fed by request bytes.**

`vendor/basalt/src/vision/stb_image.h`, 7,988 lines. Its own changelog tops out at **2.21 (2019-02-25)**; upstream is 2.30 (2024-05-31). `STB_IMAGE_RELEASE` is absent because that macro first appears in 2.26 — the missing macro *is* the version evidence.

```console
$ sed -n '60,62p' vendor/basalt/src/vision/stb_image.h
      2.21  (2019-02-25) fix typo in comment
      2.20  (2019-02-07) support utf8 filenames in Windows; fix warnings and platform ifdefs
      2.19  (2018-02-11) fix warning
```

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

Which post-2019 fixes matter **depends on which decoders are compiled in**, and four are (`STBI_NO_STDIO` also guarantees no decoder opens a file):

| Compiled **in** | Post-2.21 security work in that decoder (verified against upstream sources 2026-11-05) |
|---|---|
| **JPEG** | CVE-2022-28041 (signed-integer overflow UB in DC decode / delta accumulation), CVE-2022-28042 (invalid Huffman tables → **out-of-bounds writes**) — both fixed in 2.28; also the resampler OOB read in `stbi__jpeg_load` (nothings/stb#1178, fixed post-2.26) |
| **GIF** | GHSL-2023-145 (`stbi__gif_load_next` wild read), GHSL-2023-148 (double-free `stbi__load_gif_main_outofmem`), GHSL-2023-150 (double-free/leak `stbi__load_gif_main`) — reported **against 2.28**, so later than this copy too |
| **BMP** | the 2.28 `stbi__bmp_load` `psize == 0` hardening (header/offset bounds so the derived size cannot overflow) |
| **PNG** | 2.28 added an IDAT size limit (`> 2^30` rejected) and header-scan changes |
| Compiled **out** | TGA, PNM/PGM, PSD, PIC, HDR, PBM — so CVE-2021-42715 (HDR), CVE-2021-42716 (16-bit PGM OOB read) and GHSL-2023-147 (TGA uninitialised disclosure) **do not apply to our build**. Do not re-raise them. |

Partially mitigating, and T5 must weigh rather than assume: `preprocess.cpp:24-33` refuses images whose header claims more than `max_pixels` (`kMaxImagePixels`, `include/basalt/vision/preprocess.hpp:42`) at `stbi_info_from_memory`, **before** the decode allocates; the 2.21 copy does carry `STBI_MAX_DIMENSIONS` (23 hits) — but that guard caps *dimensions*, not the per-decoder arithmetic the CVEs above live in. Also `stbi_set_flip_vertically_on_load` relevance: 9 `flip_vertically` hits in `src/` — check whether the server path sets it (GHSL-2023-146 / CVE-2023-45662 is `stbi__vertical_flip`).

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
cd /home/iphands/prog/ai/local-mind/server/basalt/vendor/basalt
GIT_NO_LAZY_FETCH=1 git ls-tree -r dc047a77 --format='%(objectname) %(path)' \
  | while read -r h p; do [ "$(git hash-object -- "$p")" = "$h" ] || echo "MISMATCH $p"; done; echo "— blob check done rc=$?"
GIT_NO_LAZY_FETCH=1 git diff --quiet dc047a77 && echo "AUDIT TREE OK"; echo "— rc=$?"
cd .. && git status --porcelain --ignored | head; echo "— vendor status rc=$?"
cd /home/iphands/prog/ai/local-mind/server/basalt && wc -l context/audit/manifest/*.tsv | tail -1; echo "— expect 862 total"
```
**Expected observation**: **Confirms if** 0 `MISMATCH`, `AUDIT TREE OK`, vendor porcelain-clean, 862 rows. **Refutes if** any mismatch, any file that *fails* under `GIT_NO_LAZY_FETCH=1` (that is the silent skip this exists for), or row count ≠ `git ls-files | wc -l` — blocker, not drift. **Within noise if** nothing. One mismatch is a blocker.

**Commit**: `[basalt][P2][T1][audit] freeze the vendor/basalt pin and manifest all 862 files`

### T2: Prove the pin is public upstream's — and who authored the boundary files

**File**: `context/audit/manifest/index.md` (provenance section)

**What to do**: Confirm `dc047a7` is what public `jesdga95/basalt` `main` says it is — checked **against GitHub, never against our own clone** (Rule E) — by comparing GitHub's tree SHA to our `HEAD^{tree}`; check tags (`v0.2.1` is the nearest release per `versions.env`) and whether any are signed, recording "GitHub offers no signature" as a **limit** rather than a clean result; confirm no local commits or untracked files in vendor. **Then the cheap, disproportionate-signal half** (~30 contributors in six weeks — do not treat HEAD as one author): `git log --format='%ae %ad' -- <boundary file>` for `serve/engine/image_source.py`, `serve/security.py`, `serve/service.py`, `src/platform/*`, `src/vision/*`, `data/*`, `serve/web/*`. Flag any boundary file whose history includes a **one- or two-commit contributor** or an email domain off the project's cluster (`gmail`/`users.noreply`/known handles). Attribution is not accusation; it is where a reviewer looks first.

**Verify**:
```bash
gh api repos/jesdga95/basalt/commits/dc047a77beeddf1e91234d983a91d6bf929e9619 --jq '.commit.tree.sha'; echo "— must equal 3f7b69ae…"
cd vendor/basalt && git log --format='%ae' -- serve/engine/image_source.py src/platform src/vision data | sort | uniq -c | sort -rn; echo "— author census rc=$?"
```
**Expected observation**: **Confirms if** GitHub's tree SHA equals `3f7b69ae4054969ed15b9b6f02d306eb44e0ebe4`, and boundary files are dominated by the project's core authors. **Refutes if** the tree SHAs differ or vendor carries a non-public commit — **stop the plan**, nothing downstream is meaningful. An unusual one-commit author on a boundary file is a **finding to read first**, not a refutation. **Within noise if** missing signatures (documented limit), or a noreply-domain contributor that matches an existing GitHub handle.

**Commit**: `[basalt][P2][T2][audit] prove the pinned commit matches upstream and census boundary authors`

### T3: The primitive census, and six canaries that prove it can see

**File**: `context/audit/census.md`

**What to do**: Enumerate boundaries **exhaustively rather than by guesswork**, shipped files and build-time files treated separately. Python: every `import`/`from` in every shipped module, plus the holes — `importlib.import_module(var)`, `__import__`, `getattr`-assembled calls, chr()/base64-assembled names. C++/CUDA: external deps from our build files. **Output contract — each row: `path:line TAB primitive TAB disposition TAB assigned-TN`**; a census row with no task number is unowned work and the Wave 0 gate fails. Sections: network syscalls · DNS · **exec/spawn wide** (`system`/`popen`/`fork`/`exec*`/`posix_spawn`/`os.exec*`/`os.system`/`os.spawn*`/`subprocess` incl. `shell=`) · `dlopen`/`dlsym` · raw `syscall()` · file writes outside expected paths · **fingerprint reads** (C++ `getenv`; Python `os.environ`/`os.getenv`/`platform.node`/`socket.gethostname`/`/etc/machine-id`/mac address/cpuinfo) *paired with* "is there a sink nearby?" — fingerprint-gated activation is how targeted malice hides · credential stores (`~/.ssh`, `~/.aws`, keyring, browser stores, wallet dirs) · `/proc`/`/sys` · persistence (cron, systemd, autostart, rc files) · **input capture** (`/dev/input`, `EVIOCGRAB`, X11 `XRecord`/`XInput`, `uinput`, `SetWindowsHookEx`, `GetAsyncKeyState`) · **parsers that accept network bytes** (image decoders, GGUF/mmproj readers, HTTP framing/chunked-body, JSON, tokenizer) — the class a danger grep cannot see · **vendored third-party** sources · **orphan config keys**: any key in the config schema or the GGUF/profile key space naming `host`/`port`/`url`/`token`/`endpoint` that has plumbing but **no sink** — the highest-signal malice residue available, because it is a network feature with the call site removed and the field kept · **diagnostics-shaped outbound**: search by *shape*, not keyword — a module whose job is assembling a report and handing it to something (a plausible `post_report()` in a plausible `util/`) will not contain `socket()`.

**Canaries (required, separately evidenced — target 6/6)**: copy the tree to `/tmp/opencode/basalt-audit/canary/` and plant, in innocuous homes including one CUDA kernel, one numerical header and one test file:
1. a base64 blob decoding to a URL; 2. a literal `https://` to a throwaway host; 3. a raw `socket()`/`connect()`;
4. **cross-file assembly** — a URL built from constants spread across **two different top-level directories** (every other plan plants single-file; a real maintainer hiding one would not);
5. **side-channel, invisible to the taxonomy** — a routine encoding a secret into **response token count or inter-token latency**: no new primitive, no syscall, no import. It tests whether the *category list* is right, which is what the verdict actually rests on;
6. **second-hop** — data written into a log field that `/metrics` later re-labels and serves: nothing leaves, nothing connects, and the value goes out anyway.

**Verify**:
```bash
cd /home/iphands/prog/ai/local-mind/server/basalt/vendor/basalt
git ls-files 'serve/**/*.py' 'tools/**/*.py' | xargs grep -hE '^\s*(import|from) ' | awk '{print $2}' | tr -d ',(' | sort -u | tr '\n' ' '; echo "— import census rc=$?"
cd /tmp/opencode/basalt-audit/canary && <the identical census commands> | grep -icE 'canary|<planted-host>|socket|<side-channel-symbol>'; echo "— canary count (target 6)"
```
**Expected observation**: **Confirms if** 6/6 flagged, including #5 and #6 — which no keyword in the taxonomy would find, so catching them proves the census was *run*, not that it was *expected* to work. **Refutes if** any is missed. **A missed canary voids the census**: fix it, re-run it, and re-derive every Tier B reason recorded since. **Within noise if** a canary is caught by an unexpected section but caught nonetheless; **never** within noise if it was spotted by hand outside the census.

**Commit**: `[basalt][P2][T3][audit] census every boundary primitive and prove it with six planted canaries`

### T4: The one outbound channel — read line by line, then run the bypass list

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

### T5: The decode path — vendored stb 2.21, sanitized and fuzzed

**File**: `src/vision/preprocess.cpp` and the rest of `src/vision/` (1,584 l, Tier A); `src/program/serve/vision.cpp`; `src/vision/stb_image.h` as **Tier B `vendored-third-party`** — diffed as a dependency version, not read as first-party.

**What to do**: (a) Read the first-party half properly — that is where the non-diffable risk is: `(int) std::min<size_t>(len, 0x7fffffff)`, the `w*h > max_pixels` guard, and `out.rgb.assign(px, px + (size_t)w*h*3)`. **Check the guard is evaluated against the same product the `assign` consumes** — a guard on `w*h` protecting an allocation sized `w*h*3`, or one computed in `int64_t` while a consumer uses `int`, is a real bug independent of stb's own CVEs. Confirm where `max_pixels`/`kMaxImagePixels` comes from and **the maximum a request can actually produce**. (b) Build a **CPU** ASan/UBSan harness in a scratch copy driving `decode_image` over **JPEG, PNG, BMP and GIF only** — seed the corpus with **public PoCs for CVE-2022-28041/28042, GHSL-2023-145/148/150 and the 2.28 BMP offset hardening**, not only random mutations, and run it with `max_pixels` set to the request-reachable maximum so **the harness tests the code rather than being shielded by the guard**. (c) Confirm whether the server path calls `stbi_set_flip_vertically_on_load` (GHSL-2023-146 / CVE-2023-45662 is in `stbi__vertical_flip`). (d) Write the recommendation: a vendored bump to 2.30 in an **upstream** change is the real fix; ours is a `Dockerfile`-side patch, which is a build change and therefore its own plan.

**Verify**:
```bash
cd /tmp/opencode/basalt-audit/stb && g++ -std=c++17 -O1 -g -fsanitize=address,undefined \
  fuzz_decode.cpp preprocess.cpp -o fuzz_decode && ./fuzz_decode corpus-jpeg-png-bmp-gif/; echo "— fuzz rc=$?"
grep -c 'STBI_ONLY' vendor/basalt/src/vision/preprocess.cpp; echo "— expect 4"
```
**Expected observation**: **Confirms if** zero ASan/UBSan reports across the four formats at the request-reachable `max_pixels`, and the guard is shown to bound the same product the `assign` consumes. Then the finding stands as 🟠 *stale vendored dependency with known CVEs in compiled-in decoders, mitigated by an upstream guard we verified*. **Refutes if** **any** ASan/UBSan report on a malformed JPEG/PNG/BMP/GIF — that is memory corruption reachable from an unauthenticated LAN socket and it **overtakes every exfil question as the plan's headline**. **Within noise if** an absurd allocation refused by the `max_pixels` guard (the guard working; record it) — never an OOM *inside* a decoder.

**Commit**: `[basalt][P2][T5][audit] fuzz the stb 2.21 decode path and check the pixel guard against its own consumer`

### T6a–T6c: The server surface, in three ≤1,500-line slices

**T6a — logging and metrics disclosure**: `serve/util/logbuf.py`, `serve/util/telemetry.py`, the logging/metrics parts of `serve/service.py` and `serve/handler.py`. Where do prompts and completions actually go — the engine `--log` file, `logbuf`, `--request-lines`, temp/cache under `--image-dir`, the vision cache — does any survive unload, and is it world-readable in the container? Does `GET /metrics` disclose host identifiers (GPU name, driver, CUDA version, CPU model, core count) to a **remote** caller (an identifier leak, ranked honestly as such, not a backdoor)? Confirm `telemetry.py`'s name is a misnomer by reading it, and confirm the `responses.py` base64 is the `encrypted_content` round-trip.

**T6b — exec trust boundaries → feeds the T8 argv table**: trace `before_load` (`serve/config.py` → `service.py:250`) and prove it is reachable only from operator config/CLI, **never from a request body or from a model pack's GGUF metadata** — `serve/engine/modelfile.py` already maps `general.sampling.*` into request defaults, so check nothing else maps metadata into an exec-capable setting. Same for `--engine-bin`, `--script`, `--config`, `--tokenizer`. Every row you produce lands in `argv-table.md`.

**T6c — the negative sweep (one command, not a reading task)**: T3's input-capture and credential-store sections re-run over `serve/` with the sentinel convention, plus a check that nothing in `serve/` opens `/dev/input*`, `XRecord`, or a keyring.

**Verify**: `grep -nE 'open\(|\.write\(|NamedTemporary|mkdtemp' serve/service.py serve/handler.py serve/util/logbuf.py | head -30`; `grep -rn 'before_load' serve/ --include='*.py' | grep -v tests`; `python3 -m pytest serve/tests/test_metrics.py serve/tests/test_security.py -q 2>&1 | tail -2`; the T6c sweep — all ending in sentinels.
**Expected observation**: **Confirms if** prompt content is written only where an operator configured it; `before_load` and the exec flags have no request- or pack-reachable source; the negative sweeps are empty. **Refutes if** any prompt/completion persisted outside operator config, any request- or pack-influenced value reaching `subprocess`, or a non-empty input-capture/credential sweep. **Within noise if** hardware identifiers on `/metrics` (rank 🟡/🟠 by audience; never drop silently); token counts and timings in logs, which is normal.

**Commit (one per slice)**: `[basalt][P2][T6a][audit] trace server logging and metrics disclosure` · `[basalt][P2][T6b][audit] prove before_load and the exec flags have no request-reachable source` · `[basalt][P2][T6c][audit] sweep serve for input-capture and credential reads`

### T7: Template injection and tokenizer ReDoS on a single-GPU box

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

**What to do**: Where a payload actually hides, and cheap. Entropy-scan and hexdump the `.prof`; grep **every** binary asset for embedded literals (`http`, `://`, `/bin/`, `eval`, printable runs > 16 chars) — a WOFF2 or PNG yielding shell literals is the whole game. Then read the `.prof` **parser** and confirm no field is consumed as an offset/length without bounds and nothing in the file can redirect execution. Re-run (do not inherit) the web-asset external-reference check.

**Verify**: the entropy one-liner; `git ls-files | grep -E '\.(woff2|png|gif|svg|prof|ids)$' | xargs -I{} sh -c 'strings -a "{}" | grep -iE "http|://|/bin/|eval" && echo "  ^^ IN {}"'; `echo "— asset scan rc=$?"`.
**Expected observation**: **Confirms if** no asset yields URL/path/`eval` literals and `.prof` structure is fully explained by its parser. **Refutes if** any literal in any binary asset, or an unbounded offset. **Within noise if** high entropy from a genuinely compressed/float table **whose structure the parser confirms** — never accept "high entropy" as benign without the parser reading.

**Commit**: `[basalt][P2][T10][audit] entropy-scan the shipped binary assets and read the profile parser`

### T11a–T11d: Tier-B triage of the engine bulk, one commit per directory

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

**File**: `run`, `lib/common.sh`, `Dockerfile`, `versions.env` (ours) — **in scope by owner decision 2026-11-05**

**What to do**: Both reviewers independently judged this the audit's highest-value output, because it is where *our* data actually leaves or stays. `--host 0.0.0.0` + `-p 0.0.0.0:8700:8700` with an **optional** API key (`security.exposed_without_key` *flags* a non-loopback bind without a key — determine what `./run` actually **does** about it, and whether `--allowed-host cosmo.lan` widens or restricts); what `/metrics` discloses to a LAN peer; the container's network mode and whether the engine gets egress at all (the cheapest control: **no egress** — which Plan 03 proves); prompt retention in the container's writable layer and whether `--image-dir`/vision cache persists across boots; `--engine-bin`/`--script`/`--config` as trust boundaries we hold.

**Verify**: `img=$(bash -c 'source lib/common.sh; load_pins; image_for_pin'); export IMAGE_OVERRIDE=${img:-stub:none}; DRY_RUN=1 ./run 2>&1 | grep -oE '\-p [^ ]+|--host [^ ]+|--api-key.*|--network [^ ]+'; DRY_RUN=1 ./run 2>&1 | tail -1; echo "— rc=$?"`
**Expected observation**: **Confirms if** a concrete ordered action list, each item the exact flag and its one-line tradeoff. **Refutes if** nothing to change — then say that, don't manufacture recommendations. **Within noise if** an exposure deliberate and documented in `versions.env`/`README.md` as owner-chosen.

**Commit**: `[basalt][P2][T13][audit] review our own run and Dockerfile posture and write the action list`

**Wave 4 exit gate** = the plan closes: verdict written, harvested, moved.

### T14: The channel matrix, the verdict, the harvest

**File**: `context/audit/verdict.md`; findings → `context/pitfalls.md`; confirmed facts → `context/distilled.md`; plan + tracker → `completed/`

**What to do**: Open with **three sentences in the owner's nouns, not ours**: no keylogger / nothing takes your credentials / nothing sends your data out except the reply to whoever asks the server — *and the condition* (unpinned wheels; engine bulk triaged not read). Then the **channel matrix**, ordered by **human consequence**, one glossed line per row ("DNS — could it exfil your prompts via domain names?"): outbound socket · DNS · subprocess spawn · files written outside expected paths · env exfil · persistence (cron/systemd/autostart) · keylog/input capture · credential stores · GPU/NVML identifiers · logs & prompts on disk · build-time pip hooks · venv hooks · model-pack-parsed control flow · IPC/cross-host · **our own config**. Cell text is **not free-form** — exactly `proved it can't (static, T4) — runtime: Plan 03` / `read it, not there (T6a)` / `didn't look → Plan 04`. Free cells are where "inspected" quietly becomes "closed". Never mark a row `proved it can't` on static reasoning alone when Plan 03's runtime leg could settle it. Then the **depth table** (lines read vs present, per tier, named gaps → Plan 04). Then the capability-ranked findings. **End with "if you do only one thing"; put the honesty limit second-to-last** — the hedge must not get the final word: *"this cannot disprove a logic bomb reachable only under inputs we did not run."* No unexplained jargon: spell out SSRF/ReDoS in-line.

**Verify**: matrix rows ≥ 15; `grep -n 'didn.t look' context/audit/verdict.md` — each names a plan; coverage figure traceable to a pasted command; full gate; Rule C move; Rule D harvest check.
**Expected observation**: **Confirms if** every row has evidence or a named plan, and a reader who has never heard of SSRF understands the first three sentences. **Refutes if** any row implies closed without evidence, or the coverage number was typed rather than derived. **Within noise if** wording. The structure is not negotiable: **no matrix, no verdict.**

**Commit**: `[basalt][P2][T14][audit] write the channel matrix and verdict and harvest the findings`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `vendor/basalt/src/vision/stb_image.h` + `preprocess.cpp` | **2.21 vendored decoder; JPEG/GIF/BMP/PNG carry post-2019 CVEs and all four are compiled in**; fuzz + version diff + first-party guard arithmetic | **P0** |
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
3. **Risk: the census's *category list* is wrong, which three same-shaped canaries cannot detect.** *Mitigation*: canaries #4–#6 (cross-file, side-channel, second-hop) are deliberately invisible to the taxonomy; a miss voids the census and forces re-derivation of every Tier B reason. *How we'll settle it*: 6/6, evidenced per canary.
4. **Risk: a static verdict mistaken for the final answer.** Every blind spot (`dlopen`+`dlsym`, raw `syscall()`, venv C extensions, the link's true DT_NEEDED) closes only in Plan 03. *Mitigation*: Plan 03 registered as the runtime gate on this verdict; matrix rows say `runtime: Plan 03`. *How we'll settle it*: Plan 03's close, or the owner explicitly accepting a static-only verdict — recorded as their decision.
5. **Risk: the hash gap makes "we audited what we run" false.** *Mitigation*: named in the verdict headline; fix is a hashed lock (itself a plan). *How we'll settle it*: owner decision, T12.
6. **Question: is `stb_image.h` the only vendored third-party source?** *How we'll settle it*: T11's vendored-library sweep; any second one gets the same version-diff treatment, not a read.
7. **Question: can a request influence any `argv` in either language?** The load-bearing unknown of the engine half. *How we'll settle it*: T8's argv table; a `request field` origin in any element stops and escalates the plan.
8. **Question: can a model pack alter server behaviour?** Its metadata already reaches request defaults via `modelfile.py`; the chat template is the dangerous version. *How we'll settle it*: T7; if a pack can override the template, the model-pack scope exclusion is revisited.
9. **Risk: T9 is unsizable at plan time.** *Mitigation*: its file list freezes at Wave 0 exit with a line count; >3,000 lines splits into numbered subtasks in SERIES before reading starts. *How we'll settle it*: the frozen list is a Wave 0 exit artifact.
10. **Risk: tasks larger than a session die mid-flight with nothing committed.** *Mitigation*: the ≤1,500-line commit-slice rule; T6 and T11 pre-split. *How we'll settle it*: one commit per slice, each carrying its manifest rows.
11. **Risk: audit helpers break the gate** (shebang-first counting vs "Expected counts: `3 b`"). *Mitigation*: inline commands; any unavoidable helper lands with the RULES counts bump in the same commit. *How we'll settle it*: gate step 1b in each commit's Evidence.
12. **Risk: blocked docker steps become silent passes.** *Mitigation*: the Gate status block names which steps are owner-only; a task whose only verifier is blocked is not `done`. *How we'll settle it*: commit messages that name skipped steps.
13. **Risk: effort overrun (14–19 sessions, T5 on the critical path behind an owner-run build).** *Mitigation*: T11's mechanical pass is command-generated; the verdict can be issued at Tier-B completeness with the depth table explicit, and Plan 04 picked up later. *How we'll settle it*: the coverage ledger's time series.

---

## Verification Checklist

- [ ] T0: `shellcheck --version` → 0.11.0, gate step 2 → `shellcheck ok`, and RULES.md's step-2 prerequisite states the real path
- [ ] T1: the one freeze-check prints `AUDIT TREE OK`; 0 `MISMATCH` under `GIT_NO_LAZY_FETCH=1`; 862 manifest rows; vendor porcelain-clean
- [ ] T2: GitHub's tree SHA for `dc047a7` equals `3f7b69ae…`; boundary-author census recorded with any outliers named
- [ ] T3: **6/6** canaries flagged, including cross-file, side-channel and second-hop, each with its own evidence line
- [ ] T3: every census row carries a `assigned-TN`; **T9's file list frozen with a line count**
- [ ] T4: all 9 bypass classes **executed** in the scratch copy with observed outcomes; delta vs upstream's test list written
- [ ] T5: ASan/UBSan over JPEG/PNG/BMP/GIF at the request-reachable `max_pixels` → **zero** reports; the pixel guard shown to bound the same product `out.rgb.assign` consumes; public CVE PoCs in the corpus
- [ ] T6a: every prompt/completion write site is operator-configured; `/metrics` disclosure enumerated
- [ ] T6b: `before_load` and `--engine-bin`/`--script`/`--config`/`--tokenizer` have no request- or pack-reachable source
- [ ] T6c: input-capture and credential-store sweeps over `serve/` empty, sentinel-terminated
- [ ] T7: no request- or pack-supplied string reaches `Template`/`from_string`; tokenizer scan < 1 s on a pathological 40k input
- [ ] T8: `argv-table.md` complete across **both** languages, **zero** `request field` origins; the boundary script's non-matches written down
- [ ] T9: zero cross-host/IPC endpoints in loans/multi-GPU; `configure.cpp:40`'s `key` operator-only; no surviving plumbed-but-sinkless `host`/`port`/`url` key from `97ff061`
- [ ] T10: no URL/path/`eval` literal in any binary asset; `.prof` structure confirmed by its parser
- [ ] T11: `LC_ALL=C comm` → **0** unaccounted files, one commit per directory; 0 bidi/zero-width hits; vendored-library sweep done
- [ ] T12: 8 canonical names at published versions; 0 install hooks in the repo; `serve/` never reads `benchmarks.json`; hash gap in the verdict headline
- [ ] T13: ordered action list with the exact flag per item
- [ ] T14: ≥15 matrix rows, fixed cell vocabulary, no row `proved it can't` without evidence, coverage figure pasted not typed, "if you do only one thing" is the last line
- [ ] All: the gate passes on the final commit for the steps this session can run, and the commit names every step it had to skip and why
- [ ] All: findings harvested into `context/pitfalls.md` + `distilled.md` (Rule D) — bytes on disk, re-read
- [ ] All: Plans 03 and 04 registered in `SERIES.md`
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
