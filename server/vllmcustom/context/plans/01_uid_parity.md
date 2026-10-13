# Plan 01 — Run everything as uid/gid 1000:1000, host and container

> **Status**: pending
> **Created**: 2026-10-13
> **Depends on**: N/A
> **Goal**: Every process this project starts — container, launcher, build script — runs as uid/gid **1000:1000**, and no filesystem object it creates is owned by anyone else.
>
> **One exception, by design (recorded at T8)**: `./qwen3.8-flash-next/run-legacy` appends `--user 0:0`,
> because it exists to A/B the upstream PR-branch image against ours and pinning the uid there would turn
> an image comparison into a uid comparison. It is contained rather than exempt: it writes to its own
> `cache-legacy` root, so a root serve cannot leave root-owned files inside the tree the next uid-1000
> serve mounts. Gate step 7e sweeps the live tree only, so `cache-legacy` is outside its scope by design.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Make uid/gid `1000:1000` the only identity this project writes with — a `USER` directive in the image, `--user` in the launchers, and a `DOCKER` indirection so every script does its own file work unprivileged and reaches for `sudo` only at the docker socket.

**Deliverables**:
1. `common/_run-lib.sh`: one `RUN_UID=1000` constant feeding both `--user` and the cache path, a root guard, `$DOCKER` for `require_image`/`vllm_launch`, and a per-uid cache mount a uid-1000 process can actually write — plus the README § Caching rewrite **in the same commit** (Rule B.6).
2. The launcher-direct `docker` calls (`qwen3.8-flash-next/run`, `muse/run`) honour `$DOCKER`, so the launchers genuinely run as 1000 instead of being started under sudo.
3. `container/Dockerfile` ends the runtime stage with `USER 1000:1000`; build stages stay root.
4. `container/build` refuses to run as root and honours `DOCKER`; `container/archs` too.
5. Gate **step 7 (identity)** — a named skip until the image is rebuilt, never a silent pass.
6. `container/cache-migrate`: opt-in, no sudo, warms the new cache from the abandoned tree.

**Estimated effort**: Medium (1 day, of which ~0 is waiting on the owner's rebuild if T4 is run first)

---

## Scope

**In**: `common/_run-lib.sh`, the four `*/run` launchers, `container/Dockerfile`, `container/build`, `container/archs`, `AGENTS.md`, `README.md`, the gate in `context/plans/RULES.md`, `context/{pitfalls,distilled}.md`.
**Out**:
- The **existing** root-owned files under `$CACHE_DIR`. The owner chose *leave them, just stop making more* (2026-10-13). No `chown -R`, no deletion, no `rm`. `T9` may **copy** them.
- Daemon config: `daemon.json`, `--userns-remap`, rootless docker, adding the account to the `docker` group. `container/gc-setup` and `container/slice-setup` are `su`-run daemon-config scripts and stay as they are.
- `bench/_bench-lib.sh` and `scripts/*` — they call `docker logs`/`ps`/`rm` and write **no files**, so they are not part of the ownership complaint. Follow-ups row, and T8's README note must name them so the next person is not surprised.
- `container/push` — needs `docker login`, is a rare human op, writes nothing. Follow-ups.

If a task needs to cross a line above, **stop** and record a blocker.

**Completion bound**: `T3b` and `T10` carry status `blocked: awaiting owner rebuild` until the image is rebuilt. **This plan does not move to `completed/` while any task is `blocked`** (Rule C.2), and the SERIES row stays `in-progress (blocked on rebuild)`.

---

## Context

A permission-error investigation on 2026-10-13 traced root-owned files to this project's own containers. The image declares **no `USER`**, so every `docker run` executes as root, and the launchers bind-mount a host directory onto **`/root/.cache` `:rw`** — so the HF hub cache, the FlashInfer JIT caches and vLLM's autotune cache are all written by uid 0 into the owner's tree. Separately, `./container/build` mixes host-side `git` with `docker build` in one script, so running it at all requires `sudo`, which then writes git metadata as root too.

The fix has two halves that only work together: **pin the container identity**, and **stop needing `sudo` for anything that touches files**. Half the fix without the other produces a worse bug — a sudo-started launcher writing a root-owned cache directory that a uid-1000 container then cannot write into. That trap is the reason `T1` and `T2` are one wave.

README § *Caching* and § *Layout* already describe the mount layout this changes; `context/pitfalls.md` already covers "`sudo docker` masquerading as a missing image".

### Pre-Identified Bug/Issue

**(a) The cache tree is owned by root, not by the account that inspects it.**

```bash
$ find /mnt/noir/scratch/ai/vllm/cache -maxdepth 4 -printf '%u\n' | sort | uniq -c | sort -rn
    1102 root
     117 iphands
$ ls -l /mnt/noir/scratch/ai/vllm/cache/vllm
drwxr-xr-x 1 root root ... flashinfer_autotune_cache
drwxr-xr-x 1 root root ... deep_gemm
```

**(b) Containers run as root because the image never says otherwise.**

```bash
$ sudo -n docker run --rm --entrypoint bash $IMG -c id
uid=0(root) gid=0(root) groups=0(root),27(sudo)
```

**(c) The blocker for the obvious fix** — `/root/.cache` is unreadable to uid 1000, so `--user` alone breaks every launcher:

```bash
$ sudo -n docker run --rm --user 1000:1000 --entrypoint bash $IMG -c 'ls -ld /root/.cache'
ls: cannot access '/root/.cache': Permission denied
```

**(d) Today's documented full-build command does not work as the invoker**, which is why the launchers get started with sudo and the caches end up root-owned:

```bash
$ VLLM_REF=main ./container/build          # container/build:259 runs bare `docker build`
# socket is srw-rw---- root printadmin, and this account is in no docker group
```

### Why [Approach]

**Chosen**: identity asserted in two independent places — `USER` in the image (so even a hand-run `docker run` or a `docker exec` is non-root) and `--user` in the launcher (so the guarantee survives an older image or a hand-run container) — with **one constant** (`RUN_UID=1000`) feeding both the flag and the mount path, and the mount target moved to a **per-uid subdirectory**.

**Rejected — runtime `--user` only**: no rebuild, fully reversible, but the guarantee is only as wide as our own scripts, and the owner asked for the container itself to be pinned.
**Rejected — `uid-$(id -u)` for the directory name**: reviewed and rejected. The flag is pinned to 1000; deriving the path from the *invoker* lets the two drift, and under the only invocation that works today (`sudo ./qwen/run`) it creates root-owned `uid-0` and then fails with an `EACCES` that names neither cause.
**Rejected — `--userns-remap` / rootless docker**: a daemon-wide change that rewrites GPU access and every existing cache path, for a problem one `USER` line solves.
**Rejected — chown the existing cache tree**: a `sudo chown -R` over 1100 files outside git, which the owner declined. The per-uid subdirectory gets the same effect without touching the old tree; `T9` offers the copy for anyone who wants the caches warm.
**Rejected — adding the account to the `docker` group**: removes `sudo` from the docker calls but grants root-equivalent privilege, and does nothing about the file writes, which is the actual complaint.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| Image has a uid-1000 account | `ubuntu:x:1000:1000:Ubuntu:/home/ubuntu:/bin/bash` | `getent passwd 1000` in the `g187a0eb` image, 2026-10-13 |
| Image declares no `USER` → runs as root | no `USER` line in the Dockerfile; `uid=0(root)` at runtime | grep + `docker run --entrypoint bash $IMG -c id`, 2026-10-13 |
| uid 1000 sees the GPU | `GPU 0: NVIDIA RTX PRO 6000 Blackwell` | `docker run --user 1000:1000 --gpus device=0 … nvidia-smi -L`, 2026-10-13 |
| `/home/ubuntu` writable by 1000 | `drwxr-x--- ubuntu ubuntu` | `docker run --user 1000:1000 … ls -ld /home/ubuntu`, 2026-10-13 |
| venv entrypoint executable by 1000 | `-rwxr-xr-x root root /opt/venv/bin/vllm` | same run — **the file, not the directory** |
| `/root/.cache` unusable as 1000 | `Permission denied` | `docker run --user 1000:1000 … ls -ld /root/.cache`, 2026-10-13 |
| Root is **not** squashed on this NFS export | `sudo -n touch` there yields `-rw-r--r-- root root` | probe created + removed 2026-10-13 |
| Cache tree ownership split | 1102 root / 117 iphands (depth 4) | `find $CACHE_DIR -maxdepth 4 -printf '%u\n' \| uniq -c`, 2026-10-13 |
| Cache mount is the only rw bind mount in the shared path | `-v "$CACHE_DIR":/root/.cache:rw` | `common/_run-lib.sh:85` |
| `CACHE_DIR` default, 4 launchers | `/mnt/noir/scratch/ai/vllm/cache` | `qwen/run:63`, `muse/run:267`, `laguna/run:52`, `qwen3.8-flash-next/run:257` |
| Build script's privileged touchpoints | `git fetch/checkout/submodule` 142-144; `docker build` 259; `PREFLIGHT_ONLY` exits 175-177, **before** the ctx rsync | `container/build` |
| `preflight` needs no docker | comment only, no invocation | `container/preflight:7` |
| Bare `docker` fails for this account | socket `srw-rw---- root printadmin`, 0 docker group memberships | `ls -l /var/run/docker.sock`, `groups`, 2026-10-13 |
| Overwriting a file preserves owner; creating one does not | why `$BUILD_DIR/src/.git` looked clean while the cache did not | `find $V/.git -newermt … -printf '%u'` → all iphands, 2026-10-13 |
| **Unconfirmed**: production `$CACHE_DIR` parent is invoker-writable | `install -d` fails every launch if it is not | **confirm in T1** — `test -w /mnt/noir/scratch/ai/vllm/cache` |
| **Unconfirmed**: `site-packages` is *traversable* by 1000 | today everything runs as root, so a 0700 dir would go unnoticed | **confirm in T4** — `docker run --user 1000:1000 … test -r …/vllm/__init__.py` |
| **Unconfirmed**: nothing lives under `/home/ubuntu/.cache` in the image | the mount would silently hide it | **confirm in T4** |

### Rejected Claims

- **"`$BUILD_DIR/src` was littered with root files."** It was not: `find` over all four `.git` trees for `-user root -o -nouser -o -nogroup` returned **0**. Root overwrote files that already existed, and an overwrite keeps the owner. Do not go "fixing" those trees.
- **The NFS export squashes root to your uid.** It does not — the probe created a `root root` file. The earlier opposite claim came from reading overwritten files, not new ones.
- **`container/patches/boot-timing/__pycache__/` is this plan's mess.** It is dated **2026-09-25** and untracked. Note for T8, though: it is the container writing bytecode into the rw-mounted patch dir, so after this change it regenerates as uid **1000** instead of root — if it is not gitignored, T8 adds it there rather than deleting it.
- **"Risk: pinned memory needs privilege."** Reviewed and dismissed: `--ulimit memlock=-1` is applied by runc **as root at container init, before the uid drop**, so `--user` cannot affect it. Likewise `--ipc=host` (host `/dev/shm` is 1777) and the `.pth` overlays (dockerd mounts as root; the process only reads).

---

## Step-by-Step Tasks

**Wave 1 — host-side, no rebuild, no GPU: T1 → T2 → T4 → T3 → T5 → T6 → T7 → T8 → T9.**
**Wave 2 — after the owner rebuilds: T3b → T10.** Wave 1 exits when every launcher echoes `--user 1000:1000` and `PREFLIGHT_ONLY=1 ./container/build` passes without sudo.

### T1: Pin the identity in `common/_run-lib.sh` (and the README, same commit)

**File**: `common/_run-lib.sh` (`require_image` :54, `docker_core_args` :61-92, `vllm_launch` :186) + `README.md` § *Caching*

**What to do**: Four edits in one file.
1. One constant, used by both the flag and the path — never `id -u`: `RUN_UID=${RUN_UID:-1000}`, `DOCKER=${DOCKER:-sudo -n docker}`.
2. A root guard in `docker_core_args` that refuses a sudo'd launcher **by name**, naming the `uid-0` trap it is preventing.
3. `install -d` the per-uid cache dir **as the invoker before the mount**, printing a first-boot banner only when it did not already exist, and fail loudly if `$CACHE_DIR` itself is not writable.
4. `require_image` and `vllm_launch` call `$DOCKER`.

**Before**:
```bash
    --entrypoint vllm
    --gpus device=0                                # RTX PRO 6000 only — never the 4060
    -e CUDA_DEVICE_ORDER=PCI_BUS_ID
    -p "0.0.0.0:${PORT}:8799"
    -v "$MODEL_FOLDER":/models:ro
    -v "$CACHE_DIR":/root/.cache:rw
    --ipc=host
```

**After**:
```bash
    --entrypoint vllm
    # Same identity as RUN_UID below. The image ends with `USER 1000:1000` too; the
    # flag is the belt, so an older image or a hand-run container cannot regress us.
    --user "$RUN_UID:$RUN_UID"
    -e HOME=/home/ubuntu                           # set, not inferred from passwd
    --gpus device=0                                # RTX PRO 6000 only — never the 4060
    -e CUDA_DEVICE_ORDER=PCI_BUS_ID
    -p "0.0.0.0:${PORT}:8799"
    -v "$MODEL_FOLDER":/models:ro
    # Per-uid cache root. /root/.cache is 0700 and unreadable to uid 1000, and the
    # pre-split tree above it is root-owned and stays that way by choice.
    -v "$CACHE_HOST_DIR":/home/ubuntu/.cache:rw
    --ipc=host
```
and above the array:
```bash
  CACHE_HOST_DIR="$CACHE_DIR/uid-$RUN_UID"     # RUN_UID, never `id -u` — see the guard
  local _cache_new=0; [[ -e $CACHE_HOST_DIR ]] || _cache_new=1
  [[ -w $CACHE_DIR ]] || {
    echo "## $CACHE_DIR is not writable by uid $(id -u); containers cannot create $CACHE_HOST_DIR" >&2
    return 1
  }
  install -d -m 0755 "$CACHE_HOST_DIR" || { echo "## cannot create writable cache dir $CACHE_HOST_DIR" >&2; return 1; }
  (( _cache_new )) && {
    echo "## cache: $CACHE_HOST_DIR  (NEW — first boot as uid $RUN_UID)"
    echo "##        re-downloads HF metadata and re-JITs FlashInfer/DeepGEMM: expected, not a regression."
    echo "##        the old root-owned tree at $CACHE_DIR is abandoned, not deleted."
    echo "##        warm it deliberately:  SRC=$CACHE_DIR DST=$CACHE_HOST_DIR ./container/cache-migrate"
  }
```

**And in the same commit** (Rule B.6 — this is the plan's one user-visible path change), rewrite README § *Caching*'s runtime-cache bullet into a two-path block: the live path (`$CACHE_DIR/uid-1000`), the abandoned one (`$CACHE_DIR`, with its real `du -sh` figure), the one command to warm the new one, and the one command to reclaim the disk **if the owner ever decides to**.

**Verify**:
```bash
test -w /mnt/noir/scratch/ai/vllm/cache && echo "prod CACHE_DIR writable"    # production parent, not /tmp
install -d /tmp/p1cache && PATH=/tmp/p1stub:$PATH CACHE_DIR=/tmp/p1cache ./qwen/run 2>&1 \
  | grep -E '^([^ ]+ )*docker ' | grep -o -- '--user 1000:1000\|/home/ubuntu/.cache\|HOME=/home/ubuntu'
stat -c '%u:%g' /tmp/p1cache/uid-1000                       # → 1000:1000
sudo -n env CACHE_DIR=/tmp/p1cache PATH=/tmp/p1stub:$PATH ./qwen/run 2>&1 | grep -c 'run as yourself'   # → 1
ls -d /tmp/p1cache/uid-0 2>&1 | grep -c 'No such file'      # → 1  (the trap is shut)
rm -rf /tmp/p1cache
bash -n common/_run-lib.sh && grep -c 'uid-$(id -u)' common/_run-lib.sh   # → 0
```
Expected: three flags present, dir is `1000:1000`, the sudo run is **refused** and creates no `uid-0`. Then gate steps 1a/1b/1c/3/4 and 7.
**Note for the commit body**: `vllm_launch`'s echo is the refactor test's diff target (`_run-lib.sh:182-183`); the line now begins with the `$DOCKER` expansion, so any `grep '^docker '` elsewhere must become `grep -E '^([^ ]+ )*docker '`.

**Commit**: `[P1][T1][run-lib] pin uid 1000 with a per-uid cache mount, refuse sudo`

### T2: Launcher-direct `docker` calls honour `$DOCKER`

**Files**: `qwen3.8-flash-next/run` (:185, :261, :279, :286, :291), `muse/run` (:346)

**What to do**: These bypass `docker_core_args`, so T1 does not reach them. Replace each bare `docker` with `$DOCKER`. **:279 is the urgent one**: it is `$(docker run … || true)`, so today a socket denial degrades into the PLE-overlay hash check being **silently skipped** — the launcher proceeds without verifying the shim it depends on. Make that one fail loudly instead of `|| true`.

**Verify**:
```bash
bash -n qwen3.8-flash-next/run muse/run
grep -n '\bdocker \b' qwen3.8-flash-next/run muse/run | grep -v '\$DOCKER' | grep -v '^\s*#'   # → empty
PATH=/tmp/p1stub:$PATH ./qwen3.8-flash-next/run 2>&1 | grep -ci 'permission denied\|not found'  # → 0
```
Expected: no bare `docker` left in either launcher; the overlay check reports a verdict rather than silently skipping. Then gate.

**Commit**: `[P1][T2][launchers] route launcher-direct docker calls through DOCKER`

### T3: `USER 1000:1000` at the end of the runtime stage

**File**: `container/Dockerfile` (`FROM base AS vllm-openai` :472; last `RUN` :527, then ENV/ARG/LABEL/EXPOSE/ENTRYPOINT)

**What to do**: Build stages stay root — BuildKit owns them and `CCACHE_DIR=/root/.cache/ccache` plus every `--mount=type=cache,target=/root/…` assumes it. Confirmed by review: between the last `RUN` and `ENTRYPOINT` there is only metadata (ENV/ARG/LABEL/EXPOSE), nothing needing uid 0. Add, after the final `LABEL`:
```dockerfile
# Run as uid/gid 1000 — the same id as the account owning the host tree this container
# mounts. Everything above stays root (BuildKit cache mounts and the venv live under
# /root and /opt); this changes only the default for `docker run` and `docker exec`.
# /opt/venv is world-readable and /opt/venv/bin/vllm world-executable. T7 must still
# observe: site-packages dir modes, and any library hardcoding /root.
# `-m 0750` preserves the base image's mode; plain `install -d` would widen it to 0755.
RUN install -d -m 0750 -o 1000 -g 1000 /home/ubuntu
USER 1000:1000
```
Placement costs one tiny layer and invalidates nothing below it — BuildKit invalidation flows forward only, and both install `RUN`s sit above it in the stage.

**Verify**:
```bash
grep -cnE '^\s*USER 1000:1000' container/Dockerfile                                   # → exactly 1
awk '/^FROM base AS vllm-openai/{f=1} f&&/^USER/{print "line "NR; exit}' container/Dockerfile   # → a line > 472
```
Expected: one `USER`, in the runtime stage. **Status `done`; the image-level assertion is T3b.**

**Commit**: `[P1][T3][dockerfile] default the runtime stage to uid/gid 1000:1000`

### T4: Prove `USER` on the image we already have

**File**: none — a throwaway layer stacked on the current image

**What to do**: T3's rebuild is multi-hour; this costs ~2 minutes and settles the two risks that actually matter (a library hardcoding `/root`; the `.pth` overlay), plus the unconfirmed Key Facts. Stack the directive on the existing image and run the real import path.

**Verify**:
```bash
IMG=$(set -a; . container/versions.env; . container/.last-build; set +a; echo "$IMAGE:$VTAG")
# (1) Existence FIRST, asked as the image's own (root) identity, with no mount and no payload.
#     If root sees nothing, uid 1000 certainly sees nothing, and nothing you ran made it.
sudo -n docker run --rm --entrypoint sh "$IMG" -c \
  'test -e /home/ubuntu/.cache && { echo "MOUNT HIDES CONTENT:"; ls -A /home/ubuntu/.cache; } || echo "mount target clean"'
# (2) Does the base already ship the home dir with T3's bits? If it prints
#     `drwxr-x--- ubuntu ubuntu`, the RUN below is a no-op — delete it (see T3).
sudo -n docker run --rm --entrypoint ls "$IMG" -ld /home/ubuntu
printf 'FROM %s\nRUN install -d -m 0750 -o 1000 -g 1000 /home/ubuntu\nUSER 1000:1000\n' "$IMG" \
  | sudo -n docker build -q -t uidprobe -
# (3) --entrypoint IS MANDATORY. ENTRYPOINT is ["vllm","serve"] (container/Dockerfile:588),
#     so a bare `docker run uidprobe id` parses `id` as the MODEL TAG and dies with a 401
#     from huggingface.co — a probe bug reporting a network bug. [OBSERVED 2026-10-13]
sudo -n docker run --rm --entrypoint id uidprobe                             # → uid=1000(ubuntu)
sudo -n docker image inspect -f '{{.Config.User}}' uidprobe                  # → 1000:1000 (T3b rehearsal)
sudo -n docker run --rm uidprobe --help                                      # real entrypoint, no --entrypoint
sudo -n docker run --rm -e HOME=/home/ubuntu --entrypoint bash uidprobe -c '
  python -c "import vllm, flashinfer, triton, torchaudio; print(\"imports ok\")"
  test -r /opt/venv/lib/python3.12/site-packages/vllm/__init__.py && echo "site-packages traversable"
  touch /home/ubuntu/.probe && echo "HOME writable (container layer — EPHEMERAL)"
  echo "CCACHE_DIR=$CCACHE_DIR"; command -v cc; printf "int main(){}\n" >/tmp/a.c && cc /tmp/a.c -o /tmp/a; echo "cc exit=$?"
  ls -ld /root/.cache 2>&1'
# (4) The headline metric: nested dirs + 1 GiB written THROUGH the real bind mount, then
#     verified by OWNER ON THE HOST. Mount source must be created by the same namespace that
#     dockerd sees — the production cache tree is on NFS; a source dockerd cannot see gets
#     auto-created root-owned and uid 1000 is denied, which is the very failure this plan is
#     about (measured that way by accident on 2026-10-13).
install -d -m 0750 /mnt/noir/scratch/ai/vllm/cache/uidprobe
sudo -n docker run --rm --user 1000:1000 -e HOME=/home/ubuntu \
  -v /mnt/noir/scratch/ai/vllm/cache/uidprobe:/home/ubuntu/.cache:rw \
  --entrypoint bash uidprobe -c 'mkdir -p /home/ubuntu/.cache/vllm/deep_gemm && touch /home/ubuntu/.cache/vllm/deep_gemm/k.cubin && echo "mount write ok"'
find /mnt/noir/scratch/ai/vllm/cache/uidprobe -printf '%u:%g %m %p\n' | sort -k3   # want: all 1000:1000
rm -rf /mnt/noir/scratch/ai/vllm/cache/uidprobe && sudo -n docker rmi uidprobe    # rmi: gate step 5 counts images
```
**Expected observation**:
- **Confirms if**: `mount target clean`, `imports ok`, `site-packages traversable`, `HOME writable`, `cc exit=0`, and every entry written through the mount owned by `1000:1000`.
- **Refutes if**: an import raises `EACCES`, or `site-packages` is unreadable → the plan needs `PYTHONPYCACHEPREFIX=/home/ubuntu/.cache/pycache` (inside the mount — **not `/tmp`**, which is fresh per container and so would make the loss permanent) or a group-readable mode on that tree, and T3's comment must say which.
- **Within noise if**: nothing here is timed; import latency is not a signal.
- A `MOUNT HIDES CONTENT` line means the image ships content at the mount target and the mount point must move (e.g. `/home/ubuntu/vllm-cache`) — decide before the rebuild, not after.
- *Corrected 2026-10-13 at T4.* The block previously ran `docker run --rm uidprobe id` (no `--entrypoint`, so the probe argument was eaten by `vllm serve`) and listed `~/.cache` **after** the import in the same shell, so it observed the directory that very import created — while the "Expected observation" section wired that false positive to "move the mount point". Both are the same bug: the intervention preceded the observation. See `context/pitfalls.md`.

**Commit**: `[P1][T4][verify] prove uid 1000 serve path on the current image`

### T5: Run `container/build` unprivileged except at the docker socket

**File**: `container/build` (knobs ~33, `sync_src` 134-151, ctx rsync ~187-193, `docker build` 259)

**What to do**: `DOCKER=(sudo -n docker)` as an **array** (word-splitting an unquoted `$DOCKER` is invisible here because gate step 2, shellcheck, is skipped) expanded `"${DOCKER[@]}"`, used for docker subcommands only. Guard above the `source versions.env` line so a root run touches nothing:

```bash
if [[ $(id -u) == 0 && ${ALLOW_ROOT:-0} != 1 ]]; then
  cat >&2 <<EOF
## REFUSED: running as root.
##   This script git-fetches and `checkout --force`s four hand-editable trees under
##   $BUILD_DIR/src and rsyncs into $CTX_DIR; as root those writes land root-owned.
##   It needs sudo only for the docker CLI, and it calls sudo itself.
##     run it as you:      ./container/build
##     grouped host:       DOCKER=docker ./container/build
##     really mean root:   ALLOW_ROOT=1 sudo -n ./container/build
EOF
  exit 1
fi
```
`ALLOW_ROOT=1` keeps the guardrail from becoming a dead end on a host nobody has seen.
**Then the legacy-root problem** (review finding, and the reason `PREFLIGHT_ONLY` is not sufficient proof): past full builds ran under sudo, so `$CTX_DIR/*` is plausibly root-owned, and the first unprivileged *real* build will `EACCES` on `rsync -a --delete` into it — while a `PREFLIGHT_ONLY` run exits at :177 **before** the rsync and reports all clear. Add a writability check per staged tree that dies with the one-time remedy, and probe `container/.last-build` ownership (a root-owned file there fails the next build's write).

**Verify**:
```bash
bash -n container/build
env PREFLIGHT_ONLY=1 VLLM_REF=main ./container/build                    # NO sudo → "## preflight OK"
sudo -n ./container/build 2>&1 | grep -c 'REFUSED: running as root'      # → 1  (assert the text, not just exit 1)
find /mnt/noir/scratch/ai/vllm/build/ctx -maxdepth 2 -user root 2>/dev/null | head -3   # legacy check, feeds the die-msg
ls -l container/.last-build
```
Expected: unprivileged preflight passes, the sudo run refuses **by name**, and the legacy probe's result is recorded in the tracker whether or not it is empty. Then gate.

**Commit**: `[P1][T5][build] run host steps as the invoker, refuse root, guard legacy ctx`

### T6: `container/archs` honours `DOCKER`

**File**: `container/archs`

**What to do**: Same array. This is the command gate step 5 runs, and today it only works with a hand-written `sudo -n` prefix. (`container/push` is **out of scope** — see Scope; it goes to Follow-ups.) Post-T3 `archs` runs as uid 1000 inside the image; it only `cuobjdump`s, which is read-only.

**Verify**:
```bash
bash -n container/archs
set -a; . container/versions.env; . container/.last-build; set +a
DOCKER='sudo -n docker' ./container/archs "$IMAGE:$VTAG" | tail -3      # runs as invoker, no socket error
```
Expected: architecture output, no permission error. Then gate.

**Commit**: `[P1][T6][archs] honour DOCKER so it runs as the invoker`

### T7: Gate step 7 — assert the identity, as a **named skip**

**File**: `context/plans/RULES.md` (§ Project gate)

**What to do**: Rule A.5. Three assertions, all model-free, and each one **prints its own verdict line** so a shell cannot read green from an empty string. The image assertion must render `SKIP (image predates the T3 rebuild)` while `Config.User` is empty — a gate step that fails every commit is not a gate; mirror step 2's precedent exactly, and record the new baseline in the Baseline block.

```bash
u=$(sudo -n docker image inspect -f '{{.Config.User}}' "$IMAGE:$VTAG" 2>/dev/null)
case $u in
  1000:1000) echo "PASS step7: image User=$u" ;;
  "")        echo "SKIP step7: image User empty (predates the T3 rebuild)" ;;
  *)         echo "FAIL step7: image User='$u'" ; exit 1 ;;
esac
find "$CACHE_DIR/uid-1000" -user root | head -3          # unbounded: -maxdepth 2 misses hub/<repo>/<rev>
find "$CACHE_DIR" -maxdepth 1 -newermt "$BOOT_TS" -user root | head -3
```
Rewrite — do **not** delete — the sentence at `RULES.md:226`: direct `docker images` / `docker ps` lines in the gate still need `sudo -n`; what changes is that the **scripts** now carry their own privilege. Record the baseline.

**Verify**: `bash -n` the new block; run step 7 against the current (pre-rebuild) image and confirm it prints `SKIP`, and that a deliberately wrong `--user` in a stub makes it print `FAIL`. Then gate.

**Commit**: `[P1][T7][gate] add step 7 asserting uid 1000, named skip pre-rebuild`

### T8: Document the contract, correct the two stale docs, harvest

**Files**: `AGENTS.md`, `README.md` (§ *Layout*, the docker bullet), `context/pitfalls.md`, `context/distilled.md`

**What to do**: `AGENTS.md`'s "Docker needs `sudo -n`" bullet is now half-true and misleads people into sudo-ing the *scripts* — the exact thing T1/T5 refuse; say the scripts carry their own `sudo -n docker` and `DOCKER=` overrides. Same correction for the `Docker needs sudo -n docker` bullet in `SERIES.md` § Standing Constraints (`:59`), which lists `require_image` failing as plain user — after T1/T2 that is the design, not a defect. `pitfalls.md` gets three Problem/Avoid entries tagged `[OBSERVED 2026-10-13]`: the `/root/.cache` mount that made `--user` look unworkable; "a root overwrite preserves the owner, so an ownership audit must look for *new* files, not recent ones"; and "`sudo ./qwen/run` writes `uid-0`" with the guard that now catches it. `distilled.md` gets the confirmed facts, `[LIVE 2026-10-13]`. Also: gitignore `container/patches/boot-timing/__pycache__/` rather than deleting it — it is bytecode the container writes into the rw-mounted patch dir, and after this change it regenerates as uid 1000. Note the `docker exec` regression (a `pip install` in-container now lands in an unmounted `~` and evaporates on `--rm`) and that `bench/`+`scripts/*` still call bare docker.

**Verify**: `grep -c 'OBSERVED 2026-10-13' context/pitfalls.md` → ≥3; `grep -n 'uid-1000' README.md` → ≥1; `grep -n 'DOCKER' AGENTS.md` → ≥1; re-read each file after writing (Rule D.4).

**Commit**: `[P1][T8][docs] record the uid-1000 contract and correct the stale sudo notes`

### T9: `container/cache-migrate` — opt-in, and it needs no sudo

**File**: `container/cache-migrate` (new)

**What to do**: The abandoned tree is `drwxr-xr-x root root` — **world-readable**, so uid 1000 can copy it itself; no `sudo`, no `chown`, fully reversible (`rm -rf` the target). Print sizes **first**, require a typed yes, then `rsync -a --ignore-existing` per known cache dir (`huggingface`, `hub`, `vllm`, `flashinfer`, `deep_gemm`, `torch_extensions`). **Do not** offer `cp -al` (hardlinks preserve root ownership, buying nothing) and do not symlink the JIT dirs — FlashInfer and DeepGEMM write lockfiles beside their artifacts, and an unlinked symlink is a worse failure than a cold JIT; say both in the header so nobody "optimises" it later.

**Verify**:
```bash
SRC=/mnt/noir/scratch/ai/vllm/cache DST=/tmp/p9cache ./container/cache-migrate --dry-run | head -12  # sizes, no writes
install -d /tmp/p9cache && SRC=/mnt/noir/scratch/ai/vllm/cache DST=/tmp/p9cache ./container/cache-migrate <<< y >/dev/null
find /tmp/p9cache -user root | wc -l        # → 0  (copies are ours)
find /mnt/noir/scratch/ai/vllm/cache -maxdepth 1 -newermt "$(date +%T)" -user root | wc -l   # → 0 (source untouched)
rm -rf /tmp/p9cache
```
Expected: zero root-owned files in the copy, source mtime unchanged. Then gate.

**Commit**: `[P1][T9][cache-migrate] opt-in no-sudo copy of the abandoned root cache`

### T10: Runtime confirmation (gate step 6 — named by this task) — `blocked` until the rebuild

**File**: none — a run, then a read-back

**What to do**: After the owner rebuilds (`VLLM_REF=main ./container/build`, run **as the invoker**), boot the daily driver once and read it back: no `EACCES`/`Permission denied` in the boot log, headroom block present, no `[draft-load-filter] NOT applied`, host-RAM line showing the PLE table pinned, and zero new root-owned files. Capture `$BOOT_TS` **before** the boot and use it in T7's `-newermt` — a `$(date +%F)` midnight comparison passes vacuously. Restore state afterwards (Rule P2.4).

**Expected observation**:
- **Confirms if**: "startup complete" as uid 1000, `find $CACHE_DIR/uid-1000 -user root` empty, and nothing newer than `$BOOT_TS` under `$CACHE_DIR` owned by root.
- **Refutes if**: `EACCES` during weight load, KV profiling or FlashInfer JIT — the culprit will name itself, and it means a library ignores `HOME`; the fix is a targeted `-e VLLM_CACHE_ROOT=`/`XDG_CACHE_HOME=`, not a revert.
- **Within noise if**: first-boot wall time. A cold per-uid cache re-downloads metadata and re-JITs sm120f kernels; expect a long JIT stretch on the first boot of each family. **This is not a throughput measurement** — do not report it against Plan-01 numbers, and do not compare it to the warm baseline.

**Commit**: `[P1][T10][verify] confirm a non-root serve writes only uid-1000 files`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `common/_run-lib.sh` | `RUN_UID`, root guard, `$DOCKER`, per-uid mount, first-boot banner | P0 |
| `qwen3.8-flash-next/run`, `muse/run` | `$DOCKER`; :279 must stop swallowing failure | P0 |
| `container/Dockerfile` | `install -d -m 0750` + `USER 1000:1000`, runtime stage only | P0 |
| `container/build` | root guard, `DOCKER` array, legacy-`ctx` writability preflight | P0 |
| `context/plans/RULES.md` | gate step 7 as a nameable assertion; rewrite `:226` | P1 |
| `context/plans/SERIES.md` | the `:59` standing constraint about bare `docker` failing | P1 |
| `container/archs` | honour `$DOCKER` | P1 |
| `AGENTS.md`, `README.md`, `context/pitfalls.md`, `context/distilled.md` | the contract + the three traps | P1 |
| `container/cache-migrate` | new, opt-in | P2 |
| `container/push`, `bench/_bench-lib.sh`, `scripts/*` | **no edit** — Follow-ups | P2 |

---

## Open Questions / Risks

1. **First non-root boot re-downloads metadata and re-JITs.** Weights are *not* re-fetched (`$MODEL_FOLDER:/models:ro`), so the cost is HF metadata plus FlashInfer sm120f / DeepGEMM / torch.compile warm-up — order **20–60 min per family**, to be replaced with the measured figure in T8. *Mitigation*: T9's opt-in copy, and the first-boot banner in T1 that says so at the moment it is relevant rather than in a README nobody re-reads.
2. **Leaving 1102 root-owned files is the owner's choice and it has teeth.** Those directories stay unwritable to uid 1000 forever, so anything that reaches around the new mount — `docker exec --user root`, a hand-run container, a reverted launcher — meets `EACCES` with no obvious cause. *Mitigation*: after T1 the old tree is mounted **nowhere**, so a container cannot corrupt it, only fail loudly; T8 documents it by name and T7's second assertion fails if anything writes into it again.
3. **Risk: a library hardcodes `/root`.** *How we'll settle it*: **T4, today, in two minutes** — not after a multi-hour rebuild. If it fires, the fix is a targeted env override.
4. **Risk: the mount hides image content at `/home/ubuntu/.cache`.** *How we'll settle it*: T4's `WARN hidden-by-mount` line; if it prints, move the mount point before T3 is rebuilt.
5. **Question: does production `$CACHE_DIR` allow the launcher to create `uid-1000`?** T1's guard turns it into a loud failure, but the answer is still unknown until T1 runs against the real path. *How we'll settle it*: T1's first Verify line.
6. **Question: should `--user` follow the invoker instead of pinning 1000?** *How we'll settle it*: deferred — pinning is the point of the plan, and the host account already is 1000. Revisit only if a second operator with a different uid appears.
7. **Question: is `bench/` and `scripts/*` bare-`docker` breakage acceptable?** They read logs, not files, so they are outside the ownership complaint, but a failed boot will stop printing its crash log. *How we'll settle it*: owner's call at T8 review; logged as a Follow-up either way.

---

## Verification Checklist

- [ ] T1: production `test -w` passes; stubbed `./qwen/run` echoes `--user 1000:1000` / `HOME=/home/ubuntu` / `/home/ubuntu/.cache`; `stat` → `1000:1000`; **the sudo'd run is refused and no `uid-0` exists**; zero `uid-$(id -u)` in the file; README § Caching in the **same commit**
- [ ] T2: no bare `docker` in either launcher; the :279 overlay check yields a verdict, not a silent skip
- [ ] T3: exactly one `USER 1000:1000`, after line 472
- [ ] T4: `uid=1000(ubuntu)`, `imports ok`, `site-packages traversable`, `HOME writable`, no `WARN hidden-by-mount` — **or** the refutation recorded and the mount point changed
- [ ] T5: `PREFLIGHT_ONLY=1 VLLM_REF=main ./container/build` passes without sudo; `sudo -n ./container/build` prints `REFUSED: running as root`; legacy `ctx` probe recorded
- [ ] T6: `./container/archs "$IMAGE:$VTAG"` runs as the invoker with no socket error
- [ ] T7: step 7 prints `SKIP` on the current image and `FAIL` on a wrong `--user`; `RULES.md:226` rewritten not deleted
- [ ] T8: `grep -c 'OBSERVED 2026-10-13' context/pitfalls.md` ≥ 3; `AGENTS.md` and `README.md` re-read after writing
- [ ] T9: dry-run prints sizes and writes nothing; the copy contains 0 root-owned files; source tree untouched
- [ ] T10 `blocked`: boot reaches "startup complete" as uid 1000, no `EACCES`, no `hooks NOT applied`, 0 new root-owned files, against `$BOOT_TS`
- [ ] T3b `blocked`: `docker image inspect -f '{{.Config.User}}'` → `1000:1000` on the rebuilt image
- [ ] All: gate (Rule A) on the final commit, every step reported — step 2 `skip` (shellcheck absent) and step 7 `SKIP` (pre-rebuild) both **named**, neither counted green
- [ ] All: findings harvested (Rule D); plan moves to `completed/` **only after** T3b and T10 leave `blocked` (Rule C.2)
