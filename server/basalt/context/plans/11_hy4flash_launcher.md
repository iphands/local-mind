# Plan 11 — Hy4-flash launcher: `./run-hy4`

> **Status**: in-progress
> **Created**: 2026-11-20
> **Revised**: 2026-11-20 — T5 added at the owner's request after T3's stall: `./preload`, which reads a pack's `engram` section so the NFS server caches it. Scope gains `preload` (new).
> **Depends on**: Plan 10 T1 (the v0.5.0 image, `cu1343-sm120-ad274aa`: the first pin that builds `hy4flash`); Plan 01 (`./run`, `lib/common.sh`, gate steps 4–6)
> **Goal**: `./run-hy4` serves a Hy4-flash `.basalt` pack on `:8700` with flags that model accepts, `./run` and `./run-hy4` each refuse the other's pack by name, the gate checks a launcher's flags against the pack's own adapter, and one owner boot with a real reply is recorded.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Add a second launcher for the fourth model Basalt v0.5.0 builds, because `./run` passes
Flash-Next's flags and Hy4-flash refuses them.

**Deliverables**:
1. `./run-hy4` (new) and `pack_arch` in `lib/common.sh`; gate steps 1b, 4, 5 cover it and a new
   step **5b** checks each launcher's engine words against its pack's adapter; `README.md`.
2. `./run` refuses a Hy4-flash pack with a pointer to `./run-hy4`.
3. One owner boot of `./run-hy4` with a real `/v1/chat/completions` reply, recorded.
4. Facts harvested into `context/`.

**Estimated effort**: Small (2 h of agent work; one owner boot).

---

## Scope

**In**: `run-hy4` (new), `lib/common.sh` (one function), `run` (one refusal, no flag or default
changes), the gate snippets and Expected counts in `context/plans/RULES.md`, `README.md`, `AGENTS.md`,
plan bookkeeping, `context/` harvest.
**Out**: any default of `./run`; tuning Hy4-flash (expert tier, `--spec`, KV format — the defaults
here are starting values, see *Open Questions*); copying the pack off NFS (the owner's step);
`versions.env`, `Dockerfile`, `build`, `publish`; Plans 08 and 10's open rows; the audit of the built
pin (Plan 09). If a fix needs to cross this line, **stop** and record the blocker in the tracker.

---

## Context

The owner downloaded `Hy4-flash-NVFP4.basalt`, ran it through `./run` (2026-11-20), was refused, and
asked for a `./run-hy4`. A new launcher profile is a plan (`RULES.md` § *When Does a Change Need a
Plan?*). `SERIES.md`'s north star names Flash-Next only; this plan widens it at the owner's request.

### Pre-Identified Bug/Issue

The owner's run, verbatim (2026-11-20):

```bash
$ ./run ./models/vllm/jesdga/Hy4-flash-NVFP4.basalt
== image: iphands/basalt-blackwell:cu1343-sm120
== pack:  /mnt/noir/scratch/ai/llm/models/vllm/jesdga/Hy4-flash-NVFP4.basalt (158746168309 bytes, 147 GiB)
== mode:  3 slots × 362144 (slot-full-context), kv=int8, ple-io=ram, vision=dynamic, http://0.0.0.0:8700/v1 as 'cosmo-6000'
== cache: /home/iphands/prog/ai/local-mind/server/basalt/storage/cache
docker run --rm --name basalt --user 1000:1000 -e HOME=/home/ubuntu --gpus device=0 -p 0.0.0.0:8700:8700 --ulimit memlock=-1 -v /mnt/noir/scratch/ai/llm/models/vllm/jesdga:/models:ro -v /home/iphands/prog/ai/local-mind/server/basalt/storage/cache:/home/ubuntu/.cache:rw iphands/basalt-blackwell:cu1343-sm120 serve /models/Hy4-flash-NVFP4.basalt --host 0.0.0.0 --port 8700 --alias cosmo-6000 --allowed-host cosmo.lan --allowed-host cosmo --gpu 0 --max-context 362144 --kv int8 --ple-io ram --parallel 3 --slot-full-context --vision dynamic
[server] legend   pp=prompt read  tg=generation  c=cached  acc=drafts accepted  dl=tokens per window  hit=expert cache  pcie=routed experts over PCIe
[server] error: /models/Hy4-flash-NVFP4.basalt: unknown argument: --ple-io
```

The same refusal, and the next one behind it, from the engine's no-load flag check in the built
image (agent, 2026-11-20; `$img` = `iphands/basalt-blackwell:cu1343-sm120-ad274aa`, `$d` = the pack's
directory; `usage` lines dropped):

```bash
$ docker run --rm --network none --user 1000:1000 -e HOME=/tmp -v "$d:/models:ro" --entrypoint /opt/basalt/bin/basalt "$img" --model /models/Hy4-flash-NVFP4.basalt --max-context 362144 --kv int8 --ple-io ram --gpu-plan
basalt: error: unknown argument: --ple-io
$ … --model /models/Hy4-flash-NVFP4.basalt --max-context 362144 --kv int8 --gpu-plan
basalt hy4flash: error: --kv int8: the latent caches are fp16 (the default) or fp8
$ … --model /models/Hy4-flash-NVFP4.basalt --max-context 362144 --kv fp16 --gpu-plan
{"arch":"hy4flash","model":"Hy4-flash","quality_flags":"","quality_window":"1","lossy":"","spec":6,"max_context":362144,"devices":"0","cards":[{"visible":0,"token":"0","smi":0,"name":"RTX PRO 6000 Blackwell Workstation Edition"}],"notes":[],"warning":"","error":""}
```

Gate step 5 passed on this image with `--ple-io` in the flag set (Plan 10 tracker, Evidence): it
checks a name against every flag in the binary, whichever model owns it. That gap is why step 5b is
part of T1.

### Why a second launcher, and why `run` is barely touched

- **`./run-hy4` beside `./run`** (the owner's request). The two models share the container shape and
  the server options, and differ in every engine flag `./run` passes and in the defaults that make
  sense (slots, KV). Rejected: one model-aware `./run` — every default would branch on the pack, and
  `./run`'s boots are still owed to Plans 08 and 10; a launcher that changes under them muddies both.
- **Duplicate the launcher body, share only `pack_arch`.** `run-hy4` repeats `run`'s docker and
  printing code (about 60 lines). Rejected for now: moving that body into `lib/common.sh` — it rewrites
  the Flash-Next launcher while its boots are unrecorded. A Follow-ups row carries the refactor.
- **Tell packs apart by `general.architecture` in the pack's `meta` section**, the key the server
  itself reads (`vendor/basalt/serve/engine/modelfile.py:110`). Rejected: the file name (a renamed
  pack would pass) and section names (`engram`, `dfly` — a coincidence of this converter).

### Key Facts

`[SOURCE]` and `[DOC]` rows are `vendor/basalt` at the pin `ad274aa` (checked out there; `git -C
vendor/basalt log --oneline -1` → `ad274aa8`, 2026-11-20).

| Fact | Value | How confirmed |
|---|---|---|
| The pack | `/mnt/noir/scratch/ai/llm/models/vllm/jesdga/Hy4-flash-NVFP4.basalt`, 158746168309 bytes, format version 2, 14 sections; the last (`meta`) ends at the file's size, so the download is whole by its own table | `od`/`dd` over the 64 KiB header, 2026-11-20 (layout: `vendor/basalt/tools/model_file.py:21-29`); sizes: `experts` 83576037376, `engram` 63360615936, `gguf` 8003244544, `dfly` 1691383474, `vision/mmproj` 1044397760, `dense` 1039115904 |
| Architecture key | Hy4 pack `"general.architecture":"hy4flash"`; Flash-Next pack `"general.architecture":"qwen4exp"`; each once in its `meta` section, written without a space after the colon | `dd` of each pack's `meta` section piped to `grep -a -o`, 2026-11-20. The adapter also answers to `hy_v4_flash` (`src/models/hy4flash/adapter.cpp:91-95`) |
| Flags are per model | an unknown flag is an error; the adapter parses what the common set does not | `[SOURCE]` `src/program/cli/arguments.cpp:125-133` |
| `--ple-io` | Flash-Next's only | `[SOURCE]` `src/models/qwen4exp/flags.cpp:29`; absent from `src/models/hy4flash/adapter.cpp:56-66` and `dfly_meta.cpp:158-168`, whose flags are `--diag-rows --kv --spec --spec-oracle --diag-teacher --diag-tier-frozen --draft-log` |
| `--kv` for Hy4-flash | `fp16` (default) or `fp8`; anything else is refused when flags finish | `[SOURCE]` `adapter.cpp:68-75`; the probe above |
| Context | 1024 … the pack's `context_length`, 1048576; outside is refused at load, **not** by the no-load check (`--max-context 2000000 --gpu-plan` exits 0) | `[SOURCE]` `src/models/hy4flash/context_policy.hpp` (`plan_context`); pack `meta` `"context_length":1048576`; probe 2026-11-20 |
| KV cost | 14.6 KB a position with fp16 rows, 8.6 KB with fp8: about 5.3 GB / 3.1 GB at 362144; the first 32768 positions are backed at start, the rest come out of the expert tier | `[DOC]` `docs/models/hy4-flash/kv.md`, `docs/models/hy4-flash.md` § *1M context* — upstream's figures, not measured here |
| Engram table | 63.4 GB, always read from the pack row by row through `ngram::PleReader` (4 KiB direct reads, up to 256 in flight, a 65536-row cache); 16 rows a token in each of 4 layers. No flag selects a RAM mode | `[SOURCE]` `src/models/hy4flash/model.cpp:195-200`, `include/basalt/ngram/ple_reader.hpp:1-15`; `[DOC]` `docs/models/hy4-flash.md` (model table, risk 6: "64 random 96-byte rows per token") |
| Parallel slots | upstream's model table gives Hy4-flash `—`; the engine has a path for a model that does not batch (one slot's window at a time) and the no-load check accepts `--batch 3` | `[DOC]` `docs/models/index.md` row *Parallel slots*; `[SOURCE]` `src/program/serve/session_engine.cpp` (`next_batch_window`); probe 2026-11-20. **Not booted.** |
| Images | `--vision dynamic` only (or `off`); no video | `[DOC]` `docs/models/hy4-flash/serve.md` § *Images, sampling, context*; `docs/changelog/0.5.0.md` |
| RAM, upstream's design figures | "RAM holds every expert (NVFP4 77.9 GiB)"; measured on one RTX 5090 (32 GiB), not on a 96 GiB card | `[DOC]` `docs/models/hy4-flash.md` § *Memory* — estimates by its own marking |
| The server's start check | `basalt --model <pack> <engine words> --gpu-plan`: parses by the model's rules, loads nothing; about 1.3 s in the image | `[SOURCE]` `serve/engine/vision.py:47-66` (`model_plan`), called at `serve/server.py:160`; timed 2026-11-20 |
| `--parallel 1` | adds no `--batch` (only `parallel > 1` does) | `[SOURCE]` `serve/config.py:268-272`; probe: engine words `--max-context 362144 --kv fp16` |
| Host | RAM 125 GiB, 98 GiB available; `/home` 299 GB free (a local copy is 148 GiB); GPU 0 had 8730 MiB in use | `free -g`, `df -h models.local`, `nvidia-smi`, 2026-11-20 20:50 PST |
| Gate baseline | 1–6 pass: `PRODUCT_B=4 AUDIT_B=5`, `flags ok (3 engine, 8 server: 158 engine flags known)`, `imports ok (42 serve modules)` | tracker Evidence row 1 |
| Agent shell can run gate 3–6 | `docker` is in `id -nG` | `id`, 2026-11-20 |

### Rejected Claims

- **"`PLE_IO=ram` should carry over."** There is nothing to carry it to: Hy4-flash's reader takes no
  mode. Do NOT add a `--ple-io` to `run-hy4`, by `EXTRA_ARGS` or otherwise — it is the refusal above.
- **"Gate step 5 would have caught this."** It did not and cannot: it matches names against the whole
  binary. Step 5b is the per-pack check.

---

## Step-by-Step Tasks

### T1: `./run-hy4`, `pack_arch`, and the gate that covers them

**File**: `run-hy4` (new), `lib/common.sh`, `context/plans/RULES.md` (Expected counts, steps 1b, 4, 5, 5b), `README.md`, `AGENTS.md`

**What to do**:
1. `lib/common.sh`: `pack_arch <pack>` prints `general.architecture` from the pack's `meta` section,
   or nothing. It walks the header's section table (`od`, `dd`) and never fails its caller; no
   pipeline in it may end early on its reader (`pipefail` would turn the `SIGPIPE` into a failure).
2. `run-hy4`: `run`'s shape — same checks, container, cache mount, `DRY_RUN`, `IMAGE_OVERRIDE`,
   `EXTRA_ARGS`, masked `--api-key` — with these differences:

   | Knob | `run-hy4` default | Why |
   |---|---|---|
   | `PACK` / `$1` | the NFS `Hy4-flash-NVFP4.basalt` | where the owner put it |
   | architecture | must be `hy4flash` or `hy_v4_flash`; a `qwen4exp` pack is sent to `./run` | fail fast, by name |
   | `MAX_CONTEXT` | `362144`, checked 1024..1048576 | `./run`'s figure, so `cosmo-6000` clients see one context whichever model serves; the engine checks the range only at load |
   | `KV` | `fp16` (`fp16` \| `fp8`) | upstream's default and the exact one |
   | `PARALLEL` | `1` (1..8), `SLOT_FULL_CONTEXT=1` when > 1 | upstream lists no parallel slots for this model |
   | `VISION` | `dynamic` (`off` \| `dynamic`) | the only image mode |
   | `--ple-io` | never passed | not a Hy4-flash flag |

   When the pack is on NFS it prints one `== note:` line naming the Engram read pattern and the
   local-copy command. Lint it (`shellcheck -x`) **before** `git add`.
3. Gate, same commit (Rule A.5): Expected counts `4 b` → `5 b` product; `run-hy4` joins 1b's identity
   guard; step 4 gains the `run-hy4` dry-run assertions (flags present, `--ple-io` and
   `--slot-full-context` absent, a Flash-Next pack refused with a `./run` pointer, `KV=int8` and an
   out-of-range `MAX_CONTEXT` refused); step 5's flag set includes `run-hy4`'s; new step 5b runs each
   dry run's words through `serve.config.parse_settings` and `serve.engine.vision.model_plan` in the
   image against the pack it names.
4. `README.md`: the second launcher, its knobs, the NFS caveat; `AGENTS.md`: the opening paragraph no
   longer says one launcher and Flash-Next only.

**Before/After**: new file; the evidence source for every flag is the Key Facts table.

**Verify**:
```bash
shellcheck -x run-hy4 lib/common.sh                                  # → no output
bash -c 'source lib/common.sh; pack_arch <each pack>'                # → hy4flash / qwen4exp
DRY_RUN=1 ./run-hy4                                                  # → serve … --max-context 362144 --kv fp16 --parallel 1 --vision dynamic, no --ple-io
DRY_RUN=1 ./run-hy4 /mnt/noir/…/Qwen3.8-FN-NVFP4-DQ6_K.basalt        # → refused, names ./run
EXTRA_ARGS='--ple-io ram' <gate step 5b>                             # → FAIL(pack-flags) … unknown argument: --ple-io   (negative control)
EXTRA_ARGS='--kv int8' <gate step 5b>                                # → FAIL(pack-flags) … --kv int8: the latent caches …
# + the full project gate 1–6 with 5b
```

**Expected observation**:
- **Confirms if**: 5b prints `pack flags ok` for `./run`, `./run --single` and `./run-hy4`, and both
  negative controls print `FAIL(pack-flags)` with the engine's own reason.
- **Refutes if**: 5b refuses a word `run-hy4` passes by default, or passes a control.
- **Within noise if**: the engine words 5b echoes are fewer than the launcher's (`--parallel 1` adds
  none; server options are not engine words).

**Commit**: `[basalt][P11][T1][run-hy4] add ./run-hy4 for Hy4-flash packs`

### T2: `./run` refuses a Hy4-flash pack

**File**: `run`, `context/plans/RULES.md` (step 4), `README.md`

**What to do**: after the magic check, `case $(pack_arch "$PACK")` — `hy4flash` or `hy_v4_flash`
dies naming `./run-hy4`. Nothing else in `run` changes: an unreadable architecture still goes to the
engine, as today. Step 4 gains the assertion; the README's refusal sentence names it.

**Before** (`run`, after the magic check):
```bash
magic=$(LC_ALL=C head -c 8 -- "$PACK" | tr -d '\0')
[[ $magic == BASALTMF ]] || die "$PACK is not a .basalt pack (first bytes '$magic', want 'BASALTMF')."
```

**After**:
```bash
magic=$(LC_ALL=C head -c 8 -- "$PACK" | tr -d '\0')
[[ $magic == BASALTMF ]] || die "$PACK is not a .basalt pack (first bytes '$magic', want 'BASALTMF')."
case $(pack_arch "$PACK") in
  hy4flash|hy_v4_flash) die "…a Hy4-flash pack… serve it with ./run-hy4 instead." ;;
esac
```

**Verify**:
```bash
{ DRY_RUN=1 ./run; DRY_RUN=1 ./run --single; } > before.txt 2>&1     # before the edit; again after, into after.txt
cmp before.txt after.txt                                             # → no output: both dry runs print the same text
DRY_RUN=1 ./run /mnt/noir/…/Hy4-flash-NVFP4.basalt; echo $?          # → refusal naming ./run-hy4, exit 1
# + the full project gate 1–6 with 5b
```
`before.txt` and `after.txt` live in the session's scratch directory, not in the tree.

**Commit**: `[basalt][P11][T2][run] refuse a Hy4-flash pack and point at ./run-hy4`

### T3: Owner boot of `./run-hy4`, with a real reply

**File**: tracker Evidence only

**What to do**: stop whatever serves `:8700` and anything else holding GPU 0's memory. **Owner**:
`./run-hy4`; from another shell one `/v1/chat/completions` request to `http://cosmo.lan:8700/v1` as
`cosmo-6000`; paste `./run-hy4`'s `==` lines, the server's start block, and the reply. The agent
reads `docker inspect -f '{{.Image}}' basalt` and the engine log under `storage/cache/basalt/`.

**Verify**: the pasted output is read, not inferred (`RULES.md` § *Project gate* → Look at it).

**Expected observation** *(written before any boot)*:
- **Confirms if**: `== image:` names an image labelled `ad274aa…`, `== pack:` the Hy4 pack; the start
  block shows `ctx=362144 kv=fp16` and no refused flag; a reply comes back.
- **Refutes if**: the engine refuses the context or the load (VRAM or RAM), or the first prompt
  stalls on Engram reads from NFS (the watchdog's "waiting for the drive" line) — then the default
  pack location or a default value is an owner decision, recorded before any edit.
- **Within noise if**: load time, the VRAM/RAM split of the experts, and the first prompt's read
  speed. Record them; they are the first numbers for this model on this card.

**Commit**: `[basalt][P11][T3][boot] record the owner boot of Hy4-flash`

### T4: Harvest and close

**File**: `context/distilled.md`, `context/pitfalls.md`, `context/high_level.md`, this plan + tracker, `SERIES.md`

**What to do**: `distilled.md`: what Basalt loads now (four models at the pin, two served here), the
per-model flag rule and the no-load check, the Hy4-flash facts T3 confirmed, each tagged. `pitfalls.md`:
the launcher that passed gate step 5 and was refused at boot. `high_level.md`: why two launchers.
Re-read each file after writing. Then `git mv` this plan and its tracker to `completed/`, SERIES done.

**Verify**: every finding the tracker mentions is found by `grep` in the file it names; the full gate.

**Commit**: `[basalt][P11][close] Hy4-flash launcher: <outcome>`

### T5: `./preload` — have the NFS server cache the Engram section

*(Added 2026-11-20; numbered after T4 because tasks are never renumbered. It runs before T4.)*

**File**: `preload` (new), `lib/common.sh` (`pack_sections`; `pack_arch` now reads through it), `run-hy4` (its note), `context/plans/RULES.md` (Expected counts, steps 1b, 4), `README.md`

**What to do**: the owner asked for a script so the `dd` range need not be guessed. `./preload [PACK]
[SECTION…]` finds the section (default `engram`) in the pack's own table and reads it with
`dd iflag=direct,skip_bytes,count_bytes`; then, and alone with `--check`, it times `SAMPLES` random
4 KiB direct reads and says whether the server holds the section (exit 1 when cold). `--list` prints
the table; `DRY_RUN=1` prints the `dd` and reads nothing; a pack that is not on NFS is told there is
nothing to warm.

**Verify**:
```bash
shellcheck -x preload lib/common.sh            # → no output
DRY_RUN=1 ./preload                            # → the dd line with the engram row's offset and size; nothing read
./preload dfly                                 # → a real read of a 1.6 GiB section, then "in the server's RAM"
./preload --check; echo $?                     # → before any preload of engram: COLD, 1
# + the full project gate 1–6 with 5b
```

**Expected observation**: the check must separate the two states measured in T3 (0.05 ms cached,
12.4 ms cold, plus about 1 ms of process start per read). **Refutes if** a cached section and the
cold `engram` section land on the same side of the 4 ms line.

**Commit**: `[basalt][P11][T5][preload] add ./preload to warm a pack's Engram section on the NFS server`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `run-hy4` | new launcher | P0 |
| `lib/common.sh` | `pack_arch` | P0 |
| `context/plans/RULES.md` | Expected counts, gate steps 1b, 4, 5, new 5b | P0 |
| `run` | one refusal after the magic check | P1 |
| `preload` | new (T5) | P1 |
| `README.md`, `AGENTS.md` | the second launcher | P1 |
| `context/plans/SERIES.md` | bookkeeping | P1 |
| `context/distilled.md`, `context/pitfalls.md`, `context/high_level.md` | harvest | P1 |

---

## Open Questions / Risks

1. **OBSERVED (T3, 2026-11-20) — no longer a risk, a measured limit:** from the NFS pack a 456-token
   prompt was read at 53 tok/s and a 14,150-token prompt stalled until the watchdog stopped the engine
   (tracker Evidence rows 8–9). The note below did its job only as a warning; the fix is a local copy
   and a default-pack decision, both the owner's (tracker Follow-ups). Original text:
   **Risk: the Engram table is read from NFS per prompt token.** The same reader on Flash-Next's
   table, 16 rows a token, stalled a cold 121k-token prompt from this NFS (`README.md`
   § *Prerequisites*, 2026-11-06); Hy4-flash reads 64 and has no RAM mode. *Mitigation*: `run-hy4`
   prints a note when the pack is on NFS and names the local-copy command; T3 observes the first
   prompt. Copying 148 GiB to `models.local/` is the owner's step.
2. ~~**Risk: host memory.**~~ RESOLVED (T3): every expert went to VRAM (`RAM 0`), 6395 MiB of VRAM
   was free with everything loaded, and 95 GiB of RAM stayed available. Original text: **Risk: host memory.** Upstream's design holds every expert in RAM (77.9 GiB) beside the VRAM
   slots; the host had 98 GiB available and GPU 0 had 8.7 GiB in use by something else when this was
   written. *Mitigation*: T3 says to free GPU 0 first and records the RAM line of the start block.
3. **Question: are `MAX_CONTEXT=362144` and `KV=fp16` the owner's choices?** They are starting
   values (`./run`'s context; upstream's KV default), not decisions and not measured. *How we'll
   settle it*: the owner, at or after T3; `MAX_CONTEXT=1048576` and `KV=fp8` are one env var away.
4. **Question: does `PARALLEL > 1` work for Hy4-flash?** Upstream lists none; the engine accepts the
   flag. *How we'll settle it*: not in this plan — the knob exists, the default is 1, and a boot with
   it is a Follow-ups row.
5. **Risk: `pack_arch` reads a JSON key with `grep`.** A converter that writes `"general.architecture"`
   differently (escapes, a space before the colon) reads as "no architecture". *Mitigation*: `run-hy4`
   refuses an unreadable architecture by name rather than guessing; `run` lets it through to the
   engine, as before T2.
6. **Risk: the built pin is unaudited** (Plan 10 Open Question 5), and `src/models/hy4flash` is new
   code no audit has read. *Mitigation*: none here; it is Plan 09's, and serving this model is the
   owner's request.

---

## Verification Checklist

- [x] T1 *(2026-11-20, tracker Evidence rows 4–6)*: `DRY_RUN=1 ./run-hy4` prints a `serve` line with `--kv fp16`, `--parallel 1` and no `--ple-io`; gate step 5b prints `pack flags ok` three times and both negative controls print `FAIL(pack-flags)`
- [x] T2 *(2026-11-20, tracker Evidence row 7)*: `DRY_RUN=1 ./run <the Hy4 pack>` exits 1 naming `./run-hy4`; the default and `--single` dry runs are byte-identical before and after
- [x] T5 *(2026-11-20, tracker Evidence row 11)*: `./preload dfly` reads the section and reports it cached; `./preload --check` on the never-read `engram` section reports COLD and exits 1
- [ ] T3: the boot log names the `ad274aa` image and the Hy4 pack; a real reply is quoted in the tracker
- [ ] T4: each harvested fact is in the file the tracker names, re-read after writing
- [ ] All: the project gate (Rule A) passes on the final commit
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
