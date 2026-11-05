# Plan 01 — Basalt Flash-Next container: pinned sm_120 image, `./run` on :8700 — Tracker

## Overview

- Status: 71% complete (5/7 tasks)
- Start date: 2026-11-04
- Plan: `context/plans/01_flashnext_container.md`
- Evidence: the table below; build logs in `captures/build_<date>_<sha7>.log` (gitignored); boot
  start blocks pasted by the owner into the Evidence rows. Test environment: host `cosmo`, RTX PRO
  6000 (GPU 0), 125 GiB RAM, docker 29.8.1, the pack on NFS (`models/vllm/jesdga/`).

## Resume Instructions

1. Read `AGENTS.md`, `context/plans/RULES.md` (in full), then `01_flashnext_container.md`, then
   `context/distilled.md` and `context/pitfalls.md`.
2. Environment: docker as yourself (no `sudo`); `shellcheck` 0.11.0. The reference scripts are
   `../ninfer/{lib/common.sh,build,run,Dockerfile}` and `../vllmcustom/common/_run-lib.sh` — read,
   never sourced or edited.
3. Ordering: Wave 1 (T1 → T2 → T3) is agent work and needs no GPU. T4 needs the owner's `./build`;
   T5 needs T4's image; T6 needs the owner to boot with `:8700` free (stop `vllm`/ninfer/ignis
   first); T7 last. Mark a task `blocked` (with the reason) while it waits on the owner.
4. Commit per task as `[basalt][P1][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**. Never push.

## Open Unknowns

- VRAM/RAM expert split at 4×262144 — **measured, and tiered** (2026-11-05: boot 3's own lines plus the
  owner's live server): a 24,576-slot / 63.29 GiB primary expert cache resident on cuda0 *plus* a
  `resident RAM: 22.81 GiB of experts in RAM, 0 exchanged with the VRAM tier`; KV elastic 1.5/15.2 GiB,
  `expert_cache_primary_mib 64806`. An earlier phrasing here — "all 24,576 experts in VRAM" — was wrong
  against our own Evidence and is corrected in place. `--single` still to boot.
- ~~Does `bin/basalt --help` run without a GPU? (plan Q4 — T4)~~ RESOLVED (T4): yes — `docker run --rm --network none … --help` → rc=0, 198 lines, no `--gpus`.
- ~~Does the RAM tier page-lock inside the container with `--ulimit memlock=-1`? (plan Q2 — T6)~~ RESOLVED (T6 boot 3): the limit is `unlimited` inside the container and Basalt printed no page-lock / mmap-fallback warning; every expert sits in VRAM anyway (`/proc` `VmLck`/`VmPin` do not count CUDA-pinned memory).
  - *[Corrected in place 2026-11-05: two writers edited this bullet inside the same minute and a stray
    agent clause had been glued onto the end of that sentence. It is removed; the owner's verdict
    stands as written above. The bullet below is the agent's scope note, kept separate on purpose.]*
  - **Scope of that RESOLVED (agent, 2026-11-05).** Proven: *nothing failed for want of memlock* —
    `ulimit -l` unlimited, `Max locked memory unlimited unlimited`, no fallback warning — so
    `--ulimit memlock=-1` in `./run` is sufficient and stays. **Not** proven: the clause "every expert
    sits in VRAM". The owner's own row says `VmLck 0`/`VmPin 0` cannot show CUDA-pinned memory, and the
    RAM copy is real rather than vestigial — `resident RAM: 22.81 GiB of experts in RAM, 0 exchanged
    with the VRAM tier`, with engine RSS 32,987,960 kB ≈ 31.5 GiB pointing the same way. So: the narrow
    question is closed; *"which tier answers expert hits, and is the RAM copy page-locked?"* stays open
    and settles at `--single` (plan Q3, where the residency warning may switch off) or with a lock
    counter — not on the absence of a warning.
- VRAM/RAM split and KV size for 4×262144 vs 1×262144 on this card (plan Q1, Q3 — T6)
- ~~Runtime image's Python version (Key Facts — T2)~~ RESOLVED (T2): the runtime base has no python3; apt candidate `python3`/`python3-venv` 3.12.3-0ubuntu2.1 (Evidence).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Pins, shared guards, project skeleton | `versions.env`, `lib/common.sh`, `.gitignore`, `models`, `README.md` | done | Digests unchanged since ../ninfer pinned them; sudo / missing-docker guards refuse (rc=1); gate `1 b`, `env ok`. |
| 2 | T2: Dockerfile + `./build` | `Dockerfile`, `Dockerfile.dockerignore`, `build` | done | Preflight ok at `dc047a77`; dirty (untracked) checkout refused; context probe: 11.75 MB, no `.git`; no image built. Gate step 3 added. |
| 3 | T3: `./run` + its dry-run gate step | `run`, `README.md`, RULES gate | done | Gate step 4 added and passing with `image: stub:none` (no image yet); negative control `KV=fp16` → 2 FAIL lines; busy/ninfer/missing/not-a-pack all rc=1. |
| 4 | T4: Owner build; verify the image is the pin | tracker Evidence, `distilled.md` | done | Owner built `cu1343-sm120-dc047a7` (420 s compile, 0 warnings); label = pin; `--help` runs without a GPU; mock-engine serve proves layout, Host check (403 for unknown names) and alias (404). |
| 5 | T5: Check `./run`'s flags against the built image | RULES gate step 5 | done | Uses the image's own `serve.config` (the startup check), not `--help` text; `flags ok (2 engine, 8 server)`, 9 with `API_KEY`; `--no-such-flag` → FAIL. `./run` needed no change. |
| 6 | T6: Owner boots — default and `--single` — with a real reply each | tracker Evidence, `distilled.md` | in-progress | Boot 1: missing `tools/` (fixed). Boot 2: NFS `O_DIRECT` crawl, stopped. **Boot 3 (default, local pack): loaded in 40.1 s, all checks pass, 320.7 t/s.** Left: `./run --single` and a timed `docker stop` — **needs a stop-the-world window the owner schedules**: one card holds one server (`expert_cache_primary_mib 64806` vs `vram_free_mib 24435`, and `--single` shrinks KV slots, not the expert cache), and the owner's live server is on it. `./run` would refuse a busy port either way (`run:96`). |
| 7 | T7: Harvest | `context/*.md`, `README.md` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-04 | `docker buildx imagetools inspect` on both `nvidia/cuda:13.4.3-{devel,runtime}-ubuntu24.04` tags | `sha256:5626a7d0…` / `sha256:f58bd7a0…` — identical to `../ninfer/versions.env` (2026-10-27) | 1 | — |
| 2026-11-04 | throwaway `docker run --rm <runtime digest>`: `command -v python3`; `apt-cache policy python3 python3-venv` | no python3 in base; candidates 3.12.3-0ubuntu2.1 | 1 | — |
| 2026-11-04 | throwaway `docker run --rm <devel digest>`: `apt-cache policy cmake g++ ninja-build`; `nvcc --version` | cmake 3.28.3-1build7, g++ 13.2.0-7ubuntu1, ninja 1.11.1-2; nvcc 13.4.92 | 1 | — |
| 2026-11-04 | `PREFLIGHT_ONLY=1 ./build` (fresh clone) | `vendor/basalt` at `dc047a77…`, both bases resolve, `Check complete, no warnings found.`, `preflight ok`, rc=0 | 2 | identical |
| 2026-11-04 | `touch vendor/basalt/.dirty-probe && PREFLIGHT_ONLY=1 ./build` | `## vendor/basalt is dirty … ?? .dirty-probe`, rc=1; probe file removed after | 1 | — |
| 2026-11-04 | context probe: scratchpad `Probe.Dockerfile` + a copy of `Dockerfile.dockerignore`, context `vendor/basalt`, untagged | `transferring context: 11.75MB`; `/c` lists no `.git`, 13M | 1 | — |
| 2026-11-04 | `DRY_RUN=1 IMAGE_OVERRIDE=x:y ./run` / `--single` | default: `--parallel 4 --slot-full-context`; single: `--parallel 1`, no `--slot-full-context`; pack 131681979197 bytes; `storage/` not created | 1 each | — |
| 2026-11-04 | `./run` refusals: the QUASAR `.ninfer` path, `/nonexistent/x.basalt`, `README.md`, no image, `PORT=99999`, `--single` + `PARALLEL=2` | each rc=1 with its message (`.ninfer` names `../ninfer`; `README.md`: first bytes `# basalt`, want `BASALTMF`) | 1 each | — |
| 2026-11-04 | sleeper container named `basalt` (`--network none`, `sleep 60`), then `DRY_RUN=1 ./run` and `./run` | dry: "a real run would refuse", rc=0; real: `## busy: … basalt …`, rc=1, nothing created | 1 | — |
| 2026-11-04 | project gate steps 1–4 (step 4 from RULES verbatim) | `3 b`, `env ok`, `shellcheck ok`, `preflight ok`, `dry-run ok (image: stub:none)`; `KV=fp16` → `FAIL(dry:default)`/`FAIL(dry:single) missing: --kv int8` | 1 (+1 negative) | — |
| 2026-11-04 | owner `./build` → `captures/build_2026-11-04_dc047a7.log` | `iphands/basalt-blackwell:cu1343-sm120-dc047a7` (`27770926a3f6`, 3,418,642,358 bytes); `basalt: CUDA arch 120`; 507 ninja steps, compile step `DONE 420.2s`; 0 lines matching `warning`; `bin/`: basalt 94.1 MB, basalt-bench, basalt-probe, basalt-vision; pip installed upstream's 8 pins | 1 | apt layers cached from earlier probes |
| 2026-11-04 | `docker image inspect` labels; `docker run --rm --network none` → `bin/basalt --help`, `.venv/bin/python -m serve.server --help`, `ls -A /opt/basalt /opt/basalt/data/profiles`, `id` | `ai.basalt.ref=dc047a77…`; both `--help` rc=0 (no GPU); `.venv bin data serve LICENSE THIRD_PARTY_LICENSES requirements.txt`; `qwen4exp-48x512.prof`; uid 1000 (ubuntu), HOME=/home/ubuntu, Python 3.12.3 | 1 | — |
| 2026-11-04 | mock engine: `docker run -d … -p 127.0.0.1:18700:8700 <img> serve --mock-engine --host 0.0.0.0 --port 8700 --alias cosmo-6000 --allowed-host cosmo.lan --allowed-host cosmo`; curl with `Host:` headers; `docker stop` | start lines `[server] hosts cosmo.lan,cosmo`, `ready http://127.0.0.1:8700/v1`, `warning no api key, open to the network`; `/v1/models` → `cosmo-6000`; Host `cosmo.lan`/`cosmo`/`localhost` → 200, `evil.example` → 403; chat as `cosmo-6000` via `cosmo.lan` → a reply; model `claude-heavy` → 404; `docker stop` 0.66 s | 1 | the start line prints `127.0.0.1` even when bound to `0.0.0.0` |
| 2026-11-04 | gate step 5 (RULES verbatim) on `cu1343-sm120-dc047a7`; `API_KEY=k`; `EXTRA_ARGS=--no-such-flag` | `flags ok (2 engine, 8 server: 142 engine flags known)`; with key: 9 server; negative: `FAIL(flags) --no-such-flag` | 1 each | — |
| 2026-11-04 | owner boot 1: `./run` (default) on `27770926a3f6` | died after `[server] legend`: `serve/engine/modelfile.py:14` `from model_file import ModelFile` → `ModuleNotFoundError: No module named 'model_file'`; nothing loaded | 1 | — |
| 2026-11-04 | gate step 6 draft on `27770926a3f6` | `FAIL(imports) serve.engine.modelfile: ModuleNotFoundError: No module named 'model_file'` (the only failure) | 1 | reproduces boot 1 without a GPU |
| 2026-11-04 | owner rebuild with `tools/` → `63d25a81f328` (same tag); gate steps 1–6 | build stage `CACHED`, pip 5.4 s; `3 b`, `env ok`, `shellcheck ok`, `preflight ok`, `dry-run ok`, `flags ok (2 engine, 8 server)`, `imports ok (32 serve modules)` | 1 | — |
| 2026-11-04 | owner boot 2: `./run` (default) on `63d25a81f328`, start lines | `engine ctx=262144 kv=int8 batch=4 (from cli)`; `sampling temp=1.0 top_p=0.95 top_k=20 min_p=0.0`; `vision mode=dynamic gpu=0 (RTX PRO 6000 …) load=per-image`; `hosts cosmo.lan,cosmo`; `warning: this CPU has no AVX-512: the expert kernels run on AVX-2 …`; then `loading weights into cuda0 … /67.8 GiB  10 MB/s` → `6 MB/s` | 1 | in progress |
| 2026-11-04 | host view of boot 2: `ps -L` on the engine, `/proc/<pid>/io` over 5 s, `vmstat` | 6 threads in `nfs_file_direct_read` (D state), main thread `futex_wait`; rchar +26 MiB / 5 s; host 99–100 % idle | 1 | — |
| 2026-11-04 | `dd` from the pack during boot 2: `iflag=direct bs=16M count=16`, `iflag=direct bs=1M count=64`, buffered `bs=16M count=16` (uncached offsets) | 17.4 MB/s, 4.1 MB/s, 28.9 MB/s | 1 each | taken while the engine's 6 direct readers ran — NFS link vs read pattern not separated |
| 2026-11-04 | boot 2 end: watcher (docker logs once a minute); `storage/cache/basalt/aefd5d98dc650ed4/engine.log`; `dmesg` | last progress 17:41 `7.4/67.8 GiB` into cuda0, container gone by 17:42; engine log stops mid-load with no error line; no OOM or Xid — an outside stop, consistent with the owner switching to a local copy | 1 | — |
| 2026-11-04 | owner's `rsync -avPS noir40.lan:…/Qwen3.8-FN-NVFP4-DQ6_K.basalt` into `models.local/` (ssh, not the NFS mount), size sampled 10 s apart | 310 MB/s | 1 | the server and link deliver; the slow boot was Basalt's `O_DIRECT` reads over the NFS mount |
| 2026-11-04 | local copy `models.local/Qwen3.8-FN-NVFP4-DQ6_K.basalt` (owner rsync), checked | 131,681,979,197 bytes (= NFS original), `BASALTMF` + v2; `filefrag`: 555,406 extents, 464,061 `encoded` — `/home` is btrfs `compress-force=zstd:1`, so `+m` would not have helped | 1 | — |
| 2026-11-04 | owner boot 3: `./run` (default) from `models.local/` on `63d25a81f328` | `loading weights into cuda0 … 2782 MB/s` → `loaded 67.8GiB 40.1s cuda0=67.8GiB`; `auto resident=22.8GB pool-affinity=auto pool=15`; `warning: the device-planned MoE tail is off: every expert is resident`; `kv 1.5/15.2GiB cuda0 elastic slots=4`; `slots par=4 ctx/slot=262144 (slot-full-context)`; `weights loaded, ctx=262144 images=on`; no page-lock or mmap-fallback warning | 1 | — |
| 2026-11-04 | boot 3 engine log (`storage/cache/basalt/aefd5d98dc650ed4/engine.log`) | `expert cache auto: 87.44 GiB free, 700 MiB reserved … -> 24576 slots`; `expert cache 24576 slots, 63.29 GiB of VRAM` (= 48 × 512: every expert in VRAM); `elastic KV: 4096 cells backed per session, the other 13.73 GiB of KV are the expert cache's last 5334 slots until a request needs them`; MTP draft layer 1317 MiB; hc8 copies 680 MiB; image encoder 862 MiB pinned | 1 | — |
| 2026-11-04 | boot 3 requests: `curl http://cosmo.lan:8700/v1/models`; chat `cosmo-6000` "Say hi in five words." `max_tokens 512`; model `claude-heavy`; `Host: evil.example:8700` | 200 (`cosmo-6000`, `n_ctx 262144`, text+image); reply `Hi, how are you today?`, `finish stop`, 196 tokens, 0.75 s wall, server `s1 done pp=58/0c 0.1s tg=196 320.7t/s acc=74% ttft=0.1s end=stop`; 404; 403 + `error: refused Host 'evil.example:8700' from 172.17.0.1` | 1 each | single short reply — not a benchmark |
| 2026-11-04 | `nvidia-smi` with boot 3 idle | GPU 0: 73,091 / 97,887 MiB used, 73 W of a 475 W limit | 1 | — |
| 2026-11-04 | `docker exec basalt` (boot 3): `ulimit -l`, `/proc/1/limits`, engine `/proc/<pid>/status` | `ulimit -l: unlimited`; `Max locked memory unlimited unlimited`; uid 1000 (ubuntu); engine RSS 32,987,960 kB, `VmLck 0`, `VmPin 0` — CUDA-pinned host memory is not counted there, so the evidence for page-locking is the absence of Basalt's fallback warning | 1 | — |
| 2026-11-04 | T1 guards: `DOCKER='sudo docker'`, `DOCKER=/usr/bin/nonexistent-docker` sourcing `lib/common.sh` | both refuse, rc=1, with the fix line | 1 each | — |
| 2026-11-05 | `curl http://cosmo.lan:8700/v1/models` + `/metrics` against the **owner's live server**; `nvidia-smi`; `stat` and `grep -c '^basalt: gpus:'` on `storage/cache/basalt/aefd5d98dc650ed4/engine.log` | 200 `cosmo-6000`, `n_ctx 262144`; geometry `kv int8`, `batch_slots 4`, `expert_slots_primary 24576`, `expert_cache_primary_mib 64806`, `arena_mib 23353`, `vram_free_mib 24435`, version 0.2.1; GPU 0 77,117 / 97,887 MiB used; that log file born **2026-11-05 01:35** holding **3 `--batch 4` boots, no `--single`** | 1 each | **Corroboration, not T6 Evidence**: owner-started server, launch provenance unconfirmed. Its `requests[]` and prompt lines are the owner's traffic and are deliberately not recorded here. |

## Notes / Deviations

- **Revised (owner, 2026-11-04): default pack → `models.local/` (local NVMe, btrfs `compress-force=zstd:1`).**
  Boot 2 was stopped at 7.4/67.8 GiB. `/models.local/` is gitignored.
- **Boot 2 loads at ~5–10 MB/s from NFS** (plan Q9 assumed vLLM-like ~57 s loads). Basalt uploads
  weights with `O_DIRECT` (`include/basalt/core/direct_upload.hpp`), no switch or env var, built for
  local NVMe; but buffered NFS reads were also only 28.9 MB/s during the boot. The owner chose to let
  it keep loading (2026-11-04); options recorded: measure NFS with nothing running, or copy the pack
  to local NVMe (`/home`: 301 GB free).

- **T2 premise wrong** (plan *Why a source-built two-stage image* and T2: "the runtime stage must hold
  `bin/`, `serve/` and `data/`"): `serve/` also needs `tools/` — `serve/engine/modelfile.py` puts
  `<source>/tools` on `sys.path` and imports `model_file`. That T2 item is **invalid as written**;
  the Dockerfile now copies `tools/` too (fixed under T6, where it surfaced). T4's checks and the
  mock engine all passed on the broken image: none of them imports `serve.engine.modelfile`. Gate
  step 6 now imports every `serve.*` module. T4's Evidence rows describe image `27770926a3f6`, which
  the rebuild replaces under the same tag.

- T5 (plan said "appear in `bin/basalt --help` or `serve.server --help`"): the step uses the
  image's own `serve.config.engine_flags_of` + `build_parser()` instead — the exact rule Basalt
  applies at startup (`serve/config.py:25-39`), so it cannot drift from what the server accepts.

- **Environment drift, 2026-11-05.** Two tools the Evidence table used on 2026-11-04 stopped being
  reachable from the agent session: `shellcheck` (re-installed user-local, v0.11.0 at `~/.local/bin`)
  and **docker** — the agent now runs inside a container with the GPU nodes but a docker socket it may
  not open (mode `660`, numeric `0:995`; the session holds groups `1000,27`). Gate steps 3–6 are
  therefore owner-only (*superseded the same evening: they are capability-gated, and the host agent
  session ran 3–6 green — plan § Gate status*); the plan now carries a **Gate status** block saying so, per
  `context/pitfalls.md` § *A gate prerequisite recorded in RULES.md can silently stop being true*.
  Nothing in `run`, `build` or `versions.env` changed to cause this.

- **A false measurement, corrected (2026-11-05).** The session that resumed T6 reported *":8700 free
  (no listener)"* to the owner before drafting the boot block. It was not a measurement: `ss` is not
  installed in the agent container, and `ss … | grep :8700 || echo ":8700 free"` printed the `||`
  branch. `curl http://cosmo.lan:8700/v1/models` → 200: the owner's server was live on the port and
  GPU 0 held 77 GiB. No harm was possible — `./run` refuses a busy port (`run:96`) — but the claim was
  wrong and the drafted boot block would have been run against it. Full entry:
  `context/pitfalls.md` § *Empty output from a failed command was recorded as a measurement*; the
  rule for touching a live server at all is § *A live daily-driver server is not a test bench*.

- **T6's *Confirms if* does not describe this card** (plan T6: "both modes load with experts split
  between `cuda0` and page-locked RAM"). In three default-mode boots the primary expert cache is
  **fully resident** — `expert cache 24576 slots, 63.29 GiB of VRAM` with
  `basalt verify: warning: the device-planned MoE tail is off: every expert is resident`, alongside a
  `resident RAM: 22.81 GiB of experts in RAM, 0 exchanged with the VRAM tier`. "Split" cannot be
  satisfied as written. When the `--single` boot runs, record instead: the `loaded … cuda0=…GiB
  ram=…GiB` line, whether the *same* "every expert is resident" warning appears at `par=1`, any
  page-lock/mmap-fallback warning (that is Q2), and the KV/expert-cache loan lines. The clause is kept
  in the plan as written — this note is the record that reality differs, not an edit to the criterion.

- **Correction (host agent session, 2026-11-04 PST):** the bullet below is wrong. The container session
  read times in UTC: birth `2026-11-05 01:35` UTC is **2026-11-04 17:35 PST**, when boot 2 started; one
  `engine.log` holds boots 2, 3 and later (`grep -c 'expert cache auto'` → 3 on 2026-11-04 18:27 PST),
  and boot 3's lines quoted above are in it. Its other "2026-11-05" dates are the same evening in UTC.
- **Boot 3's `engine.log` is no longer on disk** (checked 2026-11-05): the file at
  `storage/cache/basalt/aefd5d98dc650ed4/engine.log` has birth `2026-11-05 01:35` and contains three
  later boots. The 2026-11-04 rows above are now the only copy of that boot's log lines — `storage/` is
  gitignored, so it was never in git.

- **Boot identity of the memlock row — owner to confirm (agent note, 2026-11-05; the row itself is
  untouched).** The Evidence row `docker exec basalt … ulimit -l` is dated 2026-11-04 and labelled
  "(boot 3)". Two independent measurements say the container answering requests *today* is a
  2026-11-05 boot: (1) that `engine.log`'s btrfs birth is `2026-11-05 01:35`, it holds three
  `basalt: gpus:` markers, and the file grew under observation (231,043 → 232,269 → 255,648 bytes) with
  its mtime tracking live requests second-for-second — and an open fd never migrates to a replacement
  inode, so a log that grows with the answering server *is* that server's file; (2) its own `/metrics`
  still names `r_0022` as its fastest prefill, timestamped `2026-11-05 01:55`, i.e. ~34 requests into
  its life at 01:55 today. **The conclusion does not depend on this**: `--ulimit memlock=-1` is a
  property of how `./run` launches the container, identical across boots, so memlock stays
  `unlimited` whichever boot was exec'd. Only the row's provenance is in question, and the row is the
  owner's measurement — so it is left exactly as he wrote it. To settle it:
  `docker inspect -f '{{.State.StartedAt}} {{.RestartCount}}' basalt`.

- **Correction (host agent session, 2026-11-04 18:3x PST = 2026-11-05 UTC):** `b308244` was committed by the **host agent session** (the one running Plan 01 on `cosmo`, in the `docker` group), not by the owner; `0b1efb6` says so. Both writers were agents; the owner only ran boots. The bullet below, written by the container session, names the owner throughout; read
  "the host agent session" for "the owner" / "his".
- **Two writers, one worktree (2026-11-05).** While the agent was mid-edit, the owner committed from the
  host session: `b308244 [basalt][P1][T6][boot] record memlock unlimited inside the container (plan Q2
  resolved)` carried the agent's then-uncommitted `## Gate status` block (30 lines of
  `01_flashnext_container.md`) and two tracker edits under *his* subject and authorship, and his edit of
  the Q2 bullet collided with the agent's, leaving the bullet self-contradicting on disk (now fixed,
  above). Nothing was amended, reverted or unstashed — Rule B2 forbids it, including `restore` /
  `checkout --` / `stash` / `clean` on work either side did not make; everything here is a forward
  commit. Practical lesson recorded in `context/pitfalls.md`: an agent must not leave knowledge files
  dirty across a window in which the owner is committing, and must name whatever is dirty in its
  handoff.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Tune past the owner's defaults (`basalt-probe` suggestions, `expert-profile-save`, `prefill-cpu-share`, `spec`, `kv` q4_0/k8v4) | Ordering principle: boot before tune | a later plan |
| `sm_120a` build A/B | Basalt accepts `120a` but tests only `120` (plan *Rejected Claims*) | a later plan |
| More than one served model name (vLLM's `claude-*` aliases) | `--alias` takes one name (plan Q6) | later, if a client needs it |
