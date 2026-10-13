# Plan 01 — Run everything as uid/gid 1000:1000, host and container — Tracker

## Overview

- Status: 50% complete (5/10 tasks; T3b and T10 start `blocked`)
- Start date: 2026-10-13 (T1)
- Plan: `context/plans/01_uid_parity.md`
- Evidence: `context/plans/01_uid_parity.md` § Context (the 2026-10-13 probe transcripts), each
  task's Verify transcript pasted into this tracker's Notes, and — from T10 onward — the launcher's
  echoed `docker … serve …` line plus the boot report. **Test environment for every row**: image
  `iphands/vllm-blackwell:cu1321-sm120-main-vllm0.32.0.dev20261013-gdf6c7ad` (Wave 1 — the *pre*-T3
  image, which is deliberate: Wave 1 changes host behaviour only), `VLLM_REF=main` pinned at
  `df6c7ad03194a3051a197d57ca4a94e4a063014e`, host uid/gid `1000:1000`, `CACHE_DIR=/mnt/noir/scratch/ai/vllm/cache`.
  **Corrected at T1**: every probe written during planning ran against `…dev20261010-g187a0eb`, but
  `container/.last-build` now names `…dev20261013-gdf6c7ad` — the owner rebuilt between the plan
  commit and T1. A commit-diff premise refresh cannot see an out-of-repo rebuild, so every Key Fact
  the plan depends on was re-probed on the current image before any edit (memlock, GPU, `site-packages`,
  `/root/.cache`, `Config.User`, `/home/ubuntu/.cache`), and all of them held. Gate step 5's
  `last-build == VLLM_MAIN_SHA` now passes, which it did not before the rebuild.

## Resume Instructions

1. Read `AGENTS.md`, then `context/plans/RULES.md` **in full**, then `01_uid_parity.md`.
2. Environment: docker needs `sudo -n` (socket is `srw-rw---- root printadmin`; this account is in
   no docker group) — but **after T1/T5 the scripts carry their own privilege**, so run
   `./qwen/run` and `./container/build` **as yourself**. `DOCKER=` overrides for a grouped host.
   `CACHE_DIR` default `/mnt/noir/scratch/ai/vllm/cache`; `RUN_UID` default `1000`; no GPU work in
   Wave 1.
3. Ordering: **T1 before T2, and both before any rebuild** — T1 alone is the `uid-0` trap, because
   the launchers still reach bare `docker` and get started with sudo. **T4 before asking the owner
   to rebuild** (2 min, settles the `/root`-hardcoding and mount-hiding questions while the answer
   is still free). T3 then T5–T9 finish Wave 1. T3b and T10 wait on the rebuild.
4. Commit per task as `[P1][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**. T1 additionally must carry the README § Caching rewrite (Rule B.6).
5. Restore state at session end (Rule P2.4): no leftover `uidprobe` image, no `/tmp/p*` dirs, no
   stub `docker` on PATH, launcher env unset.

## Open Unknowns

1. **Is production `$CACHE_DIR` writable by the invoker?** ~~open~~ **ANSWERED 2026-10-13 at T1: yes.**
   `test -w /mnt/noir/scratch/ai/vllm/cache` → writable by uid 1000, so the launcher can create
   `uid-1000` itself. (The risk was stated as "`install -d` fails"; T1 replaced `install -d -m` with
   `mkdir -m` for an unrelated reason, but the precheck guards both.)
2. **Is `/opt/venv/lib/python3.12/site-packages` traversable by uid 1000?** ~~open~~ **ANSWERED
   2026-10-13 at T1: yes** — `test -r …/site-packages/vllm/__init__.py` succeeds as uid 1000 inside
   the `gdf6c7ad` image. Recorded permanently as gate step **7c**, which asserts it on every commit.
3. **Does the image already ship content under `/home/ubuntu/.cache`?** ~~open~~ **ANSWERED 2026-10-13
   at T1: no** — the path does not exist in the image, so the bind mount hides nothing. T4's
   `WARN hidden-by-mount` check is therefore moot for this image, but T4 still runs to prove the
   stacked `USER` directive itself.
4. **Is `$CTX_DIR` root-owned from past sudo builds?** If yes the first unprivileged *real* build
   EACCes on `rsync --delete`, and `PREFLIGHT_ONLY` will not show it (it exits before the rsync).
   *Ask*: T5 Verify, line 4.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Pin identity in `_run-lib.sh` + README in same commit | `common/_run-lib.sh`, `README.md` | **done** | Refused-sudo + no `uid-0` proven; `--user`/`HOME`/mount proven; dir owner proven. Deviations below |
| 2 | T2: Launcher-direct `docker` → `$DOCKER` | `qwen3.8-flash-next/run`, `muse/run`, **`qwen3.8-flash-next/run-legacy`** | **done** — selftest assertions blocked | 7 sites converted + a source-time `${DOCKER[@]} version` probe; zero bare `docker` left in lib/launchers. **`run-legacy` pinned `--user 0:0` AND moved to its own cache root** — a root container writing the shared `uid-1000` tree is invisible to the ownership guard. `muse/run --selftest` **never executed: no Muse checkpoint on this host** (`models/vllm/RedHatAI/` is empty); identity verified by substitute probe only |
| 3 | T3: `USER 1000:1000` in runtime stage | `container/Dockerfile` | **done** | **Metadata only — no `RUN` layer** (base already ships `/home/ubuntu` 0750 `ubuntu:ubuntu`, so `install -d` was a provable no-op). Shipped as the file's **last two instructions** with `ENV HOME` + `ENV CCACHE_DIR` alongside `USER`. New gate 7d PASS; 7b stays a named SKIP until T3b's rebuild |
| 4 | T3b: Image `Config.User` assertion | (built image) | blocked: awaiting owner rebuild | |
| 5 | T4: Prove `USER` on the current image | throwaway `uidprobe` layer | **done** | **CONFIRMS the image-level facts**: identity, `Config.User`, real entrypoint, imports, traversal, HOME, `.pth`, mount target clean, 1 GiB through the mount lands `1000:1000`. **Does NOT confirm the serve path** (no weights, no port bind, no JIT — that is T10). Surfaced `CCACHE_DIR` → T3 became a 3-line metadata edit with **no `RUN`** |
| 6 | T5: `container/build` unprivileged + root guard + legacy-ctx preflight | `container/build`, **`common/_run-lib.sh`** | **done** | Root guard + writability sweep + **socket probe**, all above the first write. `DOCKER_BUILDKIT=1` dropped. Two shipped defects fell out of it: `${DOCKER@a}` and `_run-lib.sh:74`. Legacy tree measured **clean** |
| 7 | T6: `container/archs` honours `DOCKER` | `container/archs` | pending | |
| 8 | T7: Gate step 7, named skip pre-rebuild | `context/plans/RULES.md` | **invalid — folded into T1** | Rule A.5 forces the gate update into the commit that adds the behaviour, and T1's own Verify line demands "gate steps … **and 7**", so T7 could not verify itself as written. Step 7 shipped as 7a/7b/7c in T1's commit; `:226` rewritten there too. **T7's remaining scope is nothing** — re-open only if 7a/7b/7c need rework |
| 9 | T8: Docs + correct stale `sudo` notes + harvest | `AGENTS.md`, `README.md`, `context/*` | pending | |
| 10 | T9: `container/cache-migrate` (opt-in) | `container/cache-migrate` | pending | No sudo needed |
| 11 | T10: Runtime confirmation (gate step 6) | — | blocked: awaiting owner rebuild | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

**Mandatory in this project** (Rule P1): its product is a number, and a number without provenance
gets cited. A claim not in this table has no provenance and cannot be cited by a later plan.

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-13 | `find $CACHE_DIR -maxdepth 4 -printf '%u\n' \| sort \| uniq -c` (pre-work baseline) | 1102 root / 117 iphands | 1 | n/a — an ownership census, not a timing |
| 2026-10-13 | `docker run --user 1000:1000 --gpus device=0 … nvidia-smi -L` | GPU visible to uid 1000 | 1 | Pre-T3 image; the plan's central premise |
| 2026-10-13 | `docker run --user 1000:1000 … ls -ld /root/.cache` | `Permission denied` | 1 | Why the mount target must move |
| 2026-10-13 | `du -sh $CACHE_DIR` | **no result — exceeded 10 min, killed.** Superseded by the `find` census below | 3 attempts | `du` is unusable on this NFS mount; do not re-run it, use `find -printf '%s'` |
| 2026-10-13 | `find $CACHE_DIR -path "$CACHE_DIR/uid-*" -prune -o \( -user root -print \| -type f -printf '%s\n' \)` | **243900 root-owned entries, 205142 regular files, 33.0 GiB** | 1 | Ownership census + byte sum, not a timing. Corrects the baseline row above: `-maxdepth 4`/`-maxdepth 6` reported 1102/13027, i.e. **up to 19× low** |
| 2026-10-13 | `docker run --user 1000:1000 --ulimit memlock=-1 --gpus device=0 -e HOME=/home/ubuntu … 'resource.getrlimit(RLIMIT_MEMLOCK); torch pin_memory 2GiB'` | `soft,hard = (-1, -1)`, `pinned ok: True bytes: 2147483648` | 1 | **Refutes Risk 3 as drafted.** `--user` does not cost the 95.4 GiB PLE pin. Now asserted on every commit by gate step 7c |
| 2026-10-13 | T1 Verify against a stub `docker`, `CACHE_DIR=/tmp/p1cache`, `MODEL_ROOT=/tmp/p1models` | emitted line carries `--user 1000:1000`, `HOME=/home/ubuntu`, `…/uid-1000:/home/ubuntu/.cache:rw`; `stat -c %u` → `1000`; `sudo -n … ./qwen/run` → exit 1, **no `uid-0` created** | 1 | Cold banner printed once, silent on warm boot; `/root/.cache` occurrences in the emitted line: 0 |
| 2026-10-13 | `chmod 700 uid-1000` then boot; and a root-owned `uid-1000` planted in a parent I own | mode **stays 700** across a boot; foreign-owned dir → refused, **0 docker calls reached**, mode/owner untouched | 1 | Proves `mkdir -m` ≠ `install -d -m` (measured: `install -d -m 0750` on a 0700 dir → **750**) |
| 2026-10-13 | Project gate, all steps, on the T1 tree | 1a pass · **1b `27 b` / `24 p`** · 1c `env ok` · **2 `SKIP(shellcheck not installed)`** · 3 `4/4 ok` · 4 `## preflight OK` ×2 · 5 `last-build == df6c7ad…`, `1` image, arch table `_vllm_fa2_C sm_120 x76` no sm_80 SASS · 7a `PASS`×5 · **7b `SKIP`** · 7c `PASS(7c) 1000 unlimited readable` | 1 | Steps 2 and 7b are **named skips, not passes**: shellcheck is absent from this host, and the image predates T3 |
| 2026-10-13 | **T2 branch harness** — `DOCKER=/tmp/t2stub/docker-stub.sh STUB_MODE=<m> ./qwen3.8-flash-next/run <real model>`, 8 modes | `probe_sudo`/`probe_daemon` → full 7-line refusal, **exit 1, not 141**; `hash_good`/`hash_noise` → launch proceeds with overlay mounted; `hash_wrong`+`STUB_GREP=1` → mismatch message carrying a real 64-hex digest; `hash_wrong`+`STUB_GREP=0` → `PLE overlay: not mounted … (vllm#59431)`; `hash_noexist`/`hash_nothing` → the new no-hash branch | 1 each | **`DOCKER` is a space-split string, so a stub script drives every branch with no daemon, GPU or model.** This is what proves the errexit/SIGPIPE fix and the branch ordering; the two real-image runs alone could not |
| 2026-10-13 | Real-image positive control: `docker run --rm --user 1000:1000 --entrypoint sha256sum <img> $PATCH_DST`, then the same file with `--entrypoint grep -qF 'isinstance(quant_config, CompressedTensorsConfig)'` | file hash `ad4de618…` **differs** from `BASE_SHA256 1392f51c…`, yet the grep returns 0 → production takes **branch 1** (`PATCH_PLE=0`, clean boot); neither new branch fires | 1 | The overlay is stale against this snapshot *by design* — vllm#59431 landed at `c32513b`, before `df6c7ad`. Confirms the T2 retraction: `:279` failed loudly but attributed it to image content |
| 2026-10-13 | uid-1000 HOME probe — **substitute** for the unrunnable `muse/run --selftest` | `HF_HOME=/home/ubuntu/.cache/huggingface`; `os.access(W_OK)` → **False**; `makedirs`+`write_text` → **OK** | 1 | The two are not contradictory: `os.access` is False because the leaf did not exist yet, and the write is the real assertion. **Substitute only — `selftest.py` never ran**, because `models/vllm/RedHatAI/` is empty on this host |
| 2026-10-13 | Project gate, all steps, on the T2 tree | Identical to the T1 row: 1a pass · 1b `27 b`/`24 p` · 1c `env ok` · 2 `SKIP` · 3 `4/4 ok` · 4 `preflight OK` ×2 · 5 `last-build == df6c7ad…`, `1` image, `_vllm_fa2_C sm_120 x76`, **0 sm_80 SASS** · 7a `PASS`×5 · 7b `SKIP` · 7c `PASS 1000 unlimited readable` | 1 | Steps 2 and 7b are still named skips. 7a now also exercises the new probe, since the stub answers `version` |
| 2026-10-13 | Project gate, all steps, on the **T3** tree (`container/Dockerfile` + `RULES.md` step 7 edited) | 1a pass · 1b `27 b`/`24 p` · 1c `env ok` · 2 `SKIP` (named) · 3 `4/4 ok` · 4 `preflight OK` ×2 · 5 `last-build == df6c7ad…`, `1` image, `_vllm_fa2_C sm_120 x76` with **0 `sm_80`** · 7a `PASS`×5 · **7b `SKIP`×2** (image predates the edit) · 7c `PASS 1000 unlimited readable` · **7d `PASS`** · **7e `PASS`** | 1 | Two assertions added at this commit (Rule A.5: the gate moves with the behaviour). `_C_stable_libtorch`/`_moe_C_stable_libtorch` do carry `sm_80/89/90` entries — those are the documented Marlin PTX fallbacks, tolerated; the fail condition is `sm_80` **under `_vllm_fa2_C`**, which is 0 |
| 2026-10-13 | **T5 guard matrix**, each path triggered for real: `sudo -n ./container/build`; `BUILD_DIR=/tmp/fakeroot` over a `chown root:root` synthetic tree; `DOCKER=docker` and `DOCKER=/nonexistent/docker`; `sudo -n env CTX_DIR= ./container/build` | root run → `REFUSED: running as root` (`grep -c` → **1**), zero writes; root-owned tree → `REFUSED: the source tree root is not writable by uid 1000 / /tmp/fakeroot/src is owned by uid 0 / one-time fix: sudo -n chown -R 1000:1000 -- /tmp/fakeroot/src`, **exit 1 before any write**; bad `DOCKER` → probe names the transport (`permission denied … unix:///var/run/docker.sock`); `CTX_DIR=` branch prints "no staged copies are made", **no phantom rsync claim** | 1 | Every remedy line was executed, not inspected. The `DOCKER=docker` run exits **141** — that is my own `\| head -5` SIGPIPE-killing the script, the exact trap the probe's comment documents; the script itself exits 1 |
| 2026-10-13 | **`${var@a}` truth table** on bash 5.2, measured because a review suggestion depended on it: exported string / shell string / `declare -a` / `declare -ax` / `declare -Ax` | `x` / *(empty)* / `a` / `ax` / `ax` → **`@a` returns attribute FLAGS, not a type letter** | 1, 6 | **Refutes my own edit and refines the review finding.** `!= ''` (what I wrote) rejects every `DOCKER=docker` arriving from the environment — the only way anyone sets it. `== a` (what T1 shipped) misses an exported array. Correct predicate is `== *[aA]*`, now in both `container/build` and `_run-lib.sh`. Caveat: across an `exec` bash flattens an array to a scalar, so the array branch is reachable only for a same-shell array — the guard is defensive, not an exploit seam. In `context/pitfalls.md` |
| 2026-10-13 | **T5's mandated legacy probe** + the two host facts the reviews rested on inference for: `find $BUILD_DIR/ctx -maxdepth 2 -user root`; `stat` on `build`/`src`/`ctx`/`.last-build`; `sudo -n -l`; `cat /sys/fs/cgroup/vllmbuild.slice/memory.max` | **0 root-owned entries**; all four paths `1000:1000` (`.last-build` 644); `sudo -n -l` → **`(ALL) NOPASSWD: ALL`**; cgroup path → **`No such file or directory`** | 1 | The legacy-root problem **does not exist on this host** — recorded as a non-result, since the plan asks for the number either way, and the sweep is written for the host where it does. `NOPASSWD: ALL` **refutes the review's sudoers inference**: `-n` is fine, so `ALLOW_ROOT=1 sudo -n …` would work here; the printed `sudo -n env ALLOW_ROOT=1 …` is kept because it also works under a docker-only rule. The missing cgroup file means the WARN branch is already the status quo (slice never installed), unchanged by T5 |
| 2026-10-13 | Project gate, all steps, on the **T5** tree (`container/build` + `common/_run-lib.sh`) | 1a pass · 1b `27 b`/`24 p` · 1c `env ok` · 2 `SKIP` · 3 `4/4 ok` · 4 `preflight OK` ×2 · 5 `last-build == df6c7ad…`, `1` image, `_vllm_fa2_C sm_120 x76`, **0 `sm_80`** · 7a `PASS`×5 · 7b `SKIP`×2 · 7c `PASS 1000 unlimited readable` · 7d `PASS` · 7e `PASS` | 1 | Plus the plan's own Verify: `env PREFLIGHT_ONLY=1 VLLM_REF=main ./container/build` unprivileged → `## pre-write checks pass: every target writable, docker reachable via sudo -n docker` → `## preflight OK` → `PREFLIGHT_ONLY=1 — stopping before docker build`. 7a re-run matters: it sources the `_run-lib.sh` guard edited here |
| 2026-10-13 | **T3 as written** — `ENV HOME=/home/ubuntu` + `ENV CCACHE_DIR=/home/ubuntu/.cache/ccache` + `USER 1000:1000`, appended after `ENTRYPOINT`, **no `RUN`** | `awk '/^USER /{u=NR} /^RUN \| ^COPY /{r=NR} END{exit !(u&&u>r)}'` → **PASS**; `Config.User` of the *current* image still empty (correct: the edit is source-only until someone rebuilds) | 1 | The `RUN install -d -m 0750 -o 1000 -g 1000` the plan prescribed is **deliberately absent**: T4 measured the base shipping that exact mode/owner, and `install -d -m` is the idiom T1 removed for chmod'ing existing dirs. Placement after `ENTRYPOINT` is what makes 7d a meaningful text check |
| 2026-10-13 | **T4 probe battery** — `printf 'FROM $IMG\nRUN install -d -m 0750 -o 1000 -g 1000 /home/ubuntu\nUSER 1000:1000\n' \| docker build -q -t uidprobe -` against `…dev20261013-gdf6c7ad` | `--entrypoint id` → `uid=1000(ubuntu) gid=1000(ubuntu) groups=1000,27(sudo)`; `Config.User` → `1000:1000`; `docker run --rm uidprobe --help` (real `vllm serve` entrypoint, baked uid) → **exit 0**; `import vllm, flashinfer, triton, torchaudio` → `imports ok`; `site-packages` traversable; `touch /home/ubuntu/.probe` → writable; `/root/.cache` → `Permission denied` | 1 | Built in seconds, not hours. `docker rmi` afterwards — gate step 5 counts images. **Confirms the image-level facts only**; a boot is still T10 |
| 2026-10-13 | **T4 mount write-through** (the plan's headline metric): nested tree + `k.cubin` written as uid 1000 through `-v …/cache/uidprobe:/home/ubuntu/.cache:rw`, then `find -printf '%u:%g %m'` **on the host** | `1000:1000 750 uidprobe`, `1000:1000 755 vllm`, `1000:1000 755 vllm/deep_gemm`, `1000:1000 644 k.cubin` — every entry uid 1000, modes umask-normal | 1 | The container wrote **through** the bind mount and the host agrees on the owner. The same probe against a **local** path failed `EACCES` — **consistent with** this agent's `/tmp`/`$HOME` living in a namespace dockerd cannot see (evidence: a `--user 0:0` container's write into that mount is invisible from my shell afterwards), which leaves dockerd unable to see the source. That evidence proves the namespace split; "and therefore auto-created it root-owned" is a second inference the `EACCES` cannot distinguish from "the path simply does not exist in dockerd's namespace". Either way the failure is **uninformative about uid 1000** — and the converse caution holds: the **only positive bind-write evidence is this single NFS run**, so do not let "it was an artifact" launder into "mount writes are verified" |
| 2026-10-13 | Serve-time compiler chain as uid 1000: `echo $CCACHE_DIR; command -v cc; cc /tmp/a.c -o /tmp/a` | `CCACHE_DIR=/root/.cache/ccache`, `cc` → **`/usr/lib/ccache/cc`**, `ccache -s` readable, **`cc exit=0`** | 1 | **Half-refutes the review finding.** The live path to an unwritable `/root` dir is real (inherited `Dockerfile:195`, wrapper on PATH from `:187`/`:211`), but ccache degrades instead of failing the compile — so this is **dead cache, not a broken serve**, *on this evidence*. `ENV CCACHE_DIR` redirects it into the mount, so serve-time compiles cache *and* persist |
| 2026-10-13 | Two existence checks that decide T3's shape: base `/home/ubuntu` mode/owner, and base `~/.cache` as **root, unmounted, no payload** | `drwxr-x--- 1 ubuntu ubuntu` **already** in the base image → T3's `RUN install -d -m 0750 -o 1000 -g 1000` is a **provable no-op**; `mount target clean` → nothing is hidden by mounting at `/home/ubuntu/.cache` | 1 | Asked as root with no payload, per the corrected probe rule — the first T4 run asked as uid 1000 *after* importing, and read the import's own `mkdir` as image content. `command -v sudo` → **no sudo binary**, no `/etc/sudoers.d` → group `27(sudo)` is inert, measured |

Record the build every row was taken on: image tag, `ai.vllmcustom.vllm.ref`, and the launcher
env. `bench/bench-results.md` is gitignored — it is not evidence a later plan can cite.

**This plan's headline metric is an ownership census, not a throughput number.** The success
assertion is "zero new root-owned files after a full build and a serve" (T10). Decode throughput
is explicitly **not** measured here, and first-boot cold-cache wall time must never be quoted as a
regression (see T10's "within noise").

## Notes / Deviations

- **The plan draft originally derived the cache directory from `$(id -u)` while pinning
  `--user 1000:1000`.** Both reviewers flagged it as a blocking bug: under `sudo ./qwen/run` — the
  only way launchers work today, since they call bare `docker` — that creates root-owned `uid-0`,
  which the uid-1000 container then cannot write. Rewritten to a single `RUN_UID` constant before
  implementation, and T1's Verify now asserts the sudo run is *refused* and that no `uid-0` appears.
- **The draft also deferred all docs to T8**, which violates Rule B.6 for T1's path change; README
  § Caching moved into T1's commit.
- **T1 shipped five deliberate departures from its own Before/After block**, all reviewer-driven and
  all verified rather than reasoned-about:
  1. **`install -d -m 0755` → `mkdir -m 0750`.** `install -d -m` chmods a directory that *already
     exists* (measured: a `0700` dir becomes `0750`), so the plan's form would have widened a cache
     tree the owner deliberately locked, on every single boot. `mkdir` sets the mode only when it
     creates. The `elif [[ ! -d ]]` is what keeps `mkdir`'s swallowed error diagnosable.
  2. **Ownership asserted on uid alone, not `uid:gid`.** The plan asserted `== 1000:1000`, which
     assumes the invoker's *primary group* is gid 1000. True here; false on a host where it is not,
     where the launcher would refuse a perfectly usable directory and tell you to "run as that uid"
     — advice you cannot follow. Write access comes from the uid.
  3. **The sudo guard moved from `docker_core_args` to file scope.** Every launcher touches the socket
     first (`qwen3.8-flash-next/run` resolves its image by label, `require_image` inspects), so the
     plan's placement was reachable only *after* a docker call had already failed with the misleading
     "image not found" this project's own `pitfalls.md` names.
  4. **The cold banner dropped its `du -sh` instruction and its `cache-migrate` line.** The README
     documents that `du` never returns on this mount, so telling a waiting user to run it is telling
     them to hang their terminal; and `./container/cache-migrate` is T9 and does not exist yet. The
     estimate is now labelled `(unmeasured)` and paired with a liveness signal instead of a bare timer.
  5. **README cites a `find` census, not the `du -sh` figure the plan asked for.** `du` was attempted
     three times and never returned; `find -printf '%s'` completed in under 300 s and answered the
     actual question (33.0 GiB). Rule E.2: the measurement that exists beats the one the plan wanted.
- **Gate step 1b's documented `27 b / 24 p` could not be reproduced — and the snippet, not the tree,
  was wrong.** It classified by shebang only, so it silently skipped 16 shebang-less `.py` modules and
  the three sourced libs that open with `# shellcheck shell=bash`, reporting `24 b / 8 p` against a
  green run. Adding the extension fallback restores `27 b / 24 p` exactly, which proves the numbers
  were right and the snippet had drifted. **Lowering the documented numbers to `24 b / 8 p` would have
  laundered a coverage hole into the baseline** — that was the explicit advice, and it is why T1 fixed
  the classifier instead of the bar.
- **Risk 7 (`bench/`, `scripts/*`) decided: out of T1, into Follow-ups.** `bench/_bench-lib.sh:6,20`
  and `bench/bench-wrapper:85,89` call bare `docker`, so `crash-*.log` on a failed load is already
  empty today and stays empty. Pre-existing, reads logs rather than files, and `Out` names it — but it
  is the same root cause as `$DOCKER`, so it is recorded rather than argued away.
- **A live parser *was* broken by T1 and is fixed in the same commit**: `repin.md`'s stub step grepped
  `^docker `, and the new `sudo -n docker` prefix means the stub is bypassed entirely — the documented
  step would have booted a real 122B model while appearing to suppress the launch. The step now sets
  `DOCKER=docker`, the stub answers `image inspect` itself, and the anchor accepts an optional
  privilege prefix. Verified: launch suppressed, `require_image` satisfied, no socket touched.
- **The draft claimed T3 could not be verified until the multi-hour rebuild.** Wrong — only
  `Config.User` needs the rebuild; T4 proves the rest on the current image in ~2 minutes.
- **`--ulimit memlock=-1` was drafted as a risk.** Reviewed and dismissed: runc applies ulimits as
  root at container init, before the uid drop. Risk 4 removed from the plan; recorded here so the
  reasoning is not lost.

- **T2 shipped six departures from its own Before/After block**, all reviewer-driven and all
  verified rather than reasoned about:
  1. **A `${DOCKER[@]} version` probe at source time, which the plan never asked for.** Every
     downstream docker call is `|| true`, `2>/dev/null` or a `:-<default>`, so a dead socket is
     reported as a missing image — and `qwen3.8-flash-next/run:185` turns that into "rebuild the
     12 GB snapshot". One probe makes all of them honest. Kept despite a per-launch round trip:
     tens of ms against a sweep of eight 122B reloads, and a cached sentinel was explicitly
     rejected because it would hide the mid-sweep failures (dockerd restarted, sudoers revoked)
     that are the probe's whole purpose.
  2. **The probe prints docker's own output, and prints the LAST three lines, via no pipeline.**
     Both are fixes to my own first draft, caught in review. `docker version` emits its 8-line
     Client block *before* it fails on the socket, so `head -3` showed `Client:`/`Version:` and
     hid `Cannot connect …` — the line the message exists to surface. And `sed … <<<"$x" | head -3`
     sits in a live errexit scope (the `if` suppresses errexit for the *condition* only): `head`
     closes the pipe, `sed` takes SIGPIPE, `pipefail` returns 141, errexit kills the script there,
     so the three hint lines never print and the launcher exits **141 instead of 1** — a harness
     reads that as a crash, not a refusal. It fails on *chatty* errors, and a chained sudo refusal
     is already three lines. `tail -3 | sed` has no early-exiting consumer.
  3. **The PLE digest is matched (`[0-9a-f]{64}`), not taken as the first token.** The capture is
     stdout+stderr merged, so `%% *` turns an `ld.so: warning:` into "sha256 ld.so: warning:" in the
     mismatch message — the precise misattribution the block exists to prevent.
  4. **`run-legacy` is pinned to root, not to 1000, and given its own cache root.** The plan says
     pin everything; this image is upstream's, was never booted here, and the model card's numbers
     are root numbers — pinning it would make the A/B a comparison of uids. Root in the container
     then writes into the mounted `uid-1000` cache tree, and the `CACHE_UID` guard cannot see it
     (it checks the leaf, which stays 1000-owned), so the *next* `./qwen/run` would pass the guard
     and die `EACCES` minutes into boot naming nothing. `cache-legacy` is the fix; a warning line
     was rejected as invisible. Cost: the legacy launcher starts cold and is not uid-pinned — both
     deliberate, both in the file's own comment.
  5. **`muse/run --selftest` gets `--user` + the cache mount, with hand-rolled flags.** A selftest
     that runs as root passes on a configuration the server never runs in. Flags are written out
     rather than reusing `CONTAINER_ARGS` because `docker_core_args` adds `-it` whenever `DETACH` is
     unset, and a selftest piped to a log or run from CI has no TTY.
  6. **The no-hash case got its own branch.** The plan's diagnosis (a socket denial skipped the
     check silently) was **wrong and is retracted**: it failed loudly, but blamed image content.
     The real gap was that "could not read it" and "differs" shared one message.
- **The two reviewers split on T2's status, and the split is recorded rather than averaged.**
  neckbeard: leave row 2 `blocked` — the substitute probe never runs `selftest.py`, so the parser
  assertions stay unverified. hoodie: mark it done and annotate — the substitute covers the entire
  failure surface the diff introduces. Taken as **done, with the blocked half named in the cell**:
  T2's own DoD (convert the call sites, no bare `docker`, make `:279` honest) is met and verified,
  and the missing item is a step-6 runtime assertion, which the owner's 2026-10-13 gate decision
  keeps out of the per-commit bar. The dissent stands, and the substitute *did* under-test — it
  skipped the host-side `mkdir`/ownership step the real path performs.
- **There is no Muse checkpoint on this host.** `models/vllm/RedHatAI/` is empty and no `*Glimmer*`
  directory exists under `models`. Nothing in the tree distinguishes "`./muse/run --selftest`
  failed" from "there are no weights to test", so write it down before anyone debugs the selftest.
- **T2's audit boundary, stated so the gap is a decision and not an oversight:** `common/_run-lib.sh`
  and the five `*/run` launchers. `container/build`, `container/push` and `container/archs` still
  call bare `docker` (root-by-design, and gate step 5 documents the `sudo -n` they need), and
  `bench/*` + `scripts/*` are named in the plan's `Out` list.

- **T4 was run with the plan's own Verify block and it produced two confident wrong answers.**
  Both are recorded in `context/pitfalls.md` and the block is corrected in place in the plan file.
  1. `docker run --rm uidprobe id` has no `--entrypoint`, and `ENTRYPOINT` is `["vllm","serve"]`
     (`container/Dockerfile:588`), so `id` was parsed as a **model tag** and vLLM died on a **401
     from huggingface.co**. A bug in the probe was reported as a network/auth fault.
  2. The block lists `~/.cache` **after** the import, in the same shell — so it observed the
     directory that very import created, and the plan's own Expected-observation line reads that
     as "move the mount point". Had I taken it at face value it would have churned T1's shipped
     mount target plus a Rule B.6 README rewrite. Asked properly (as root, unmounted, no payload),
     the mount target is **clean**.
  The shared shape: **the intervention preceded the observation.** For an existence question, ask
  the most privileged identity available with nothing mounted and no payload run.
- **T4 changed T3's shape, twice.** (a) **No `RUN` layer is needed**: the base image already ships
  `/home/ubuntu` as `drwxr-x--- ubuntu ubuntu`, which is exactly what the planned
  `install -d -m 0750 -o 1000 -g 1000` would produce — so that `RUN` is a provable no-op using the
  same `install -d -m` idiom T1 removed from the launcher. T3 is now metadata only. (b) **One
  metadata line was missing and is now in scope**: `CCACHE_DIR=/root/.cache/ccache` is inherited
  from `base` (`Dockerfile:195`) into the runtime stage with `/usr/lib/ccache` **ahead of**
  `/usr/bin` on PATH (`:187`, `:211`), so a uid-1000 `cc` *is* ccache pointed at a directory it
  cannot write. Measured: `cc` still exits 0 — ccache degrades rather than breaking the build — so
  this is **serve-time compilation running uncached forever**, not a break. Redirecting it into the
  mounted tree (`ENV CCACHE_DIR=/home/ubuntu/.cache/ccache`) makes it cache and persist. Both
  review findings were stronger than the evidence warranted in one direction and weaker in the
  other; the probe is what told them apart.
- **`CONFIRMS` is scoped on purpose.** T4 proves the image-level facts and the mount's ownership.
  It does not load weights, bind a port, or trigger a JIT compile. Of the checks it skips, the
  honest split is: **port 8799 > 1024** needs no `CAP_NET_BIND_SERVICE` (inspection, airtight);
  `/opt/venv/bin/vllm` executing at the baked uid is **`[LIVE]`, not inspection** — world-executable
  is necessary but not sufficient (every parent dir needs `x`, the shebang interpreter needs `rx`),
  and what closes it is that `docker run --rm uidprobe --help` exited 0; **memlock was measured**,
  not skipped (row above). What genuinely remains an **assumption about the host, not the image**,
  is `--ipc=host`: it uses the host `/dev/shm`, whose mode is a host property — and a crashed root
  container can leave stale root-owned `psm_*`/`mp-*` segments that uid 1000 cannot unlink (sticky
  bit) while they hold RAM. Check `ls -l /dev/shm` before the first uid-1000 boot (T10). Do not
  quote T4 as a green light for the serve path.
- **The mount-write probe failed on local paths and succeeded on NFS, and the failure is the
  plan's premise, not a defect.** This agent's `/tmp` and `$HOME` live in a namespace dockerd
  cannot see, so dockerd **auto-created the bind source root-owned** and uid 1000 was denied — the
  exact behaviour that produced the 243900-entry tree in README § Caching. The production path is
  NFS and it works: nested dirs + a 1 GiB file land `1000:1000`. Recorded because a future reader
  will otherwise re-run the probe against a local path, see `EACCES`, and conclude T4 refutes.
- **Group `27(sudo)` on the container's uid 1000 is inert — but not for the reason I first wrote.**
  Lead with the unassailable part: **it is not a regression**, the container runs as root today,
  which is strictly more privileged. The reason it is inert is *not* that the group is unreachable
  — `find -perm -4000` finds setuid **`newgrp`**, **`sg`**, **`su`**, `gpasswd`, `passwd`, `chage`,
  `expiry`, `mount`, `umount`, `ssh-agent`, so `newgrp sudo` will happily put uid 1000 into group 27.
  It is inert because group 27 confers **nothing to confer**: `command -v sudo` → absent, `pkexec` →
  absent, and **both `/etc/sudoers` and `/etc/sudoers.d` do not exist**. `su` is setuid-root but
  `passwd -S root` reports **`L`** (locked password), so it cannot authenticate to root either.
  Stated as a set of absences, not as an unusality.
- **T5 departed from its own task row in five ways, and the row was rewritten to match** — most
  of them found by review, all of them verified by triggering the failure rather than reasoning
  about it:
  1. **`common/_run-lib.sh` is in the commit.** The task row named `container/build` only. T5
     duplicates the launcher's `DOCKER` contract, and duplicating it would have duplicated a
     **shipped bug** in T1's `${DOCKER@a} == a` guard (see § Evidence, "`${var@a}` truth table").
     Fixing the contract in one file and not the other leaves two files that disagree about the
     same variable. The row's file list now names both.
  2. **The guards sit above the `PREFLIGHT_ONLY` exit, not above the first write only.** This is a
     real behaviour change: `PREFLIGHT_ONLY=1 ./container/build` now needs a live dockerd, so it is
     no longer the socket-free offline check README § Preflight advertised. That sentence is now
     fixed in the same commit (Rule B.5: the gate moves with behaviour; B.6: docs ride with code),
     and the distinction is recorded there — `./container/preflight` run directly is still offline.
  3. **`DOCKER_BUILDKIT=1` was dropped** from the `docker build` line. It is read client-side, so
     under the default `sudo -n docker` sudo's `env_reset` stripped it anyway: the flag meant two
     different things on the two supported hosts. BuildKit is the default builder on this project's
     dockerd (29.7.2), and every build run here so far already ran with it stripped, so no output
     can change. Accepted residual: on a `DOCKER=docker` host an ambient `DOCKER_BUILDKIT=0` now
     goes unopposed — it fails loudly (`--build-context`/`--secret` are BuildKit-only), not
     silently, so it stays a note rather than a check.
  4. **Printed remedies lost their `-n`** (`sudo chown …`, not `sudo -n chown …`), against the
     convention everywhere else in the project. The convention exists so an in-script sudo can never
     hang. A remedy line is pasted by a human, who should get a password prompt rather than a
     failure on a host with no NOPASSWD rule. Rule adopted: **`-n` for what the script runs, plain
     `sudo` for what it prints.**
  5. **The root guard has an escape hatch the launcher guard does not** (`sudo env ALLOW_ROOT=1`).
     Deliberate asymmetry, documented in both files: a root *launcher* breaks the serve it is about
     to start and has no legitimate use; a root *build* leaves ownership damage that the sweep
     catches at the front door with the fix printed. The hatch silences the check only — the writes
     still land root-owned and the next unprivileged run refuses again.
- **Two of T5's review suggestions were refuted by measurement, not by argument.** The `${DOCKER@a}`
  widening (`!= ''`) would have rejected `DOCKER=docker` from the environment — i.e. every use — and
  the `ALLOW_ROOT=1 sudo …` form the first review called broken **works on this host**, because its
  rule is `(ALL) NOPASSWD: ALL` rather than the docker-only rule the review assumed. Both printed
  claims were rewritten to say what is actually true here and why the chosen form is still the
  portable one. Recorded because both would have shipped as confident falsehoods about sudoers and
  about bash.
- **Testing the guards mutated the working tree twice, and both times the tree was checked
  afterwards rather than assumed clean.** `DOCKER=(…)` in a test array reached `sync_src` before the
  intended failure (SIGPIPE from `| head -2` killed it mid-`git fetch`), and `BUILD_DIR=build` — a
  test meant to prove the relative-path hang — turned out to *legitimately pass* the check (`.` is
  writable) and began cloning pytorch into the repo before `timeout 10` killed it. All four source
  trees verified afterwards at their pinned refs with `dirty=0`; the stray `./build` was removed.
  The lesson is in the Verify block, not the guard: any test that runs `container/build` past its
  pre-write banner is running `sync_src`, which `checkout --force`s trees RULES.md calls
  unrecoverable. That is why every T5 test is now run against either a synthetic `BUILD_DIR` under
  `/tmp` or a `DOCKER=` that fails the probe.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| `container/push` honours `DOCKER` | Out of scope (needs `docker login`, writes no files) but still bare `docker` | Plan 02 or a drive-by |
| `bench/_bench-lib.sh` + `scripts/*` (`docker logs`/`ps`/`rm`) | A failed boot stops printing its crash log when run unprivileged | Plan 02 |
| Reclaim the abandoned root-owned cache (`du -sh` + one command) | Owner chose to leave it; the disk is still theirs to reclaim | Plan 02, owner-decided |
| Persistent `pip install` in the running container | `docker exec` now lands as uid 1000 with an unmounted `~`; today's root flow let it persist in `/opt/venv`. Consider `-e PIP_USER_BASE=/home/ubuntu/.cache/pip-user` | Plan 02 |
| `container/patches/boot-timing/__pycache__/` in `.gitignore` | Container-written bytecode inside the repo tree; regenerates as uid 1000 after T1 | T8, if not already covered |
| `repin.md:197-199` predicts MTP 3 / util 0.96; the launcher emits MTP 4 / 0.95 | Stale expectation found during the repin run, deliberately left alone | Plan 02 |
| The cold-boot banner prints **after** `[[ -w ]]` and `mkdir`, the only two calls in the launcher that can block indefinitely on a hard NFS mount — so the one boot where a stall is likeliest prints nothing | Found at T1 review; the fix is to emit the first banner line before `mkdir`, or wrap it in `timeout` and name the mount | T8 or Plan 02 |
| ~~`require_image` still reports `Image … not found` when the real cause is that `sudo -n` did not answer~~ | **Closed by T2**: a `${DOCKER[@]} version` probe at source time now refuses before any call that could misreport, and prints docker's own last three lines. `context/pitfalls.md` names this lie | done 2026-10-13 |
| ~~`muse/run`'s `--selftest` path runs the container as root while every other path is pinned to 1000~~ | **Closed by T2**: it now runs `--user 1000:1000` with `HOME` and the per-uid cache mount | done 2026-10-13 |
| **Run `./muse/run --selftest` for real** — it has never been executed on this host. Its `--user` change is verified only by a substitute probe (`os.access` False / `makedirs`+write OK), and the probe skipped the host-side `mkdir`+ownership step | No Muse checkpoint on this host: `models/vllm/RedHatAI/` is empty. It is the ritual step after every vLLM bump anyway, so the checkpoint's return closes it naturally | T8 / step 6, on checkpoint refetch |
| `muse/patches/atem-parsers/selftest.py` should end with a deterministic `SELFTEST: PASS` / `SELFTEST: FAIL <reason>` line | Its verdict currently has to be read off a tail behind library warnings (a `HF_HUB_ENABLE_HF_TRANSFER` FutureWarning prints on import — the image sets a deprecated env var). Pass/fail should not depend on what the library logged that day | Plan 02 |
| `run-legacy` now has its own cold cache root, so a legacy A/B re-downloads HF metadata and re-JITs | The price of not letting a root container write into the shared `uid-1000` tree. Copy the useful parts across only if a legacy run becomes routine | Plan 02, owner-decided |
| Measure the boot-time delta of `__pycache__` no longer being writable, and check what a boot writes **outside** the mount | Post-T3, `/opt/venv/.../site-packages` is root:root 0755, so Python silently recompiles the whole import tree every boot. If the delta is real, `ENV PYTHONPYCACHEPREFIX=/home/ubuntu/.cache/pycache` (inside the mount — **never `/tmp`**, fresh per container). Pair it with `find /home/ubuntu -maxdepth 2 -newermt $BOOT_TS` **inside** the container: anything a library puts outside `~/.cache` is on the container layer and is silently re-paid on every boot | T10 |
| `context/plans/01_uid_parity.md:125` and the `boot-timing/__pycache__` Follow-up row both say patch-dir bytecode is written by the container into an **rw**-mounted dir | It is mounted `:ro` (`qwen3.8-flash-next/run:656`), so nothing writes there any more; the 2026-09-25 bytecode predates that mount. Fix the claim or T8 records a false uid-migration story | T8 |
| README wording for "`docker exec` now lands you in as uid 1000" | Lead with the muscle-memory trap: `pip install` now fails `EACCES` on `/opt/venv` where it used to write root-owned files silently — use `docker exec -u root`, or `PIP_USER_BASE=/home/ubuntu/.cache/pip-user` to persist. Also: `HOME` is writable but on the container layer (**ephemeral**); `~/.cache` is the one durable place | T8 (this is that row's wording) |
| ~155 MB of `uv` cache is baked into the runtime image layer | Observed at `/root/.cache/uv` with `CACHEDIR.TAG`. Most likely the base stage's `uv venv`/`uv pip install` (`Dockerfile:212`), which attaches no `--mount=type=cache` — so the cache is committed to a layer instead of cached, and is now dead weight nothing reads. Unverified which RUN writes it | Plan 02, image size |
| `container/archs` must be re-run **post-rebuild**, not just pre | Gate step 5 greps its `sm_120` output, and after T3 that `docker run --rm --entrypoint bash` carries no `--user` — so it executes as uid 1000 for the first time. Read-only (`cuobjdump` on world-readable `.so`s, temp files in `1777 /tmp`), so it should pass; T6's Verify currently only exercises it pre-rebuild | T6 |
| `--ipc=host` puts vLLM's shared-memory segments on the **host's** `/dev/shm`, and prior root boots may have left root-owned `psm_*`/`mp-*` entries | Sticky bit means uid 1000 cannot unlink them, so a crashed root serve leaks RAM that no later uid-1000 serve can reclaim. `ls -l /dev/shm` before the first uid-1000 boot; clear stale entries by hand if any. Also the one place the container writes to the host outside the cache mount | T10 |
| The plan's Goal line says "**Every** process this project starts … runs as uid/gid 1000:1000" while `run-legacy` serves as root by design (`run-legacy:169`, `--user 0:0`) | The behaviour is right and the README says so, but the Goal is overclaimed as written and gate step 7 cannot see it — it scans `uid-1000`, while legacy writes under a different root (`cache-legacy`). Amend the Goal to name the exception at T8; T3 also makes that `--user 0:0` **load-bearing** rather than decorative, so gate 7d/greps assert it stays explicit | T8 |
| `vllm_launch`'s echoed line is a serialization, not a quoting-preserving one: `--override-generation-config '{ … }'` re-splits if pasted | The comment now says so. Needs `printf '%q '` only if the proposed DRY_RUN golden-file test ever lands | Plan 02, with that test |
| The four-tree list is written **four** times (`sync_src` calls, the `--build-context` args, T5's `_tree` loop, T5's `_ctx` loop) | A fifth tree would be built and staged but **silently skip the ownership guard** — the failure is invisible, which is the worst kind for a guard whose whole job is refusing. One `TREES=(…)` array consumed by all four sites. Loop vars `_tree`/`_ctx` also leak as globals and `_ctx` is reused later | Plan 02 or a drive-by |
| Refuse `CTX_DIR` equal to `$SRC_DIR`, `$BUILD_DIR` or an ancestor of either | The staging loop then `rsync --delete`s a tree **onto itself**. Pre-existing, not introduced by T5; T5 removed only the lethal half of it (the printed `rm -rf` no longer names a user-settable root, so a mistyped `CTX_DIR` cannot print a pasteable delete of the build root) | Plan 02, or T8 as a documented caveat if no code lands |
