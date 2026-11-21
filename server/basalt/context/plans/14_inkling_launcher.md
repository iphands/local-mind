# Plan 14 — Inkling 2 Mini launcher: `./run-ink`

> **Status**: in-progress
> **Created**: 2026-11-21
> **Depends on**: Plan 13 T1 (the v0.5.5 image, the first that builds `inkling2`); Plan 11 (the launcher this one is shaped after, `pack_arch`, gate steps 4 and 5b)
> **Goal**: `./run-ink` serves Thinking Machines Lab's Inkling 2 Mini pack on `:8700` with that model's own flags, refuses any other model's pack and a pack that is still downloading, and one owner boot answers a real request.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: A third launcher, for the model v0.5.5 added, checked by the gate and booted once.

**Deliverables**:
1. `./run-ink`, a `pack_declared_bytes` helper, gate steps 1 and 4 extended, `README.md`, `AGENTS.md`.
2. `./run` and `./run-hy4` name `./run-ink` when handed an Inkling pack.
3. Once the pack has finished downloading: its sha256 against the model card, and gate steps 4, 5
   and 5b run on the real pack.
4. One owner boot with a real reply, recorded.
5. Facts harvested into `context/`.

**Estimated effort**: Small–Medium (half a day of agent work; one owner boot).

---

## Scope

**In**: `run-ink` (new), `lib/common.sh` (one helper), the two arch `case`s in `run` and `run-hy4`,
the gate snippets and Expected counts in `context/plans/RULES.md`, `README.md`, `AGENTS.md`, plan
bookkeeping, `context/`.
**Out**: the pack download (the owner's, in progress); tuning any default (context, KV, slots,
drafting depth); the completeness check in `./run` and `./run-hy4` (a Follow-ups row); narrowing
`BASALT_MODELS`; `versions.env`, `Dockerfile`, `patches/`. If a fix needs to cross this line,
**stop** and record the blocker in the tracker.

---

## Context

The owner asked (2026-11-21): "See https://huggingface.co/jesdga/Inkling-2-Mini-Basalt model card and
basalt github make a ./run-ink runner for this model. I have the model downloading now (not usable
yet, still in progress)". A new launcher profile is a plan (`RULES.md` § *When Does a Change Need a
Plan?*): a pack Basalt cannot load must fail fast in the launcher, not in the engine. Plan 13's
Follow-ups carried this as "a launcher for Inkling 2 Mini"; Plan 11 is the precedent.

### Pre-Identified Bug/Issue

A feature. Today there is no launcher for the model the image now builds:

```bash
$ ls run-ink
ls: cannot access 'run-ink': No such file or directory
$ grep -n 'has no launcher' README.md
17:image holds five models and this project serves two; Inkling 2 Mini, new in v0.5.5, has no launcher
```

and neither existing launcher fits it. Its adapter takes three flags of its own, where `./run`
passes `--kv int8 --ple-io ram`:

```bash
$ grep -n 'a == "--' vendor/basalt/src/models/inkling2/adapter.cpp vendor/basalt/src/models/inkling2/dspark.cpp
vendor/basalt/src/models/inkling2/dspark.cpp:723:    if (a == "--spec") {
vendor/basalt/src/models/inkling2/dspark.cpp:726:    } else if (a == "--draft-log") {
vendor/basalt/src/models/inkling2/adapter.cpp:41:        if (a == "--kv") {
```

### Why a third launcher, and a synthetic pack in the gate

- **A launcher per model**, as Plan 11 decided (§ *Why a second launcher*): engine flags are per
  model, and a launcher that guesses from the pack hides which flags a boot got. `./run-hy4`'s flags
  happen to be valid names here too (`--max-context`, `--kv fp16`), but its refusals, its NFS note
  and its header are Hy4-flash's. Rejected: `PACK=<inkling> ./run-hy4` with the arch check loosened.
- **The docker half is repeated**, as in `./run-hy4`: change the container shape in one and change
  it in the others. Rejected here for the same reason as there: a shared launch function would be a
  change to `./run`, the daily launcher, inside a plan about another model.
- **Gate step 4 checks `./run-ink` on packs it writes itself** (a header, a table and a `meta` section
  of a few hundred bytes). The real pack is still downloading, and a pack that is cut short can only
  ever be had by making one. Rejected: waiting for the download before the launcher can be
  committed — it would leave a launcher, two edited launchers and gate edits uncommitted in a
  worktree with two writers (Project Rule P1). The real pack joins the gate in T3.
- **Refuse an incomplete pack by the header's own size field.** A `.basalt` header records the
  file's length; a file shorter than that is a download in progress or a cut-short copy. Rejected:
  leaving it to the arch check, which says "architecture unreadable" for it (observed, Key Facts).

### Key Facts

`[SOURCE]` rows are `vendor/basalt` at the pin `ff0b978` (v0.5.5). `[CARD]` rows are the model card
at `huggingface.co/jesdga/Inkling-2-Mini-Basalt` and its file list, read 2026-11-21 through a fetch
tool that returns quotes and paraphrase — claims until T3 and T4 confirm them.

| Fact | Value | How confirmed |
|---|---|---|
| The pack | one file, `Inkling-2-Mini-NVFP4-q8.basalt`, 19.7 GB, sha256 `f7b4e5f8e17681c4735a7dfc51ffd8e805c44184fad4e1778b13187d79dc3787`; needs Basalt 0.5.5 or later; Apache-2.0 weights, repacked, not an official release | `[CARD]`; the name and size also in `[SOURCE]` `docs/models/inkling2.md` § *Pack*. The hash is also in the download's own temp-file name (below). T3 hashes the file |
| Where it is landing | `/mnt/noir/scratch/ai/llm/models/vllm/jesdga/`, beside the other two packs: at 13:13 PST an `hf download` temp file `.cache/huggingface/download/….f7b4e5f8….incomplete`, which is renamed to the pack's name when it finishes | `ls -la`, `find … -newermt` in that directory, 2026-11-21 |
| A cut-short pack with the final name is possible | at 13:11 PST the directory held `Inkling-2-Mini-NVFP4-q8.basalt` at 1,724,310,502 bytes (an earlier download attempt, gone two minutes later): magic `BASALTMF`, a 10-section table whose last section `meta` ends at byte 19,734,300,551, and `pack_arch` printed nothing, because `meta` was not there yet | `stat`, `pack_sections`, `pack_arch` on it, 2026-11-21 |
| The header carries the file's length | `HEADER = struct.Struct("<8sIIQ40s")`: magic, version, section count, `file_bytes` (a u64 at byte 16); for both complete packs here it equals `stat -c %s` (158746168309, 131681979197) | `[SOURCE]` `tools/model_file.py:27,143`; `od -An -tu8 -j16 -N8` on each, 2026-11-21 |
| Architecture id | `inkling2` | `[SOURCE]` `src/models/inkling2/meta.hpp:12`; `docs/models/index.md` |
| Model flags | `--kv fp16` (the default) or `fp8`, anything else is an error; `--spec N` and `--draft-log` for the drafter | `[SOURCE]` `src/models/inkling2/adapter.cpp:39-53`, `dspark.cpp:722-732` |
| Context | `--max-context` 1,024 to 1,048,576, default 8,192; outside the range the model refuses at load (`model.cpp:301-302`) | `[SOURCE]` `docs/models/inkling2.md` § *Memory*; `[CARD]` gives the same ceiling |
| Memory | on the card: 1.10 GiB dense, 14.71 GiB experts, 0.48 GiB drafter; the token embedding (0.86 GiB) in page-locked RAM; per position 32 KiB with fp16, 16 KiB with fp8, plus the drafter's full layer. One card only | `[SOURCE]` same file, § *Memory*, § *Limits* |
| What that is here | 362,144 positions × 32 KiB = 11.05 GiB, 1,048,576 × 32 KiB = 32 GiB, beside about 16.3 GiB of weights on a 96 GiB card: by this arithmetic every expert stays on the card at any context (the drafter's layer is not in the sum) | arithmetic from the row above; `nvidia-smi` 97887 MiB. **Not measured** — T4's start block shows the plan the engine made |
| Experts in RAM | only "when the asked context does not fit beside every expert"; then page-locked, so `--ulimit memlock=-1` stays | `[SOURCE]` § *Memory* |
| Drafting | the pack's DSpark drafter, on by default; `--spec N` caps the window (default 8), `--spec 1` turns it off; output is plain decoding's | `[SOURCE]` § *Drafting*; `[CARD]` |
| Parallel slots | "Not measured. `--parallel N` starts N sessions, each with its own caches and drafter scratch, the context split between them." | `[SOURCE]` § *Parallel slots* |
| Images, video | `--vision dynamic` only; no video | `[SOURCE]` § *Images*, § *Limits*; `docs/models/index.md` |
| Thinking | a request's `reasoning_effort` (`none` … `max`, or 1 to 100); "There is no hard switch: at `none` the model still writes a short thinking block (a few hundred tokens), so leave room in `max_tokens`." Not a launcher flag | `[SOURCE]` § *Chat, thinking and tools* |
| No table read per prompt token | nothing in the guide or the model's sources reads the pack during a prompt; `grep -rn 'open_unbuffered\|DirectFile\|O_DIRECT' src/models/inkling2` prints nothing | `[SOURCE]`; Plan 13 tracker, Evidence row 5. So no NFS note and no `./preload` step; T4 shows whether a prompt reads the disk |
| Upstream's own command | `bin/basalt serve models/Inkling-2-Mini-NVFP4-q8.basalt --vision dynamic`, with `--max-context N` for a longer context | `[CARD]` |
| The image | `iphands/basalt-blackwell:cu1343-sm120-ff0b978` (`a3441f94bdf1`) ships `data/profiles/inkling2-24x128.prof` | Plan 13 tracker, Evidence row 8 |
| Gate baseline | 1–6 with 5b pass on that image at `f484d57` | Plan 13 tracker, Evidence row 9; re-run before this plan's commit (tracker Evidence row 1) |

### Rejected Claims

- **"Use `./run-hy4`'s defaults because they were chosen."** They were not: its header calls them
  starting values. `./run-ink` takes the same two (`MAX_CONTEXT=362144`, the engine's `--kv fp16`)
  and says the same. Do NOT present them as tuned.
- **"The pack on NFS needs `./preload`."** That is Hy4-flash's Engram table. Do NOT copy the NFS note.
- **"Serve at 1,048,576 because it fits."** The arithmetic says it might; nothing here measured it.
  The default stays the other launchers' 362144 until a boot shows otherwise.

---

## Step-by-Step Tasks

### T1: `./run-ink`, the completeness helper, the gate

**File**: `run-ink` (new), `lib/common.sh`, `context/plans/RULES.md` (Expected counts, steps 1b, 2, 4), `README.md`, `AGENTS.md`

**What to do**: `lib/common.sh`: `pack_declared_bytes <pack>` prints the header's `file_bytes` (or
nothing). `run-ink`, shaped after `run-hy4`: knobs `PACK` (default the NFS path above), `MAX_CONTEXT`
362144 (1024..1048576), `KV` fp16 (`fp16` | `fp8`), `PARALLEL` 1 (1..8), `SLOT_FULL_CONTEXT`,
`VISION` dynamic (`off` | `dynamic`), `BUFFERED_IO` 1, and the shared ones (`PORT`, `ALIAS`,
`ALLOWED_HOSTS`, `API_KEY`, `NAME`, `CACHE_DIR`, `EXTRA_ARGS`, `IMAGE_OVERRIDE`). Checks, in order:
`.ninfer` path, exists, readable, magic, **complete** (size on disk ≥ the header's `file_bytes`,
else "incomplete … still downloading?" with both figures), architecture `inkling2` (a Flash-Next
pack names `./run`, a Hy4-flash pack `./run-hy4`). No NFS note. `--spec` is not a knob:
`EXTRA_ARGS='--spec 1'` turns drafting off. Gate: Expected counts `6 b` → `7 b`, the identity guard
and step 2's comment name `run-ink`; step 4 writes two packs of its own (a complete `inkling2` one
and one whose header claims more bytes than the file has) and asserts the dry run's words, the
refusals and the pointers.

**Verify**:
```bash
shellcheck -x run-ink lib/common.sh                       # before git add — an untracked file is invisible to the gate
DRY_RUN=1 ./run-ink <a complete synthetic inkling2 pack>  # → --max-context 362144 --kv fp16 --parallel 1 --vision dynamic, no --ple-io
DRY_RUN=1 ./run-ink <the same, cut short>                 # → "incomplete", exit 1
DRY_RUN=1 ./run-ink                                       # today: "pack not found" (the download has not finished)
# the dry run's flags through gate step 5's check by hand: every name accepted by the image's binary
# + the full project gate 1–6 with 5b
```

**Commit**: `[basalt][P14][T1][run-ink] add ./run-ink for Inkling 2 Mini packs`

### T2: `./run` and `./run-hy4` name `./run-ink`

**File**: `run`, `run-hy4`, `context/plans/RULES.md` (step 4), `README.md`

**What to do**: one `case` arm in each: an `inkling2` pack is refused with "serve it with `./run-ink`
instead". `./run` still sends any other unknown architecture to the engine, as before. Gate step 4
asserts both pointers on the synthetic pack.

**Verify**: `DRY_RUN=1 ./run <synthetic inkling2 pack>` and `DRY_RUN=1 ./run-hy4 <the same>` exit 1
and print the pointer; the `serve` words of `./run`, `./run --single` and `./run-hy4` on their own
packs are byte-identical to before (`cmp`); the full gate.

**Commit**: `[basalt][P14][T2][run] point an Inkling pack at ./run-ink from ./run and ./run-hy4`

### T3: the real pack — hash, and the gate on it

**File**: `context/plans/RULES.md` (steps 4, 5, 5b), tracker Evidence

**What to do**: when the download has finished (the pack exists under its own name and its size is
the header's `file_bytes`): `sha256sum` against the card's hash; `./preload --list`-style read of its
table (`pack_sections`), `pack_arch`. Gate: step 4 gains the default `DRY_RUN=1 ./run-ink` on the
real pack and the cross-refusals with the real packs; steps 5 and 5b gain `./run-ink` in their
launcher lists (5b then prints four `pack flags ok` lines). 5b's negative control:
`EXTRA_ARGS='--kv int8'` → `FAIL(pack-flags) ./run-ink: …`.

**Expected observation** *(written before the pack exists)*:
- **Confirms if**: the hash matches; `pack_arch` prints `inkling2`; 5b prints
  `pack flags ok (./run-ink: inkling2 takes --max-context 362144 --kv fp16)`.
- **Refutes if**: the hash differs (a damaged download: the owner re-fetches); the model's parser
  refuses a word `./run-ink` passes — then the launcher changes, not the gate.
- **Within noise if**: 5b echoes the words in another order or adds a derived one, as it does
  `--batch 3` for `./run`.

**Commit**: `[basalt][P14][T3][gate] check ./run-ink against the downloaded pack`

### T4: owner boot — a real reply

**File**: tracker Evidence

**What to do**: stop whatever serves `:8700`. **Owner**: `./run-ink`, then one
`/v1/chat/completions` request to `http://cosmo.lan:8700/v1` as `cosmo-6000`; paste the `==` lines,
the start block and the reply. Agent reads `docker logs` and the engine log.

**Expected observation** *(written before the boot)*:
- **Confirms if**: `== image:` names the `ff0b978` image; the engine log says `session is up (engine
  0.5.5)`; the start block shows `ctx=362144 kv=fp16`, every expert on the card (no RAM spill) and
  drafting on; a reply comes back.
- **Refutes if**: the engine refuses a flag or the context; experts are planned into RAM at this
  context (then the memory arithmetic in Key Facts is wrong, and the default is an owner decision);
  a prompt stalls on disk reads (then the "no table read" fact is wrong).
- **Within noise if**: load time and rates differ from upstream's RTX 5090 figures. Record them.

**Commit**: `[basalt][P14][T4][boot] record the owner's boot of Inkling 2 Mini`

### T5: Harvest and close

**File**: `context/distilled.md`, `context/pitfalls.md`, `context/high_level.md`, this plan + tracker, `SERIES.md`

**What to do**: `distilled.md`: the model on this card (the plan the engine made, load time, the
rates T4 saw), the pack's header length field and what a download in progress looks like.
`pitfalls.md`: anything that took two attempts. Then `git mv` to `completed/`, SERIES done.

**Commit**: `[basalt][P14][close] Inkling 2 Mini launcher: <outcome>`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `run-ink` | new | P0 |
| `lib/common.sh` | `pack_declared_bytes` | P0 |
| `context/plans/RULES.md` | Expected counts, gate steps 1b, 2, 4, 5, 5b | P0 |
| `run`, `run-hy4` | one `case` arm each | P1 |
| `README.md`, `AGENTS.md` | the third launcher | P1 |
| `context/` | harvest | P1 |

---

## Open Questions / Risks

1. ~~**Risk: the launcher is written before its pack can be read.**~~ RESOLVED (T3) for the pack and the
   flags: the hash matches and the model's parser takes the launcher's words (tracker Evidence rows
   7–8); the boot is still T4. Every engine-side fact was from the pinned source and the card. *Mitigation*: T1 checks flag names against the built binary; T3 runs
   the model's own parser on the real pack (gate 5b); T4 boots. T1 is `done` as a launcher that
   dry-runs, not as one that has served.
2. **Risk: gate step 4's synthetic pack drifts from the real format.** It has only what the launcher
   reads: the magic, the count, the length, one table entry, `meta`. *Mitigation*: T3 adds the real
   pack beside it; the synthetic one stays for the cut-short case, which no real file can supply.
3. **Question: is 362144 the right context for this model here?** The engine's default is 8,192, the
   model's maximum 1,048,576, and the arithmetic says either fits. *How we'll settle it*: T4 shows
   the memory plan at 362144; the default is the owner's to change.
4. **Question: is `PARALLEL` above 1 usable?** Upstream: not measured. *How we'll settle it*: not in
   this plan; the knob exists, the default is 1, a Follow-ups row.
5. **Risk: `:8700` serves one model at a time**, so booting Inkling stops Flash-Next. *Mitigation*:
   `./run-ink` refuses while a `basalt` container runs, as the others do; T4 is the owner's to time.
6. **Risk (found at T1): the completeness check sees length, not holes.** The owner's second download
   wrote a temp file that already had the pack's full length while a tenth of it was still unwritten
   (tracker Evidence row 3), and `./run-ink` accepted it. Under its own name such a file would be
   served. *Mitigation*: `hf download` gives the file its name only when it is whole; T3 hashes the
   pack once; the engine verifies sections it reads. A check of every byte at each start is not
   worth 20 GB of reads.

---

## Verification Checklist

- [x] T1 *(2026-11-21, tracker Evidence rows 2–4)*: `DRY_RUN=1 ./run-ink` on a complete synthetic `inkling2` pack prints Inkling's flags and none of Flash-Next's; on a cut-short one it exits 1 with "incomplete"; gate step 4 asserts both
- [x] T2 *(2026-11-21, tracker Evidence row 5)*: `./run` and `./run-hy4` refuse an `inkling2` pack and name `./run-ink`; their own `serve` words are unchanged
- [x] T3 *(2026-11-21, tracker Evidence rows 7–8)*: the pack's sha256 is the card's; gate step 5b prints a `pack flags ok (./run-ink: inkling2 …)` line
- [ ] T4: the boot log names the `ff0b978` image and the pack; a real reply is quoted in the tracker
- [ ] T5: each harvested fact is in the file the tracker names, re-read after writing
- [ ] All: the project gate (Rule A) passes on the final commit
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
