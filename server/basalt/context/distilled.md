# Distilled — confirmed facts for basalt

Compact, confirmed learnings: formats, protocols, tool/library behavior, measured limits. Read
before new work; add an entry whenever something is confirmed — **above the `## Open Questions`
section**, which always stays last. Keep it dense.

**Provenance discipline:** every entry carries a tag — `[SOURCE]` (upstream code, cite
`path:line`), `[DOC]` (documentation), `[REPO]` (this repo's own code/config), `[LIVE YYYY-MM-DD]`
(observed on a running system). **Do not upgrade a tag without doing the work.**

<!-- Entry shape:

## <Topic — the rule in one line> [TAG][TAG]

- The fact, stated so it can be checked.
- The evidence: command + verbatim output, or `path:line`.
- What it means for this project.
-->

## Basalt loads only Qwen3.8-Flash-Next, from single-file `.basalt` packs [DOC 2026-11-04]

- Basalt (jesdga95/basalt, a fork of Strata, MIT) is "a local inference engine for the
  **Qwen3.8-Flash-Next** mixture-of-experts model" (`README.md` at `bbf2f5bb7`). A pack is one
  `.basalt` file: weights, tokenizer, MTP drafter, 8-bit hyper-connection copies, vision
  projector. Expert profiles ship with the engine (`data/profiles/qwen4exp-48x512.prof`), keyed
  `<general.architecture>-<block_count>x<expert_count>` (`docs/model-file.md:83`).
- Ready packs: HF `jesdga/Qwen3.8-Flash-Next-Basalt` — `Qwen3.8-FN-NVFP4-DQ6_K` (default, faster),
  `-NVFP4-DQ8` (~7% slower on prose, lower KLD), `-UD-Q4/5/6_K_XL`, `-Q8_0`, `-GSQ-RCO-*`.
- **Not loadable:** `models/vllm/jesdga/Qwen3.8-27B-QUASAR-DFlash2-nvfp4-NInfer/qwen3_8_27b_quasar.ninfer`
  (19,624,516,612 bytes) — a dense Qwen3.8-27B artifact for NInfer (its `README.md`:
  `library_name: ninfer`, tested at NInfer `d44ab584`). Same HF author, different engine. It belongs
  in `../ninfer`.
- No `.basalt` file exists under `/mnt/noir/scratch/ai/llm/models` yet
  (`find … -iname '*basalt*'`, 2026-11-04).

## Building Basalt: CUDA 13, sm_120, CMake ≥ 3.24, GCC ≥ 13, Python ≥ 3.10; nothing fetched [DOC 2026-11-04]

- Requirements: Linux x86-64 with AVX2 (AVX-512 used when present), compute capability 12.0,
  CUDA 13, CMake 3.24+, GCC 13+ (`bbf2f5bb7` "docs: the Linux build needs GCC 13 or newer"),
  Python 3.10+ (`README.md`).
- Build: `cmake -S . -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build -j` →
  `bin/basalt`, `bin/basalt-bench`, `bin/basalt-probe`, `bin/basalt-vision`. Server deps:
  `pip install -r requirements.txt` (pinned: numpy, jinja2, regex, pillow, psutil, …).
- "The build needs nothing outside this repository and fetches nothing" (`docs/building.md`).
- An install or container image **ships `data/` with the binaries, `serve/` and `tools/`**
  (`docs/building.md`); `--expert-profile FILE` names the profile explicitly (0.2.1).
- `basalt-vision` is built for one card at configure time (`-DCMAKE_CUDA_ARCHITECTURES=<cc>`,
  `docs/running.md:247`).
- Releases: tags `vX.Y.Z`, no binaries; 0.2.1 on 2026-11-03. Upstream keeps **no shims**: a
  removed or renamed flag is just an unknown-flag error (`AGENTS.md`, changelog index).

## Serving: `bin/basalt serve --config basalt_config.json`, binds 127.0.0.1 by default [DOC 2026-11-04]

- Config shape (`README.md`): `{"server": {"model": "<pack>", "gpu": "0", "port": 8080},
  "engine": {"expert-cache": "auto", "prefill": "auto", "kv": "int8", "resident-experts": true}}`.
  Every setting: upstream `docs/configuration/index.md`.
- "The server listens on 127.0.0.1 by default" — inside a container that means unreachable from
  the host unless it binds `0.0.0.0` or the container uses host networking.
- OpenAI- and Anthropic-compatible API plus a web UI/stats page (`serve/`).
- Experts that do not fit in VRAM live in system RAM (optionally a second Blackwell GPU as a peer
  tier). `--gpu auto` uses every visible Blackwell GPU (`docs/building.md`).

## DQ6_K pack on this card: every expert fits in VRAM; 40 s boot from local NVMe [LIVE 2026-11-04]

- Boot 3 (`./run` default: 4 slots × 262144, `kv int8`, `vision dynamic`, pack in `models.local/`):
  `loaded 67.8GiB 40.1s cuda0=67.8GiB` at ~2.7 GB/s; `expert cache 24576 slots, 63.29 GiB of VRAM` —
  48 layers × 512 experts, **all of them** (start warning: `the device-planned MoE tail is off: every
  expert is resident`). *Fixed 2026-11-04 (T7): this line first said "so the decode never touches the
  CPU tier" — wrong.* A RAM copy sits beside the cache (`resident RAM: 22.81 GiB of experts in RAM`)
  and served `201337 blobs` over boot 3's life: the experts whose slots the elastic KV borrows. The
  5950X's AVX2-only CPU path is used only for those.
- The 4 full-context slots: `kv 1.5/15.2GiB cuda0 elastic slots=4` — 4096 cells per session backed at
  start; the other 13.73 GiB is the expert cache's last 5,334 slots, taken back as long conversations
  grow (up to 22% of the experts then run on the CPU).
- GPU 0 idle after load: 73,091 / 97,887 MiB, 73 W. One short reply: 320.7 t/s, 74% drafts accepted,
  TTFT 0.1 s (one run — not a benchmark).
- The local copy loaded fine although cosmo's `/home` (`compress-force=zstd:1`) stored 464,061 of its
  555,406 extents compressed.
- `--single` (boot 4): `loaded 67.8GiB 34.4s`; still all 24,576 slots / 63.29 GiB in VRAM;
  `kv 0.7/4.0GiB cuda0 elastic` (32,768 cells backed; the other 3.30 GiB = the cache's last 1,284 slots);
  `resident RAM: 12.38 GiB`. Client traffic on it: a 26,676-token prompt read at 9,842 t/s, then 9,304
  tokens at 357 t/s, 67% drafts accepted (one request each; not a benchmark).
- `docker stop basalt` → 2.3 s, VRAM freed (SIGTERM handled).

## Basalt loads weights with `O_DIRECT`, and there is no switch [SOURCE][LIVE 2026-11-04]

- `include/basalt/core/direct_upload.hpp:1-6` at `dc047a77`: a load phase copies file ranges to VRAM
  through block-aligned `O_DIRECT` reads into pinned staging buffers (falls back to buffered reads
  only when the filesystem refuses `O_DIRECT`; NFS accepts it). `src/platform/direct_file.cpp:384`
  opens with `O_RDONLY | O_DIRECT`. No CLI flag or env var turns it off (`getenv` in `src/`:
  `CUDA_VISIBLE_DEVICES`, `CUDA_MODULE_LOADING` only).
- Live: boot 2 of the DQ6_K pack from `/mnt/noir` (NFS 4.2/RDMA) showed 6 engine threads in
  `nfs_file_direct_read` and a load of ~5–10 MB/s. Buffered `dd` of the same file measured 28.9 MB/s
  during the boot, so the NFS link was slow too that evening (not separated — Plan 01 tracker).
- Means: expect basalt boots from NFS to be slow; local NVMe is the layout upstream targets.
- **Cause on noir (2026-11-04, read-only over `ssh helper@noir.lan`):** `/main` is btrfs on a 14.6 TB
  PERC H710P virtual disk of **spinning** drives (`rotational=1`, `read_ahead_kb=128`); the pack had
  **131,723 extents**, almost none physically adjacent (125,506 ≤ 1 MiB), from a parallel, out-of-order
  download — so a cold read is ~125k seeks, worse with Basalt's six readers. With the file in noir's
  page cache, direct reads from cosmo ran 749 MB/s (16 MiB) / 164 MB/s (1 MiB). The owner then ran
  `btrfs filesystem defragment -v -f -t 128M` → **927 extents** (128 MiB is btrfs's max extent; ~980
  is the floor). `+m` was already set; 615 compressed extents (~77 MB) remain, as defrag without `-c`
  leaves compressed extents alone.

## The built image: `iphands/basalt-blackwell:cu1343-sm120-dc047a7` [LIVE 2026-11-04]

- Owner build (Plan 01 T4): 3.42 GB, `ai.basalt.ref=dc047a77…`; Basalt's compile is 507 ninja steps
  (≈ 420 s at `JOBS=16`, 0 warnings) — `BUILD_TESTING=OFF` still builds the parity programs, but
  only `bin/` ships. `bin/basalt` is 94.1 MB.
- `bin/basalt --help` and `serve.server --help` run with **no GPU** (`--network none`, no `--gpus`),
  so flag checks need no card.
- `basalt serve --mock-engine` runs the whole server with a scripted engine and no model or GPU —
  the cheapest end-to-end check of the image layout, the Host check and the alias.

## Host check and alias behave as documented [LIVE 2026-11-04]

- With `--host 0.0.0.0 --allowed-host cosmo.lan --allowed-host cosmo`: `Host: cosmo.lan:8700`,
  `cosmo:8700`, `localhost:8700` → 200; `evil.example:8700` → **403** (mock engine, T4).
- With `--alias cosmo-6000`, `/v1/models` lists only `cosmo-6000`; a chat naming `claude-heavy` →
  **404**.
- The start line prints `ready http://127.0.0.1:8700/v1` even when bound to `0.0.0.0`; the
  `network` line gives the container address. Without `--api-key`:
  `[server] warning  no api key, open to the network (--api-key KEY)`.
- `docker stop` ends the server in < 1 s (SIGTERM handled; `serve/server.py:301`).

## Image build inputs: BuildKit honors `Dockerfile.dockerignore`; the runtime base has no Python [LIVE 2026-11-04]

- The build context is `vendor/basalt`, which has no `.dockerignore`. BuildKit reads
  `<Dockerfile>.dockerignore` beside the `-f` Dockerfile: with ours, the context is 11.75 MB and
  holds no `.git` (scratchpad probe build, Plan 01 T2 Evidence).
- `nvidia/cuda:13.4.3-runtime-ubuntu24.04@sha256:f58bd7a0…` has no `python3`; apt's candidate is
  3.12.3 (`python3`, `python3-venv`). The devel base: cmake 3.28.3, g++ 13.2.0, ninja 1.11.1,
  nvcc 13.4.92 — all above Basalt's minimums.
- `PREFLIGHT_ONLY=1 ./build` → `preflight ok` at `dc047a77`; `docker build --check` sends no context.

## This host: one sm_120 card, and one Basalt server per card [LIVE 2026-11-05][REPO]

- Booting a second server on GPU 0 is not possible while one runs, in **any** mode: the load is the
  expert cache, and `--single` shrinks KV slots, not the cache. Read on the owner's live server:
  `expert_slots_primary 24576`, `expert_cache_primary_mib 64806`, `arena_mib 23353`, against
  `vram_free_mib 24435`; `nvidia-smi` showed 77,117 / 97,887 MiB used. Consequence for plans: a
  comparison of two modes is **sequential** — stop, load, measure, stop, load — never concurrent.
- `./run` enforces it before docker starts: `refuse_busy "$PORT" vllm ninfer ignis basalt` (`run:96`),
  so a clashing boot exits 1 with `## busy:` and touches nothing. In `DRY_RUN` it *reports* the clash
  instead of refusing (`run:92-97`) — which makes `DRY_RUN=1 ./run` the port oracle for this project.
- Expert residency at 4×262144 is **tiered, not "all in VRAM"**: a 24,576-slot / 63.29 GiB primary cache
  resident on cuda0 (`expert cache 24576 slots, 63.29 GiB of VRAM`) *and* `resident RAM: 22.81 GiB of
  experts in RAM, 0 exchanged with the VRAM tier`; `basalt verify: warning: the device-planned MoE tail
  is off: every expert is resident`. No starvation warning at 4 slots, and the extra slots are cheap:
  `--batch 4: 0.12 GiB of KV cache and state per slot (262144 tokens), 0.49 GiB in all; the slots beyond
  the solo one take 0.37 GiB of VRAM on CUDA0 that the expert cache would otherwise hold`.
- **Provenance limit on the three bullets above**: they were read from the **owner's running
  daily-driver** on 2026-11-05 (`/metrics` + its `engine.log`), launch provenance unconfirmed — the
  card held three `--batch 4` boots logged that morning (*UTC morning = 2026-11-04 evening PST; the
  "daily-driver" was Plan 01's T6 test boot 3 — host agent session, 2026-11-04*), none `--single`. They corroborate Plan 01's
  boot 3; they are not a substitute for T6's directed boots, and `--single`'s geometry is still unknown.

## This host: one sm_120 card, 125 GiB RAM [LIVE 2026-11-04]

- `nvidia-smi`: GPU 0 RTX PRO 6000 Blackwell Workstation, 97,887 MiB, cc 12.0; GPU 1 RTX 4060,
  8,188 MiB, cc 8.9 (not usable by Basalt). `free -g`: 125 GiB total.
- docker 29.8.1, shellcheck 0.11.0, python3 3.14.10; no `gh`, no `hadolint`.
- **Tool versions here are the 2026-11-04 shell's view, not a host constant** (checked 2026-11-05 from
  the agent container, which reports docker client 29.9.2 and keeps `shellcheck` at `~/.local/bin`, and
  has no `ss`). Re-check in the shell that will use the answer — see
  `context/pitfalls.md` § *Empty output from a failed command was recorded as a measurement*.

---

## Basalt's history cannot be diffed against the project it forked from — the two repos share 0 commit SHAs and have no merge-base [LIVE 2026-11-05]

`jesdga95/basalt` declares itself (in the pinned tree's `AGENTS.md:3`, `CONTRIBUTING.md:56`,
`README.md:68`) a fork of `Niko1221/Strata`, and GitHub reports `fork=false` — which means **no
fork-network link**, not that the relationship is false. Measured at `dc047a77` against a pristine
`git fetch --filter=tree:0 --no-tags origin main` of Strata: **5,637 commits** on
`refs/remotes/origin/main` (`ffa80d73…`). Pristine-check first: `count-objects` went `in-pack 0` → 5,637
after the fetch, and there is no `objects/info/alternates`. Result: **0 shared commit SHAs, different
root commits, no merge-base.** They are two rewritten variants of one history — **844** commit pairs match
on the content key `(tree, author-date, committer-date, subject)`, with **0 duplicate keys on either side**
(2,194 distinct keys across our 2,194 commits; 5,637 across their 5,637).

Our 2,194 commits split at **2026-10-05T00:00:00Z** (the Software Heritage visit date, one day before
`jesdga95/basalt` was created): **861 before, 1,333 after**. **844 of the 861 (98.0 %) have a content twin
in Strata's `main`; 0 of the 1,333 after do.** That is the fork boundary measured by content, not by a
merge-base. The **17 unmatched pre-fork commits** are all authored 2026-09-27 … 2026-10-04 — 16 by
`Jesus Gulfo` (the `STRATA_*` opt-in kernel/serve work) and one Niko1221 housekeeping commit — i.e. work
in our line on the eve of the fork that Strata's *current* `main` does not carry; whether upstream dropped
it or rewrote it out is **not established**. None of them is a `src/platform/` seam commit. Two
consequences: the carried-over line is bounded at **≤ 2026-10-04**, and 17 pre-fork commits join the
"no external existence" set that § 6's read order rests on.

> ⚠ **A date partition is undefined until its basis is named.** This entry published **882 / 1,312** for
> months-equivalent of the same history, because the script compared `--date=iso-strict` output as a
> *string*, which partitions by the **author's local calendar date**; on a **UTC instant** it is
> **861 / 1,333**. Both are internally consistent; the UTC basis is the one quoted from here on
> (`98.0 %`, 17 unmatched). `pitfalls.md` item 16.

Canonical figures for this entry: `audit/wave0/t2-lineage.txt` §§ 0–4 (raw output and controls).
Command-level traps: `pitfalls.md` § *Cross-repo git forensics*.

- **The strongest form of the measurement** (`t2-lineage.txt` § 11b): fetch **both** projects into **one**
  scratch store with the same command, then `comm -12 <(git rev-list A|sort) <(git rev-list B|sort)` →
  **`|A|=2194  |B|=5637  |A∩B|=0`**, with the store self-certifying (`in-pack` 7834 = 2194 + 5637 + 3 tag
  objects) and every count re-run under `GIT_NO_LAZY_FETCH=1`. It also cross-checks the `A..B` arithmetic
  both directions (`strata..pin` = 2194 = `|pin|`, `pin..strata` = 5637 = `|strata|`) against independently
  counted totals, and gives `v0.2.1..pin = 3` as a third *mechanism* (second *source*) for the engine fact.
  Note `git merge-base` between unrelated histories is **silent** (rc=1, no output), and
  `merge-base --independent` is **not** an unrelatedness test — see `pitfalls.md` § *Cross-repo git
  forensics*.

- **Consequence for any future work here:** `git log <strata>/main..<pin>`, `git cherry`, cross-repo
  `bisect` and blame continuity **run and print a confident wrong answer** (with no merge-base, `A..B`
  is just `B`). `git diff A B` is the exception and stays sound — it is tree-to-tree. Join these
  histories by **content key**, files by **blob SHA**, changes by **`git patch-id --stable`**.
  **Never compare *commit* SHAs between the two repositories** — they share none, which is what makes the
  tools above necessary rather than merely tidier. The ban covers commit identity only: the blob-SHA and
  patch-id joins named one clause earlier are the intended method, and `plans/02_basalt_vendor_audit.md`
  § **T2b** (U12) requires them.
- **Version names collide.** Basalt publishes 3 tags, Strata 95 (to `v0.2.10`); `v0.1.0` and `v0.2.1`
  exist on both sides pointing at **different commits**. An unqualified `v0.2.1` is ambiguous —
  in this project's files it means **basalt's** (`1b3f6964`, tree `515676c1`).
- The fork's own metadata is intact: 0 files under `src/`, `include/` or `cmake/` mention Strata, so
  lineage is invisible in the build and visible only in git.

## In this history, 71 % of pre-fork commits and 95 % of basalt-era commits self-declare AI co-authorship — disclosure, not authorship [LIVE 2026-11-05]

**Read the qualifier before the numbers.** Basalt's own `AGENTS.md` **instructs** contributors to end each
commit message with the co-author line their tool specifies, so these trailers are a **house convention**
that records what the committer *claimed* about how the commit was made. A trailer is **disclosure, not
authorship**: the defensible sentence is *"this history self-identifies as substantially AI co-authored"*.
It is never a statement that AI wrote the code, and it is never a share of *code originality* — 94.6 % is a
share of lines in 80 files under one blame pass, and code originality has no denominator here.

Method: `git log -z --format='%H\t%B'` (NUL between records, tab inside them — the other form silently
returns zero, `pitfalls.md` item 12), matched case-insensitively against
`co-authored-by: (claude|openai|chatgpt|copilot)` / `claude-session:` / `generated-with:`. Every population
below names itself, because the four counters are similar in magnitude and are not interchangeable.

**The 80**: the 87 rows of `audit/wave0/t2-author-census.tsv` minus the 6 `BINARY_SKIPPED` Tier A assets and
the 1 vendored `stb_image.h` (`B-vendored@T5`) — 69 `A-planned` + 11 still `unassigned` in the manifest, so
the set is **census-derived, not tier-derived**, and its provenance is that file, not a tier name.

| Population (each row is a different denominator) | With an AI trailer | Share |
|---|---|---|
| Commits reachable from `dc047a77` (whole history) | 1,881 / 2,194 | 85.7 % |
| … **pre-2026-10-05 UTC** — the inherited pre-fork line | 612 / 861 | **71.1 %** |
| … **post-2026-10-05 UTC** — work in `jesdga95/basalt`'s own repo | 1,269 / 1,333 | **95.2 %** |
| Blamed lines in the **80 first-party census files** (9 % of the 219,247 audited lines) | 19,035 / 20,111 | 94.6 % |
| Distinct commits those blames credit | 329 / 367 | 89.6 % |
| `Niko1221/Strata refs/remotes/origin/main`, as fetched 2026-11-05 | **3 / 5,637** | 0.05 % |

- **The pre/post split is the point.** 85.7 % is a property of *inherited* history two-thirds of the way
  through; the share inside basalt's own repository is **95.2 %**, and the ancestor's own pre-rewrite line
  was already **71.1 %**. So this is not "basalt is 86 % AI-generated", and it is emphatically not "Strata
  was human-written".
- **The last row is a trap, not a contrast.** That line was rewritten (next entry), so post-edit it is not
  a disclosure record at all; the 3 remaining trailers were not individually characterised. Read it as
  *AI involvement established in our line, unresolved in theirs where the disclosure was removed.*
- **Capability rank is unaffected** — a metadata finding has no capability dot; it is recorded for
  **evidential weight**. **Canonical figures, owned by** `audit/wave0/t2-provenance.txt` § 10 (it prints
  every population above); heaviest files and the read-order consequences are owned by
  `audit/wave0/t2-boundary-authorship.txt` § 6.

## The ancestor project rewrote its published mainline; the only measured delta is AI-attribution metadata [LIVE 2026-11-05]

Across the 844 byte-identical-content commit pairs described above: Claude trailers **1,049 ours / 0
theirs**, human co-authors **22 / 22**, Codex trailers **3 / 3** — one tool's attribution removed, every
other trailer preserved. The two root commits are identical in tree (`4591e4ca…`), author, committer and
**both** epochs, differing only by our root's extra `Co-Authored-By: Claude Opus 5.5 (1M context)` and
`Claude-Session` lines. Corroboration: Software Heritage's 2026-10-05 visit records **Strata's own
`refs/tags/v0.1.0` at *our* trailer-bearing root**, where that tag points at the trailer-free twin today;
and `jesdga95/basalt` was created `2026-10-06T22:17:02Z`, i.e. **after** that record, so the
trailer-bearing line was the ancestor's published release before our fork's repository existed.

- **Motive is not established**, and preserving both epochs while editing only the message is
  `git filter-repo`'s *default* behaviour, which is a weaker claim than concealment. Four innocent
  explanations for basalt keeping the old line (compliance without erasure, a clean-room re-derivation with
  no motive at all, a third-party tooling accident, a custody transfer) are at
  `audit/wave0/t2-lineage.txt` § 5.
- **The method this forces on the audit: narrative history is editable; hash pins, trees, blobs and
  patch-ids are not.** Commit messages, author fields and tag targets are **not** evidence in this
  supply chain. Anchor every external claim to a named snapshot hash and a date, and check the object
  store before believing any prose — including a forge's metadata and this file's own summaries of it.
- **Not established:** who ran the rewrite, whether one party controlled the ancestor's origin URL at
  both dates, and therefore anything about *intent*.
  Canonical detail, each in `audit/wave0/t2-lineage.txt`: the four innocent explanations **§ 5**, the 2×2
  remote-presence matrix **§ 2**, the Software Heritage visit record **§ 7**, both-histories-in-one-store
  controls **§ 11b**.

## The pin's C++ engine is byte-identical to basalt's own `v0.2.1`; only `serve/` and docs moved [LIVE 2026-11-05][REPO]

`versions.env` asserts *"the engine is identical to v0.2.1"*. Measured at `dc047a77`: **3** commits and
**9** changed files since basalt's `v0.2.1` (`README.md`, three under `docs/`, five under `serve/`), **0**
under `src/` / `include/` / `cmake/` / `tools/` / `CMakeLists.txt`; over those engine paths
`|v0.2.1|=553`, `|pin|=553`, `|intersection|=553`, symmetric difference **0**. So Plan 01's Key Facts row
for this pin — written from GitHub's `compare/v0.2.1...main` on 2026-11-04 — is re-confirmed by local blob
identity, a stronger mechanism over the same source. **Three mechanisms, two sources**: GitHub's compare API
and a local `rev-list` at a tag fetched from GitHub both rest on *what `jesdga95/basalt` publishes today*, so
they would agree wrongly if the tag moved; the only corroboration from a different pair of objects is
`multimodel..main = 3` with merge-base at the `v0.2.1` peel target, measured in a different clone
(`audit/wave0/t2-lineage.txt` §§ 11, 11b). Practical effect: Plan 01's characterisation of the C++ engine
carries to this pin, so Plan 02's `src/platform/` reads start from partly-characterised ground; the stalest
point is `CMakeLists.txt` (287 commits, `unassigned` in the manifest).

## Open Questions

- ~~Does an NVFP4 Flash-Next pack fit entirely in 96 GiB VRAM, or which share lands in RAM, and
  what `ulimit -l` / `--ulimit memlock` does the container need for the RAM tier to page-lock?~~
  RESOLVED (Plan 01 T6): every expert fits in VRAM (DQ6_K entry above), with a RAM copy beside it;
  `--ulimit memlock=-1` gives `unlimited` in the container and Basalt printed no fallback warning.
- Which of the image's upstream defaults (`expert-cache`, `prefill`, `kv`, `--spec` depth) are
  already right for this card? `bin/basalt-probe` "suggests values" (`docs/probe.md`) — owner run.

<!-- Things we believe but have not confirmed. Check here before trusting a related assumption;
     move an item up (with its tag) once it is confirmed, or into pitfalls.md if it bit. -->
