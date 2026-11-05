# Plan 03 — Basalt audit, runtime half: observed egress, exec and input capture

> **Status**: pending
> **Created**: 2026-11-05
> **Depends on**: Plan 01 (the image for the pin). Gates Plan 02's verdict (`02_basalt_vendor_audit.md` § *Re-scope*).
> **Goal**: Watch the pinned Basalt image serve real requests and record every packet it sends, every program it runs and every device or credential file it opens — so "does it phone home or keylog" is answered by observation, not by reading.
> **Agent**: implementation agent (writes and reads); **the owner runs the boot**

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

> **This is a stub written at Plan 02's re-scope (2026-11-05).** A pending plan is a hypothesis (RULES
> lifecycle rule): T1 re-checks every Key Fact below before anything is built, and revises this plan
> with a `Revised` line if one is wrong.

---

## TL;DR

**What**: Boot the pinned image on a Docker network with no route out, drive it with a fixed script of
requests, capture all traffic and the server processes' `connect`/`execve`/`openat` calls, and read the
captures against a stated pass condition.

**Deliverables**:
1. A capture method that works without `sudo` (T1 decides between the options in § *Why*).
2. One owner-run capture session, saved durably (not `/tmp`): a packet capture, syscall traces for both
   server processes, and the request script's own log.
3. `context/audit/runtime.md`: every destination contacted, every program executed, every
   device/credential path opened, each explained or flagged.
4. Static inspection of the shipped binary: `readelf -d`, `nm -D`, `strings`, plus the venv's
   `.pth`/`sitecustomize` hooks — agent-runnable now (`--network none`).

**Estimated effort**: Medium (1–2 days, including the owner's boot).

---

## Scope

**In**: the image `iphands/basalt-blackwell:cu1343-sm120-dc047a7` (Basalt `dc047a7`), as `./run` launches it.
**Out**: reading source (Plan 02); performance; anything needing `sudo` on the host. If capture is
impossible without `sudo`, **stop** and record the blocker rather than widening privilege.

---

## Context

Every static method shares one blind spot: `dlopen`+`dlsym`, raw `syscall()`, and C extensions in the venv
can open sockets that no grep or read of `serve/` sees. Only observed syscalls and packets close it. This
plan was split from Plan 02 when docker was unreachable from agent shells; on 2026-11-05 it became
reachable and an image for the pin exists, so it runs alongside Plan 02's reading.

### Pre-Identified Bug/Issue

N/A — an investigation, not a fix. Today's state, measured 2026-11-05:

```bash
$ docker ps --format '{{.Names}} {{.Image}} {{.Status}}'
basalt iphands/basalt-blackwell:cu1343-sm120-dc047a7 Up 2 hours
$ docker run --rm --network none --entrypoint sh iphands/basalt-blackwell:cu1343-sm120-dc047a7 -c \
    'for t in strace tcpdump ss python3 curl wget; do printf "img:%-8s %s\n" $t "$(command -v $t || echo MISSING)"; done; id; ls /opt/basalt/bin | head'
img:strace   MISSING
img:tcpdump  MISSING
img:ss       MISSING
img:python3  /usr/bin/python3
img:curl     MISSING
img:wget     MISSING
uid=1000(ubuntu) gid=1000(ubuntu) groups=1000(ubuntu),27(sudo)
basalt
basalt-bench
basalt-probe
basalt-vision
```

### Why observation in a no-egress network

A no-route network (`docker network create --internal`) makes a phone-home attempt **visible and
harmless**: the SYN or DNS query is still captured, but it reaches no one. The rejected alternative —
observing on the normal bridge — would let a real exfiltration complete during the test.
**Capture-method options for T1** (no `sudo` anywhere):
(a) a sidecar container sharing the server's network namespace (`--network container:<name>`) with
`--cap-add NET_RAW` and a locally built `tcpdump` image pinned by digest;
(b) host `strace -f -p` on the container's processes, which share the host's uid 1000 — needs T1 to
confirm ptrace is permitted;
(c) a debug image derived from the pinned one with `strace` added — rejected unless (b) fails, because it
changes the thing under test.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| Image for the pin | `iphands/basalt-blackwell:cu1343-sm120-dc047a7` | `image_for_pin` (`lib/common.sh`), 2026-11-05 |
| Docker reachable from an agent shell | yes — group `docker` gid 995, socket `root:docker 660` | `id`; `stat` 2026-11-05 |
| Capture tools in the image | none (`strace`, `tcpdump`, `ss`, `curl`, `wget` missing) | `docker run --network none … command -v`, 2026-11-05 |
| Host tools | `tcpdump`, `strace`, `nsenter`, `readelf`, `nm`, `strings` present; `tshark` missing | `command -v`, 2026-11-05 |
| Container user | `uid=1000(ubuntu)`, same numeric uid as the host user, in group `sudo` (27) — **inert: `sudo` is not installed and there is no `/etc/sudoers.d`** (Plan 02 T13, `wave0/t13-posture.txt`, 2026-11-05); the image has 8 setuid binaries instead, which T2 lists | `id` in the image, 2026-11-05 |
| Yama ptrace scope | not present on this kernel (`/proc/sys/kernel/yama/ptrace_scope` absent) | 2026-11-05 — **whether host `strace -p` works is unconfirmed, confirm in T1** |
| Shipped binaries | `/opt/basalt/bin/`: `basalt`, `basalt-bench`, `basalt-probe`, `basalt-vision` | `ls` in the image, 2026-11-05 |
| GPU and `:8700` | one server at a time; the running `basalt` container must stop for the boot | `CLAUDE.md`, SERIES *Standing Constraints* |

---

## Step-by-Step Tasks

### T1: Prove the capture method on a dummy workload, without the GPU

**What to do**: Create the `--internal` network. Pick capture option (a), (b) or (c) by **trying it** on a
throwaway container that makes one outbound connection and runs one `execve`. Pass: both appear in the
capture. Record the exact commands. No model boot.
**Verify**: the dummy's SYN or DNS query is in the pcap; its `execve` is in the trace.
**Expected observation**: **Confirms if** both are captured with no `sudo`. **Refutes if** neither option
works without privilege → `blocked`, owner decides. **Within noise if** DNS shows only as a query to Docker's
embedded resolver — that still counts as an attempt.
**Commit**: `[basalt][P3][T1][capture] prove the no-sudo capture method on a dummy workload`

### T2: Static inspection of the shipped image (agent-runnable)

**What to do**: Under `--network none`: `readelf -d` (DT_NEEDED, RUNPATH) and `nm -D` on the four
binaries; `strings -a` for URLs, hosts and `/dev/input`; list every `.pth`, `sitecustomize.py`,
`usercustomize.py` in the venv and read each.
**Verify**: one pasted command per check, ending in a sentinel.
**Commit**: `[basalt][P3][T2][static] inspect the shipped binaries and venv hooks`

### T3: The owner-run capture session

**What to do**: The agent writes one command block: stop `basalt`; boot the pinned image as `./run` would,
but on the `--internal` network; attach capture; run a fixed request script (one chat, one image request by
URL, one `/metrics` scrape, idle 10 minutes); save everything under a durable path; restart the normal
server. **The owner runs it** and pastes the summary lines.
**Commit**: `[basalt][P3][T3][capture] record the owner's capture session`

### T4: Read the captures

**What to do**: Tabulate every destination, every `execve`, every `openat` under `/dev`, `/proc/*/fd`,
keyrings or `~/.ssh`-like paths. Explain or flag each in `context/audit/runtime.md`.
**Expected observation**: **Confirms if** the only peers are the test client (and the image URL's host,
attempted, unreachable), `execve`s are the server's own documented children, and no input device or
credential path is opened. **Refutes if** any other destination, unexplained `execve`, or input or
credential open → 🔴 in Plan 02's `findings.md`, escalate to the owner.
**Commit**: `[basalt][P3][T4][audit] read the captures and write runtime.md`

### T5: Hand the result to Plan 02's verdict, harvest, close

**Commit**: `[basalt][P3][close] …` (RULES lifecycle)

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `context/audit/runtime.md` | new: the observed-behaviour table | P0 |
| `context/plans/03_basalt_audit_runtime_tracker.md` | progress and evidence | P0 |

---

## Open Questions / Risks

1. **Question: which capture method works without `sudo`?** *How we'll settle it*: T1, by trying it.
2. **Risk: a phone-home that fires only when egress succeeds** (e.g. after a successful DNS lookup) would
   look like a single failed attempt here. *Mitigation*: any attempt to an unexplained destination is
   flagged on its own; a monitored-egress follow-up is decided only if one appears.
3. **Risk: behaviour that needs time or a trigger** (a timer, a specific request). *Mitigation*: the
   10-minute idle window and the fixed request script; anything longer is a recorded limit, not a pass.
4. **Risk: the boot disturbs the daily driver.** *Mitigation*: the owner runs T3 and restarts the normal server
   in the same block.

---

## Verification Checklist

- [ ] T1: a dummy workload's outbound attempt and `execve` both appear in captures taken without `sudo`
- [ ] T2: DT_NEEDED/`nm -D`/`strings` of all four binaries and every venv `.pth`/`sitecustomize` recorded, each explained
- [ ] T3: the capture session ran on an `--internal` network against the pinned image, and its files are saved durably
- [ ] T4: `runtime.md` lists every destination, `execve` and device/credential open, each explained or flagged
- [ ] All: the project gate passes on the final commit
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
