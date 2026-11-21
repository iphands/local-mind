# Plan 12 — Local patch: a switch that stops Basalt's `O_DIRECT` reads

> **Status**: in-progress
> **Created**: 2026-11-20
> **Depends on**: Plan 10 T1 (the pin `ad274aa` and its image recipe); Plan 11 (`./run-hy4`, `./preload`, whose blocked T3 this plan's boot also settles)
> **Goal**: the image is built from the pinned commit plus one local patch that makes `BASALT_BUFFERED_IO=1` read every file through the OS cache; `./run`, `./run-hy4` and `./preload` turn it on by default; one owner boot of Hy4-flash from the NFS pack reads a long prompt without the watchdog firing.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Carry a three-file patch to Basalt that turns `O_DIRECT` off on request, apply it in our
image build, and have the launchers request it.

**Deliverables**:
1. `patches/0001-buffered-io-switch.patch`; `Dockerfile` applies `patches/*.patch`; `./build` checks
   they fit the pin and labels the image `ai.basalt.patches`.
2. `BUFFERED_IO` (default `1`) in `./run`, `./run-hy4` and `./preload`; gate and `README.md` follow.
3. The owner's rebuild; a gate check that the image for the pin carries the patch.
4. One owner boot of `./run-hy4` after `./preload`, a prompt of the size that stalled, recorded.
5. Facts harvested into `context/`.

**Estimated effort**: Small–Medium (half a day of agent work; one owner build, one boot).

---

## Scope

**In**: `patches/` (new), `Dockerfile`, `build`, `lib/common.sh`, `run`, `run-hy4`, `preload`, the gate
snippets and Expected counts in `context/plans/RULES.md`, `README.md`, plan bookkeeping, `context/`.
**Out**: `versions.env`; `publish` and its tag names (the patched image keeps the pin's tag; the label
says what is in it); a RAM-locked Engram mode for Hy4-flash (option B, tracker Follow-ups); sending
the patch upstream; tuning. `vendor/basalt` is never edited: the patch is applied inside the build.
If a fix needs to cross this line, **stop** and record the blocker in the tracker.

---

## Context

Plan 11 T3 (2026-11-20): Hy4-flash from the NFS pack stalled on a 14,150-token prompt. Cause, measured:
Basalt reads with `O_DIRECT`, which skips cosmo's page cache, so every Engram lookup went to noir, and
noir did not hold the section in RAM. The owner then asked (same evening) for the switch — "I want to
use cosmo page caches" — off by default in the engine, on in our launchers. A Dockerfile change and a
launcher default are each a plan (`RULES.md` § *When Does a Change Need a Plan?*).

### Pre-Identified Bug/Issue

The stall (Plan 11 tracker, Evidence rows 9–10, verbatim there):

```bash
$ docker logs basalt        # the owner's ./run-hy4, NFS pack, 2026-11-20
[server] s1 read  pp=14150 60s
basalt serve: error: no progress for 60 s during a request (reading the prompt, from token 0) - stopping the engine so the server starts it again
  threads waiting on the disk (state D): 256 of 280 (nfs_file_direct_read x256) - the engine is waiting for the drive: a rotational or failing disk with --ple-io direct (--ple-io ram), or a drive too slow for the reads asked of it
```

Today's engine has no way to ask for cached reads:

```bash
$ grep -rn 'O_DIRECT' vendor/basalt/src --include=*.cpp | grep '::open'
vendor/basalt/src/platform/direct_file.cpp:387:    impl_->fd = ::open(path.c_str(), O_RDONLY | O_DIRECT | O_CLOEXEC);
vendor/basalt/src/platform/file.cpp:356:    return ::open(self.c_str(), O_RDONLY | O_DIRECT);
vendor/basalt/src/platform/file.cpp:362:    return ::open(path.c_str(), O_RDONLY | O_DIRECT);
vendor/basalt/src/platform/file.cpp:371:    return ::open(path.c_str(), O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC | O_DIRECT, 0600);
```

### Why an environment switch, patched at build time

- **An environment variable, not a flag.** `BASALT_BUFFERED_IO=1` is read once in the platform layer.
  A flag would have to travel from each model's option parser to two platform files; the server
  already hands its whole environment to the engine (`serve/engine/vision.py:525-526`). Rejected: a
  CLI flag — several times the patch, all of it to re-fit at every repin.
- **Reuse upstream's own fallbacks.** Every caller of `open_unbuffered` already takes a cached path
  when it answers -1 (a filesystem that refuses `O_DIRECT`). The patch makes it answer -1; only
  `DirectFile`, which has no fallback on POSIX, gets a changed `open`. Rejected: new read paths.
- **All reads, not only the table** (the owner's choice, "option A"). The write open at
  `file.cpp:371` is left alone: the conversation cache is on local disk.
- **Applied in the Dockerfile from `patches/`, `vendor/basalt` untouched.** `./build` refuses a dirty
  checkout, and the pin stays the truth about upstream's bytes. Rejected: a fork of the repository
  (a second pin to keep) and editing the checkout (dirty tree, lost on the next checkout).
- **Same image tag, new label.** `./publish` derives every tag from the image's labels and is out of
  scope; `ai.basalt.patches` names each patch and its hash. Rejected: a tag suffix.

### Key Facts

`[SOURCE]` rows are `vendor/basalt` at `ad274aa`.

| Fact | Value | How confirmed |
|---|---|---|
| Read opens with `O_DIRECT` | `DirectFile::open` (POSIX), `open_unbuffered(int)`, `open_unbuffered(path)` | the `grep` above |
| `DirectFile` users | Engram/PLE rows (`src/ngram/ple_reader.cpp:330`), expert reads (`src/core/file_reads.cpp:40`, `expert_source.cpp`, `qwen4exp/decode/expert_dispatch.cpp`), the conversation store | `grep -rln DirectFile src include`, 2026-11-20 |
| `open_unbuffered` callers fall back on -1 | `src/core/loader.cpp:344-354` (keeps a `buffered` fd beside it), `src/core/model_file.cpp:38-40`, `src/core/conversation_store.cpp:112-115`, `src/vision/mmproj.cpp:107-109` | `[SOURCE]`, read 2026-11-20 |
| A POSIX `DirectFile` is a pool of threads doing blocking `pread` | so the same code reads a cached descriptor; nothing else depends on the flag but `bypasses_cache()` | `[SOURCE]` `src/platform/direct_file.cpp:338-419` |
| After a load that went through the cache upstream drops those pages | `advise_dontneed` when `!unbuffered` | `[SOURCE]` `src/core/expert_source.cpp:866` |
| The RAM guard counts page cache as available | reads `MemAvailable` | `[SOURCE]` `src/platform/memory.cpp:283-297` |
| The engine inherits the server's environment | `env = dict(os.environ)` | `[SOURCE]` `serve/engine/vision.py:525-526` |
| The patch | 3 files, 7 hunks; `git apply --check` passes on the pinned tree; both `.cpp` files pass `g++ -std=c++20 -fsyntax-only` (host g++ 16.2.1) | 2026-11-20, tracker Evidence row 1 |
| The patch does what it says | a harness linking the two patched files: switch unset, empty or `0` → `open_unbuffered` gives an `O_DIRECT` fd, `DirectFile`'s fd is `O_DIRECT`, `bypasses_cache()` 1; switch `1` → -1, a cached fd, 0, and one line on stderr | 2026-11-20, tracker Evidence row 1 |
| What cached reads are worth here | 4 KiB reads of the `engram` section: 12.4 ms cold on noir, 0.05 ms once noir has the page; a page in cosmo's own cache needs no round trip | Plan 11 tracker, Evidence row 10 |
| Host RAM | 125 GiB; 95 GiB available with Hy4-flash loaded (experts all in VRAM); the `engram` section is 59 GiB | Plan 11 tracker, Evidence row 8 |

### Rejected Claims

- **"With the switch on, NFS prompts are fast."** Not by itself: a page in nobody's cache is still a
  12 ms seek on noir. The switch lets cosmo's cache hold what was read once; `./preload` is what reads
  it once. Do NOT drop `./preload` from the instructions.
- **"Buffered loads will fill RAM with weights."** Partly handled upstream (`advise_dontneed` above)
  and reclaimable in any case; T4 records what a boot actually shows. Do NOT pre-empt it with another
  patch.

---

## Step-by-Step Tasks

### T1: the patch, the Dockerfile step, `./build`

**File**: `patches/0001-buffered-io-switch.patch` (new), `Dockerfile`, `build`, `README.md`

**What to do**: add the patch (a unified diff with `a/` `b/` prefixes). `Dockerfile`: after `COPY . .`,
apply `patches/*.patch` in name order from a named build context (`--build-context patches=…`) with
`patch -p1 --fuzz=0`, so a patch that no longer fits fails the build; `patch` joins the apt list;
`ARG BASALT_PATCHES` → label `ai.basalt.patches`. `build`: for each patch, `git apply --check`
against the checked-out pin (in the preflight too, so gate step 3 covers it), pass the context and the
label value (`<file>:<sha256, 12 hex>` per patch, or `none`).

**Verify**:
```bash
PREFLIGHT_ONLY=1 ./build 2>&1 | tail -4        # → "== patch fits: 0001-buffered-io-switch.patch", …, preflight ok
# rehearsal of the Dockerfile step in the devel image, on a copy of the three files: patch -p1 --fuzz=0, then the image's g++ -fsyntax-only
# negative control: a patch with one context line changed → the preflight dies naming it
# + the full project gate 1–6 with 5b (steps 5–6 run on the existing, unpatched image)
```

**Commit**: `[basalt][P12][T1][patch] add the buffered-io patch and apply patches/ in the image build`

### T2: `BUFFERED_IO` in the launchers and `./preload`

**File**: `run`, `run-hy4`, `preload`, `lib/common.sh`, `context/plans/RULES.md` (step 4), `README.md`

**What to do**: `BUFFERED_IO` (`1` default, or `0`) becomes `-e BASALT_BUFFERED_IO=<v>` on the docker
command and `io=buffered|direct` on the `== mode:` line. With `1` and an image whose
`ai.basalt.patches` label lacks the patch, the launcher says so in a `== note:` and starts anyway
(the old image ignores the variable). `./preload` follows the same knob: with `1` it reads through
the page cache (cosmo and noir both keep it), with `0` it reads direct as before; `--check` samples
the same way. `run-hy4`'s NFS note is rewritten for both cases.

**Verify**:
```bash
DRY_RUN=1 ./run | grep -c -- '-e BASALT_BUFFERED_IO=1'       # → 1; likewise ./run --single, ./run-hy4
BUFFERED_IO=0 DRY_RUN=1 ./run-hy4 | grep -o 'io=[a-z]*'      # → io=direct
DRY_RUN=1 ./preload                                          # → a dd without iflag=direct
# the serve words of all three dry runs are unchanged (compare before/after)
# + the full project gate 1–6 with 5b
```

**Commit**: `[basalt][P12][T2][run] BUFFERED_IO=1 by default in ./run, ./run-hy4 and ./preload`

### T3: owner rebuild; the gate checks the image carries the patch

**File**: `context/plans/RULES.md` (new step 6b), tracker Evidence

**What to do**: **Owner**: `./build`. Agent: record the image id and labels; add gate step 6b — the
image for the pin has `ai.basalt.patches` naming every file in `patches/` with its hash, and
`bin/basalt` contains the string `BASALT_BUFFERED_IO` — and run the whole gate on the new image.
Not committed before the build: 6b fails on the old image.

**Expected observation**:
- **Confirms if**: the build log shows `== applying 0001-buffered-io-switch.patch` and the compile
  finishes; 6b prints `patches ok (1)`.
- **Refutes if**: `patch` reports fuzz or a reject, the compile fails in a patched file, or the binary
  lacks the string (the patched objects were not linked).

**Commit**: `[basalt][P12][T3][image] record the patched build and gate on its patch label`

### T4: owner boot — Hy4-flash from NFS, a long prompt

**File**: tracker Evidence (this plan's and Plan 11's T3 row)

**What to do**: **Owner**: `./preload && ./run-hy4`, then the kind of request that stalled (a prompt
of 14k tokens or more), and paste the `==` lines, the start block and the request's lines. Agent
reads the engine log for the `BASALT_BUFFERED_IO=1` line and the prompt timings.

**Expected observation** *(written before the boot)*:
- **Confirms if**: the engine log carries `BASALT_BUFFERED_IO=1: file reads go through the OS cache`;
  the prompt is read without a stall report; the Engram host step is a small part of the prompt time.
- **Refutes if**: the watchdog fires again; or the load fails or slows badly through the cache; or
  the RAM guard trips. Then `BUFFERED_IO=0` is the way back, and the default is an owner decision.
- **Within noise if**: load time differs from the 105.8 s / 33.8 s of the direct loads. Record it.

**Commit**: `[basalt][P12][T4][boot] record the owner's buffered-io boot of Hy4-flash`

### T5: Harvest and close

**File**: `context/distilled.md`, `context/pitfalls.md`, `context/high_level.md`, this plan + tracker, `SERIES.md`

**What to do**: `distilled.md`: the switch, what it changes, the measured boot. `high_level.md`: why
we carry a patch, and **Revisit if** upstream grows its own switch or a RAM mode for Hy4-flash.
`pitfalls.md`: anything that took two attempts. The repin checklist gains "re-fit `patches/`".
Then `git mv` to `completed/`, SERIES done.

**Commit**: `[basalt][P12][close] buffered-io patch: <outcome>`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `patches/0001-buffered-io-switch.patch` | new | P0 |
| `Dockerfile`, `build` | apply and label patches | P0 |
| `run`, `run-hy4`, `preload`, `lib/common.sh` | `BUFFERED_IO` | P0 |
| `context/plans/RULES.md` | gate steps 4, 6b | P0 |
| `README.md` | the switch, the build step | P1 |
| `context/` | harvest | P1 |

---

## Open Questions / Risks

1. **Risk: the patch stops fitting at the next repin.** *Mitigation*: `./build` checks every patch
   against the pin before anything is built, and the Dockerfile applies with `--fuzz=0`; the repin
   checklist names it (T5).
2. **Risk: a buffered load of the weights is slower, or crowds the page cache** (86 GiB for
   Hy4-flash, 68 GiB for Flash-Next beside its 50.7 GiB locked table). *Mitigation*: T4 measures the
   Hy4-flash load; `BUFFERED_IO=0` restores upstream's behaviour per boot. Flash-Next under the
   switch is **not** booted in this plan — a Follow-ups row.
3. **Risk: the published image differs from upstream under the pin's tag.** *Mitigation*: the
   `ai.basalt.patches` label and `README.md` say so; `./publish` is the owner's to run.
4. **Risk: unaudited code now includes ours.** The patch is 7 hunks and is in the tree to read;
   Plan 09's audit, when it runs, must read `patches/` too. *Mitigation*: Follow-ups row.
5. **Question: is cosmo's page cache enough, or does the table need locking in RAM?** Cached pages
   can be evicted under pressure. *How we'll settle it*: T4 and use; option B (a `--ple-io ram` for
   Hy4-flash) stays a Follow-ups row.

---

## Verification Checklist

- [x] T1 *(2026-11-20, tracker Evidence rows 2–3)*: `PREFLIGHT_ONLY=1 ./build` prints `== patch fits: 0001-buffered-io-switch.patch` and `preflight ok`; a damaged patch makes it die by name
- [ ] T2: all three launcher dry runs carry `-e BASALT_BUFFERED_IO=1`; `BUFFERED_IO=0` gives `io=direct`; the `serve` words are unchanged
- [ ] T3: gate step 6b prints `patches ok (1)` on the rebuilt image
- [ ] T4: the engine log shows the switch on, and a prompt of 14k tokens or more is read from the NFS pack with no stall report
- [ ] T5: each harvested fact is in the file the tracker names, re-read after writing
- [ ] All: the project gate (Rule A) passes on the final commit
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
