# Plan 01 — Ignis launchers for Flash-Next and the 27B on :8700

> **Status**: in-progress
> **Created**: 2026-10-27
> **Depends on**: N/A
> **Goal**: `./qwen38fn/run` serves Qwen3.8-Flash-Next and `./qwen38-27b/run` serves the Qwen3.8-27B through the pinned upstream Ignis 0.8.0 image on `:8700` as `cosmo-6000`, with the README saying how to download the weights, and a same-session bench against vLLM FP4-Local recorded.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Pin the upstream Ignis 0.8.0 image and the Hugging Face weight revisions its catalog
pins, write two launchers that run it on the one RTX PRO 6000 under the built-in `rtxpro6000`
profile on `:8700` as `cosmo-6000`, document the weight downloads (never run them), and have the
owner boot, smoke-test and bench them against the vLLM FP4-Local daily driver.

**Deliverables**:
1. `versions.env` (image by tag **and** digest, Ignis source ref, three HF repos + revisions),
   `lib/common.sh` (guards and helpers), `models` symlink, `.gitignore` `/storage/`, and
   `README.md` with the download how-to (container and `hf` forms) and the run commands.
2. `qwen38fn/run` — Flash-Next (trellis a25 + MTP companion), MTP 3, 8 lanes, 262,144 context.
3. `qwen38-27b/run` — the 27B NVFP4 + DFlash2 7 (`ABLITERATED=1` → the abliterated artifact).
4. `scripts/launcher-dryrun` and project gate step 4: both launchers against a stub `docker`.
5. Owner-run evidence: image pulled, `PRINT_CONFIG=1` accepted by the real binary, first boots,
   smoke requests, and Ignis-vs-vLLM numbers (N ≥ 3) in the tracker's Evidence table.

**Estimated effort**: Medium (1 day of agent work for T1–T3; the owner's T4–T6 add ~90 GiB of
downloads, boots and bench sessions).

---

## Scope

**In**: `server/ignis/` only — launchers, pins, shared lib, dry-run harness, README, this
project's `context/`. Reading (never editing) `../vllmcustom/bench/` and running it with `--out`
into our `captures/`.
**Out**: building Ignis from source, patching it, changing `llama-proxy`'s config, touching
`../vllmcustom` or `../ninfer` files, tuning sweeps beyond the profile (a later plan). If a fix
needs to cross this line, **stop** and record the blocker in the tracker instead of expanding scope.

---

## Context

The user wants Qwen3.8-Flash-Next served through [Ignis](https://github.com/gpillon/ignis) — a
single-GPU Rust + C++/CUDA engine for exactly this model family on `SM120a` — from a
`qwen38fn/run` script, plus the 27B, on the **same port** as the vLLM daily driver
(`../vllmcustom/qwen3.8-flash-next/run-fp4local`) so `llama-proxy` keeps working, and fast on the
RTX PRO 6000. Weights are **not** downloaded by agents: the README says how (decision
2026-10-27). Upstream publishes a `rtxpro6000` profile and PRO 6000 numbers (Flash-Next 258 tok/s
one lane on code, 427 at eight); this plan makes them reproducible on our card and comparable to
FP4-Local. Context relied on: `context/plans/SERIES.md` Standing Constraints (owner runs GPU
steps, no sudo, one server on `:8700`, host facts); `../vllmcustom/context/distilled.md` for the
FP4-Local numbers; `../ninfer/lib/common.sh` as the guard reference.

### Pre-Identified Bug/Issue

Feature — today nothing in this project serves; `:8700` is the vLLM daily driver, answering to
five names (2026-10-27):

```bash
$ ls qwen38fn/run
ls: cannot access 'qwen38fn/run': No such file or directory
$ curl -sS -m 5 localhost:8700/v1/models | python3 -c 'import sys,json; print([m["id"] for m in json.load(sys.stdin)["data"]])'
['cosmo-6000', 'cosmo-proxy', 'claude-heavy', 'claude-light', 'claude-haiku-4-5-20251001']
$ docker ps --filter publish=8700 --format '{{.Names}} {{.Image}} {{.Ports}}'
vllm iphands/vllm-blackwell:cu1321-sm120-main-vllm0.34.0.dev20261027-g40fe396 0.0.0.0:8700->8799/tcp
```

`llama-proxy` routes `model: "cosmo-6000"` to `http://cosmo.lan:8700`
(`../proxy/config.yaml:9-10,30-31`).

### Why the pinned upstream image + per-model launchers

- **Upstream image, pinned by digest** — `ghcr.io/gpillon/ignis:0.8.0` is a release built for
  `SM120a` on CUDA 13.1.1 (`Containerfile:19-21,85`), which the host's 615.78.08 driver runs.
  **Rejected: building from source** (ninfer's shape): an hours-long owner build and a toolchain
  pin set to maintain, for nothing until we need a patch. A digest pin keeps the image from moving
  under us the way `latest` would. **Rejected: `develop` / `main` (0.9.0)**: 260 commits past the
  release and not published as a version; a later plan can bump the pin.
- **One launcher per model** (the user asked for `qwen38fn/run`; it also matches vllmcustom's
  `<family>/run` layout) with `--switch-allow-implicit false`, so a request can never trigger a
  60–124 s reload into the other model. **Rejected: one server switching models by name** — Ignis
  can, but our one served name `cosmo-6000` must mean one model, and an implicit switch on a
  shared port is a surprise reload for every other client.
- **`--model-artifact` + `--model-id cosmo-6000`**, not catalog lookup: a non-catalog id
  *without* an artifact silently boots the CPU mock (`crates/server/src/download/mod.rs:60-65,
  120-125`), so the launcher always names the file and refuses when it is missing.
- **`--user 1000:1000`** with `HOME=/cache`, not the image's uid 10001: upstream's own fix for a
  writable cache is `sudo chown 10001` (`docs/user/start/install.md:73-77`), and this project has
  no sudo. Every writable path is passed explicitly, so nothing needs `/home/ignis`.

### Key Facts

All `path:line` cites are in the Ignis source at tag **v0.8.0** (`950ee0d1`), read 2026-10-27
from a clone in the session scratchpad. **RF** = `docs/findings/2026-10-18-rtxpro6000-profile-measured.md`.
Rows marked "research agent" were read by a research subagent and not re-read line by line;
the rest were re-read directly.

| Fact | Value | How confirmed |
|---|---|---|
| Image pin | `ghcr.io/gpillon/ignis:0.8.0` = `latest` = `sha256:83d0ccc3c2dd48280e34e99524626ade0b0fbe5869098fce4be8b45966cdcb6e`; `develop` is a different digest; no `0.9.0` tag | ghcr registry API (`/v2/gpillon/ignis/tags/list`, manifest HEAD), 2026-10-27 |
| Source of the image | tag `v0.8.0` = `950ee0d1c2cf6178cd7fec29df3f17c3e127f987` (2026-10-24); `main` is 260 commits ahead at 0.9.0 | `git log`/`git rev-list --count v0.8.0..HEAD` on the clone, 2026-10-27 |
| Container shape | `ENTRYPOINT ["/usr/local/bin/ignis-server"]`, `USER ignis` (uid 10001), `ENV IGNIS_SERVER_BIND=0.0.0.0:8000`, `EXPOSE 8000 9464`, CUDA 13.1.1 base | `Containerfile:85,92,108,125-130` |
| Served name | `--model-id` takes any id except the *other* family's; exactly one id is served | `crates/server/src/config.rs:1514-1523`; test `config/tests.rs:1951-1965` (`my-id` on Flash-Next → `my-id`) |
| Unknown / blank `model` | unknown → `404 model_not_found` "unknown model: X (loaded: Y)", no switch; blank or missing → the loaded model | `crates/server/src/api.rs:577-586,814-816` |
| The other four vLLM names | **will 404** under Ignis — there is no multi-alias mechanism in v0.8.0 (catalog `aliases` are family default ids only) | `crates/server/src/download/catalog.yaml:14-16`; research of `main` found none either, 2026-10-27 |
| Mock trap | a non-catalog `--model-id` without `--model-artifact` serves the CPU **placeholder** with a WARN | `crates/server/src/download/mod.rs:60-65,120-125` |
| Profile selection | `auto` reads NVML total memory of **CUDA device 0** (88–104 GiB → `rtxpro6000`); a named profile always wins; device 0 is hardcoded | `docs/user/reference/cli.md:36-38`; `crates/server/src/main.rs:52-56`; `crates/server/src/runtime.rs:967` |
| `rtxpro6000` profile | 27B: `max_context 1048576`, `prefill_chunk 2048`, DFlash2 `draft_tokens 7`. Flash-Next: `max_context 1048576`, MTP `draft_tokens 3`, `decode_lanes 8`. Both: `vision.enabled true`, `ngram.hot_bytes auto` | `crates/server/src/config/profile.rs:216-227`; `docs/user/reference/profiles.md:45-58` |
| Precedence | patch > flag > env > file > profile > default; a per-model flag (`--qwen38flashnext-…`, `--qwen38-…`) beats the general one | `crates/server/src/config/source.rs:433-487` (research agent; not re-read) |
| Context decision | **262,144** (`--model-max-context 262144`): `auto` rope applies no scaling up to 262,144, so no YaRN — matches FP4-Local (user decision 2026-10-27) | `README.md` "A context you can rescale"; `docs/user/reference/configuration.md:215` |
| Idle unload | default 300 s; `--model-idle-unload 0` never unloads — **launcher default 0** (user decision 2026-10-27) | `docs/user/reference/configuration.md:602-614` |
| Flags the launchers pass | `--model-artifact` (:202), `--model-id` (:189), `--download-enabled` (:1190), `--switch-allow-implicit` (`docs/user/configuration/model-and-server.md:24`), `--model-idle-unload` (:606), `--model-max-context` (:215), `--ngram-persist-path` (:1270, `model\|auto\|dir`), `--kv-disk-path` (:1312, `model\|auto\|dir`), `--profile` (`cli.md:36`) | `docs/user/reference/configuration.md` row numbers as listed |
| Writes beside the artifact | n-gram cache and KV-disk default to `model` (beside the artifact); on a `:ro` mount they WARN and turn off — so both go to `/cache` | `docs/user/reference/configuration.md:1262-1320`; `docs/user/start/install.md:61-83` |
| Reads | `O_DIRECT`, falling back to buffered where refused; **no io_uring** (unlike NInfer) — docker's default seccomp is fine | `crates/artifact/src/direct.rs:5-9`; grep of crates/kernel/Cargo.lock by research agent |
| Host RAM (Flash-Next, 96 GB card) | every expert stays in VRAM; load needs ~9 GB available, or ~37 GB for the whole 28.8 GB n-gram table (`auto`); no `--memory` cap (cgroup v2 limit caps the plan) | `docs/user/configuration/host-ram.md:29-34`; RF:249-257 (research agent) |
| No init | the server is PID 1 on purpose; it drains on SIGTERM, a second signal ends it | `docs/user/troubleshooting.md:378-384` |
| Config check without a model | `ignis-server config print [--format json\|yaml] [FIELD FLAGS]`; name `--profile` (auto reads NVML) | `docs/user/reference/cli.md:26`; `docs/user/configuration/index.md:57-64` (research agent) |
| Flash-Next weights | `gpillon/Qwen3.8-Flash-Next-trellis-a25-ignis` @ `8d94db83cd8e58d6be31e796723e5e35b960638c`: `qwen3_8_flash_next_trellis_a25-v2.ninfer` (71,760,711,680 B) + `.conversion.json`, `qwen3_8_flash_next_mtp_3p0-v2.ninfer` (1,037,905,920 B) + `.conversion.json`; 67.80 GiB; Qwen Community License 1.0; not gated | `crates/server/src/download/catalog.yaml:41-50`; HF API (research agent, 2026-10-27) |
| MTP companion | required when MTP is on (the profile turns it on); missing → start refused | `docs/user/configuration/flash-next-mtp.md:13-15` |
| 27B weights | `gpillon/Qwen3.8-27B-nvfp4full-dflash2-NInfer` @ `e961b419b672e183aa55df8c4b975abc82006e8a`: `qwen3_8_27b_nvfp4full-v2.ninfer` (19,406,942,468 B) + `.graft.json`; abliterated: `…-abliterated-NInfer` @ `ef3216949e42c42fc713feffb2e05849d57f0f3b`, `qwen3_8_27b_nvfp4full-v2-huihui-abliterated.ninfer` + `.graft.json`; apache-2.0 | `crates/server/src/download/catalog.yaml:18-36` |
| HF HEAD vs pins | HF HEAD moved past every pin (card/README edits only; large-file sizes and hashes match) — pin the catalog revisions | HF API (research agent, 2026-10-27) |
| Download layout | flat `<dir>/<file name>`; `.part` + resume + SHA-256 before rename; `ignis-server model download <id…> --out <dir>` needs no GPU | `crates/server/src/download/catalog.rs:130-131`, `download/transfer.rs:5-12` (research agent) |
| Bench client fit | `../vllmcustom/bench/bench` sends `temperature 0.6`, `stream`, `stream_options.include_usage`, `ignore_eos` with `max_tokens`; Ignis honours `ignore_eos` with a cap (unbounded → 400) | `../vllmcustom/bench/lib/benchclient.py:30-60`; `crates/server/tests/openai_http.rs:972-1003`. `include_usage` on Ignis: **unconfirmed — confirm in T4** |
| `--user 1000:1000` works | the server needs no write under `/home/ignis` when every path is explicit | **unconfirmed — confirm in T4** |
| GPU mapping | host GPU 0 = RTX PRO 6000 (`GPU-07e366a8-…`), GPU 1 = RTX 4060; `--gpus device=0` + `CUDA_DEVICE_ORDER=PCI_BUS_ID` is vllmcustom's working pattern | `nvidia-smi -L`, 2026-10-27; `../vllmcustom/common/_run-lib.sh:257-258` |
| Bench `--out` | `bench/bench` appends to `bench/bench-results.md` beside itself unless `--out` is given — always pass `--out captures/…` | `../vllmcustom/bench/bench:38-41,54,79` |

### Prior Measurements

| Source | Metric | Result | Runs | Spread |
|--------|--------|--------|------|--------|
| `../vllmcustom/context/distilled.md:109` (Plan 06 tracker, `scripts/flashnext-matrix`, `…gfe8f214`, 2026-10-18) | vLLM FP4-Local decode, code prompt, single stream, humming, MTP K=3 | 195.5 tok/s | 4 | 188.5–210.6 |
| same | vLLM FP4-Local decode, long (~16k ctx), K=3 | 190.7 tok/s | 4 | 184.4–197.4 |
| Upstream `[DOC]` `docs/findings/2026-10-18-rtxpro6000-profile-measured.md:374-379` (**Server Edition on AWS**, greedy, 512 tokens, `--model-loop-guard off`, EBS gp3) | Ignis Flash-Next, 1 lane code / prose / 8 lanes code | 258.2 / 193.7 / 427.5 tok/s | 3 (median) | not stated |
| Upstream `[DOC]` same file, RF:181-185 (older develop build) | Ignis 27B, 1 request code / prose, DFlash2 7 | 256 / 179 tok/s | 3 (median) | not stated |

The upstream rows are a different card edition, sampling and prompt set — a claim to reproduce,
not a baseline. The FP4-Local rows are 9 days and two image pins old (`…g40fe396` is built but
unbenched): T6 re-measures vLLM in the same session.

### Rejected Claims

- **"The image needs a seccomp profile for io_uring, like NInfer."** No — Ignis reads with
  `O_DIRECT` and has no io_uring (`crates/artifact/src/direct.rs:5-9`). Do NOT add one.
- **"`latest` is fine."** It equals `0.8.0` today, but it is a moving tag. Pin by digest.

---

## Step-by-Step Tasks

**Wave 1 (agent, no GPU) — T1–T3.** Exits when both launchers pass gate steps 1–4.
**Wave 2 (owner runs, agent records) — T4–T6.** Exits when the Evidence table holds both boots and
the bench.

### T1: Pins, shared guards, models link, README download how-to

**File**: `versions.env` *(new)*, `lib/common.sh` *(new)*, `models` *(new symlink)*,
`.gitignore`, `README.md` *(new)*, `context/distilled.md`

**What to do**:
- `versions.env` — plain `KEY=value` (gate step 3): `IGNIS_IMAGE=ghcr.io/gpillon/ignis:0.8.0@sha256:83d0ccc3…cb6e`
  (full digest), `IGNIS_VERSION=0.8.0`, `IGNIS_SRC_REF=950ee0d1c2cf6178cd7fec29df3f17c3e127f987`,
  `FN_REPO`/`FN_REV`, `Q27_REPO`/`Q27_REV`, `Q27A_REPO`/`Q27A_REV` (Key Facts), and the artifact
  file names (`FN_ARTIFACT`, `FN_MTP`, `Q27_ARTIFACT`, `Q27A_ARTIFACT`). Comment lines carry the
  provenance (date + how confirmed).
- `lib/common.sh` — **written fresh; `../ninfer/lib/common.sh` and
  `../vllmcustom/common/_run-lib.sh` are references (read, never sourced)**: sourcing it runs the
  guards (DOCKER string/empty/sudo, not root, `docker version` socket probe with a 15 s timeout);
  helpers `die`, `load_pins` (requires every key), `owned_dir` (mkdir as you, refuse unless uid
  1000 owns it), `refuse_busy <port> <name>…` (anything publishing `:8700`, or containers named
  `vllm`/`ignis` — one server at a time), `require_files <dir> <file>…` (missing → die with the
  README's exact download command for that model), `check_gpu` (`nvidia-smi -i "$GPU"
  --query-gpu=name` must contain `RTX PRO 6000`: the profile is pinned to that card),
  `ignis_launch` (below, filled in T2). Every function ends in `return 0`.
- `models` → `/mnt/noir/scratch/ai/llm/models` (the siblings' tree). Artifacts live flat in
  `models/ignis/` (created by the owner's download, T4).
- `.gitignore`: add `/storage/` (the cache directory, T2).
- `README.md`: what this is; prerequisites (docker as yourself, GPU 0 = PRO 6000, ~37 GB free RAM
  for Flash-Next's n-gram table, one server on `:8700`: `docker stop vllm` first); **Download the
  weights** — both forms, pinned revisions, all four files for Flash-Next and both files for the
  27B (`hf download <repo> --revision <rev> --include '*.ninfer' '*.json' --local-dir
  models/ignis`, and the container form `docker run --rm --user 1000:1000 -v
  "$PWD/models/ignis:/models" "$IGNIS_IMAGE" model download qwen3.8-flash-next --out /models`),
  sizes, licenses (Qwen Community License 1.0 for Flash-Next; apache-2.0 for the 27B; the
  abliterated one does not refuse), and "agents never run these"; the run commands (T2/T3 fill);
  the one-served-name note (only `cosmo-6000`; the other vLLM names 404).
- `context/distilled.md`: seed the confirmed Key Facts that later work relies on (served id, mock
  trap, profile table, flags, artifact files), each tagged `[SOURCE]`/`[DOC]` with `path:line` @
  v0.8.0.

**Before**:
```bash
$ ls versions.env lib/common.sh README.md models
ls: cannot access 'versions.env': No such file or directory
ls: cannot access 'lib/common.sh': No such file or directory
ls: cannot access 'README.md': No such file or directory
ls: cannot access 'models': No such file or directory
```

**After**: the four exist; `lib/common.sh` is the only tracked bash file.

**Verify**:
```bash
DOCKER='sudo docker' bash -c 'source lib/common.sh' ; echo "rc=$?"   # → "## DOCKER names 'sudo'…", rc=1
bash -c 'source lib/common.sh && load_pins && echo "$IGNIS_IMAGE"'    # → ghcr.io/gpillon/ignis:0.8.0@sha256:83d0…
grep -c 'hf download' README.md                                        # → ≥ 3 (Flash-Next, 27B, abliterated)
readlink models                                                        # → /mnt/noir/scratch/ai/llm/models
bash -n lib/common.sh && shellcheck -x lib/common.sh   # + project gate steps 1–3 (RULES.md § Project gate)
```
Expected: the sudo refusal fires; pins load; gate step 1 counts `1 b`; step 3 `env ok`. Update
RULES gate step 1's **Expected counts** to `1 b` in this commit.

**Commit**: `[ignis][P1][T1][pins] add ignis 0.8.0 pins, guards, README download steps` — *commit
before marking done (Rule B), tracker row in the same commit.*

### T2: `qwen38fn/run` + launcher dry-run (gate step 4)

**File**: `qwen38fn/run` *(new)*, `lib/common.sh` (`ignis_launch`), `scripts/launcher-dryrun`
*(new)*, `context/plans/RULES.md` (gate step 4 made exact; counts), `README.md`

**What to do**: `qwen38fn/run` sources `lib/common.sh`, loads pins, then:

- Env knobs (defaults): `PORT=8700`, `SERVED=cosmo-6000`, `GPU=0`,
  `MODELS_DIR=$ROOT/models/ignis`, `CACHE_DIR=$ROOT/storage/cache`, `CONTEXT=262144`,
  `IDLE_UNLOAD=0`, `SPEC_TOKENS=` (empty = the profile's 3), `LANES=` (empty = the profile's 8),
  `DETACH=` (`-d` when set, `-it` otherwise, like vllmcustom), `PRINT_CONFIG=` (1 = run
  `config print --format yaml` with the same field flags instead of serving — no model load),
  `EXTRA_ARGS=` (appended **last**, last-wins).
- Refusals before any `docker run`: `refuse_busy "$PORT" vllm ignis`, `check_gpu`,
  `require_files "$MODELS_DIR" "$FN_ARTIFACT" "$FN_ARTIFACT.conversion.json" "$FN_MTP"
  "$FN_MTP.conversion.json"`, `owned_dir "$CACHE_DIR"/{ngram,kv-disk}`.
- Docker args (`ignis_launch`): `run -it|-d --rm --name ignis --user 1000:1000 -e HOME=/cache
  --gpus device=$GPU -e CUDA_DEVICE_ORDER=PCI_BUS_ID -p 0.0.0.0:$PORT:8000
  -v $MODELS_DIR:/models:ro -v $CACHE_DIR:/cache --stop-timeout 60` (no `--init`, no `--memory`).
- Ignis args: `--profile rtxpro6000 --model-artifact /models/$FN_ARTIFACT --model-id $SERVED
  --download-enabled false --switch-allow-implicit false --model-idle-unload $IDLE_UNLOAD
  --model-max-context $CONTEXT --ngram-persist-path /cache/ngram --kv-disk-path /cache/kv-disk`,
  plus `--qwen38flashnext-spec-draft-tokens $SPEC_TOKENS` / the lanes flag only when set (exact
  per-model flag names confirmed against `docs/user/reference/configuration.md` at v0.8.0 in this
  task), then `EXTRA_ARGS`.
- Echo the full command (`## docker run …`) before running it — it is the provenance line P1.3
  asks for.

`scripts/launcher-dryrun`: makes a `mktemp -d` sandbox with stub `docker` (records argv; `version`
→ 0; `ps` → empty unless `STUB_BUSY=1`) and stub `nvidia-smi` (prints the PRO 6000 name, or the
4060's when `STUB_GPU=4060`), fake empty artifact files under a sandbox `MODELS_DIR`, a sandbox
`CACHE_DIR`, runs the launcher with `PATH` pointed at the stubs, asserts the recorded argv, runs
the negative arms (sudo, busy, wrong GPU, missing artifact, `PRINT_CONFIG=1` → `config print` and
no `-p`), removes the sandbox (`trap … EXIT`), prints `dryrun ok`. It never creates `storage/`.

**Before**:
```bash
$ ls qwen38fn/run scripts/launcher-dryrun
ls: cannot access 'qwen38fn/run': No such file or directory
ls: cannot access 'scripts/launcher-dryrun': No such file or directory
```

**After**: RULES gate step 4 becomes a fenced `./scripts/launcher-dryrun` with pass = last line
`dryrun ok`; step 1 **Expected counts** `3 b`.

**Verify**:
```bash
./scripts/launcher-dryrun qwen38fn   # → per-assertion "ok …" lines, then "dryrun ok"
#   asserts: -p 0.0.0.0:8700:8000, --gpus device=0, --user 1000:1000, /models:ro,
#   --profile rtxpro6000, --model-artifact /models/qwen3_8_flash_next_trellis_a25-v2.ninfer,
#   --model-id cosmo-6000, --download-enabled false, --switch-allow-implicit false,
#   --model-idle-unload 0, --model-max-context 262144, EXTRA_ARGS last;
#   refusals: DOCKER=sudo, STUB_BUSY=1, STUB_GPU=4060, missing MTP file → exit 1 with the message
git status --short --ignored .      # → only this task's own edits; no storage/, no sandbox leftovers
# + project gate steps 1–4 (RULES.md § Project gate)
```
Expected: `dryrun ok`; the harness left no file in the tree.

**Commit**: `[ignis][P1][T2][run] add qwen38fn/run for Flash-Next on :8700` — *commit before
marking done (Rule B), tracker row in the same commit.*

### T3: `qwen38-27b/run`

**File**: `qwen38-27b/run` *(new)*, `scripts/launcher-dryrun`, `context/plans/RULES.md` (counts),
`README.md`

**What to do**: same shape through `ignis_launch`. `ABLITERATED=1` swaps
`Q27A_ARTIFACT` (+ `.graft.json`) for `Q27_ARTIFACT`; `SERVED` stays `cosmo-6000`.
`require_files` checks the artifact and its `.graft.json` (the model card's own command fetches
only the `.ninfer`, and the load is refused without the graft — research agent, 2026-10-27).
DFlash2 7 and 8 lanes come from the profile; `SPEC_TOKENS` maps to
`--qwen38-spec-draft-tokens`. No KV-disk on the 27B by default (`configuration.md` kv_disk.bytes),
so only `/cache/ngram` is needed — keep the `--kv-disk-path` flag harmless or drop it (decide from
the docs in this task).

**Before**: `ls qwen38-27b/run` → `No such file or directory`.

**After**: `./scripts/launcher-dryrun` (no argument) runs both launchers' arms; counts `4 b`.

**Verify**:
```bash
./scripts/launcher-dryrun qwen38-27b               # → … "dryrun ok"; artifact qwen3_8_27b_nvfp4full-v2.ninfer
ABLITERATED=1 ./scripts/launcher-dryrun qwen38-27b # → artifact …-huihui-abliterated.ninfer, model-id still cosmo-6000
./scripts/launcher-dryrun                          # → both, "dryrun ok"
# + project gate steps 1–4 (RULES.md § Project gate)
```

**Commit**: `[ignis][P1][T3][run] add qwen38-27b/run for the 27B on :8700` — *commit before
marking done (Rule B), tracker row in the same commit.*

### T4: Flash-Next first boot and smoke (owner runs; agent records)

**File**: tracker Evidence, `context/distilled.md`, `context/pitfalls.md` (if anything bites),
`README.md` (verified status)

**What to do**: the owner, in order: (1) `docker pull` the pinned image and confirm
`docker image inspect --format '{{index .RepoDigests 0}}'` ends in the pinned digest;
(2) `PRINT_CONFIG=1 ./qwen38fn/run` and `PRINT_CONFIG=1 ./qwen38-27b/run` — every flag accepted,
the printed config shows `profile rtxpro6000`, MTP 3 / 8 lanes, `max_context 262144`; (3) the
README's Flash-Next download; (4) `docker stop vllm`, `./qwen38fn/run`; (5) the smoke set below.
The agent records the outputs, the boot log's plan lines (expert residency, KV pool, host plan,
load time from NFS) and settles the two unconfirmed Key Facts.

**Verify** (owner, server up):
```bash
docker ps --filter publish=8700 --format '{{.Names}} {{.Image}}'            # → ignis ghcr.io/gpillon/ignis…
curl -fsS localhost:8700/ready && echo                                       # → 200 once loaded
curl -sS localhost:8700/v1/models | python3 -c 'import sys,json; print([m["id"] for m in json.load(sys.stdin)["data"]])'   # → ['cosmo-6000']
curl -sS localhost:8700/v1/chat/completions -H 'Content-Type: application/json' \
  -d '{"model":"cosmo-6000","messages":[{"role":"user","content":"Say hi"}],"max_tokens":64}'   # → a reply, reasoning_content present
curl -sS -o /dev/null -w '%{http_code}\n' localhost:8700/v1/chat/completions -H 'Content-Type: application/json' \
  -d '{"model":"claude-heavy","messages":[{"role":"user","content":"x"}],"max_tokens":8}'      # → 404 (expected, Key Facts)
# one tool-call request (a `tools` array) → finish_reason "tool_calls"; one stream with stream_options.include_usage → a usage chunk
# one request through llama-proxy (the client path the owner uses) → answered
```

**Expected observation**:
- **Confirms if**: boot reaches `/ready`, the config shows the profile + our overrides, the boot
  log plans every expert in VRAM, the four API checks behave as stated, the container writes only
  under `storage/cache`.
- **Refutes if**: the start refuses a flag, falls back to the mock (a WARN naming the placeholder),
  fails on a write under `/home/ignis` (then `--user 1000:1000` is wrong — record it, pick a
  fix in a revision), or `include_usage` is refused (then T6 needs a different client).
- **Noise**: load time varies with the NFS page cache — record cold vs warm separately.

**Commit**: `[ignis][P1][T4][live] record Flash-Next first boot and smoke on :8700`

### T5: 27B first boot and smoke (owner runs; agent records)

**File**: tracker Evidence, `context/distilled.md`

**What to do**: the README's 27B download; `./qwen38-27b/run`; the T4 smoke set (DFlash2 7 in
the config print; the boot log's weights ~18.6 GB and KV pool). `ABLITERATED=1` only if the owner
downloads that artifact — otherwise the row is `skipped` with that reason.

**Verify**: as T4, with `qwen38-27b`; `/v1/models` → `['cosmo-6000']`.

**Expected observation**: confirms if it serves with DFlash2 7 and 8 lanes at 262,144 context;
refutes if the graft is not found beside the artifact or the profile is not applied.

**Commit**: `[ignis][P1][T5][live] record 27B first boot and smoke on :8700`

### T6: Same-session bench — Ignis vs vLLM FP4-Local (owner runs; agent records)

**File**: tracker Evidence, `captures/` (gitignored), `context/distilled.md`, `README.md`
(measured status line)

**What to do**: one session, environment checked first (P1.5: no other GPU user, clocks/power
default, warm-up pass unrecorded). Change one thing: the engine.
1. vLLM: `../vllmcustom/qwen3.8-flash-next/run-fp4local` (its own defaults), wait for `/health`,
   one unrecorded `../vllmcustom/bench/bench warm --out "captures/warm_$(date +%F).md"`, then 3
   recorded passes: `../vllmcustom/bench/bench vllm-fp4local --out "captures/bench_$(date +%F)_engine.md"`;
   then `../vllmcustom/bench/conc vllm-c$N --streams $N --prompt code --rounds 3 >> "captures/conc_$(date +%F)_engine.jsonl"`
   for N = 1, 2, 4. (`--out` always: without it `bench/bench` appends to vllmcustom's tracked
   `bench-results.md`.)
2. Stop it; `./qwen38fn/run`; the same warm-up and the same commands labelled `ignis-fn`.
3. Optional, same shape: `./qwen38-27b/run` labelled `ignis-27b` (a different model — reported
   beside, never as a Flash-Next comparison).
Record mean and spread per prompt (short/code/long) and per stream count, the image digests, the
artifact revision, the launchers' echoed `docker run` lines.

**Verify**: the Evidence table has ≥ 3 runs per cell for both engines with spread; each capture
file named in its row.

**Expected observation** (written 2026-10-27, before any run):
- **Confirms if** Ignis single-stream code decode beats FP4-Local's same-session mean by more
  than both spreads (upstream's 258 on a Server Edition vs FP4-Local's 195.5 suggests ~1.3×).
- **Refutes if** Ignis is at or below FP4-Local on code at one stream — then record it, and the
  first suspects are sampling (bench sends temperature 0.6; upstream measured greedy, and MTP
  acceptance fell 0.80 → 0.75–0.77 sampled, RF:86-90), the loop guard (upstream benched `off`),
  and NFS-backed n-gram reads.
- **Within noise if** the means differ by less than the larger spread — write "within noise".

**Commit**: `[ignis][P1][T6][bench] record Ignis vs vLLM FP4-Local, same session`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `versions.env` *(new)* | image tag + digest, source ref, HF repos/revisions, artifact names | P0 |
| `lib/common.sh` *(new)* | guards, refusals, `ignis_launch` | P0 |
| `qwen38fn/run` *(new)* | Flash-Next launcher | P0 |
| `qwen38-27b/run` *(new)* | 27B launcher (`ABLITERATED=1`) | P0 |
| `scripts/launcher-dryrun` *(new)* | stub-docker harness = gate step 4 | P0 |
| `README.md` *(new)* | download how-to, run commands, served-name note, measured status | P0 |
| `models` *(new symlink)* | → `/mnt/noir/scratch/ai/llm/models` | P1 |
| `.gitignore` | `/storage/` | P1 |
| `context/plans/RULES.md` | gate step 4 exact; step 1 expected counts | P1 |
| `context/distilled.md`, `context/pitfalls.md` | seeded facts; whatever bites | P1 |

---

## Open Questions / Risks

1. **Is the effect above noise?** FP4-Local's code spread is 188.5–210.6 (4 runs); upstream's
   gap suggests ~60 tok/s. *How we'll settle it*: T6, N ≥ 3 per cell, same session.
2. **Does it survive a restart?** The n-gram cache persists under `storage/cache/ngram`, so the
   first boot and later boots differ. *Mitigation*: T4 records cold vs warm load; T6 runs after a
   warm boot and says so.
3. **Risk: clients that use the other four vLLM names break.** Ignis serves one id; `llama-proxy`
   only sends `cosmo-6000` (`../proxy/config.yaml:9-10,30-31`), but anything hitting `:8700`
   directly with `claude-heavy` etc. gets a 404. *Mitigation*: README says so; T4 checks the proxy
   path; a name-rewriting shim is out of scope (Follow-up if the owner hits it).
4. **Risk: `--user 1000:1000` vs the image's uid 10001.** *Mitigation*: every writable path is
   explicit and `HOME=/cache`; T4 is the test. If it fails, the fallback is a cache dir uid 10001
   can write — which needs the owner (no sudo here) — recorded as a revision.
5. **Risk: NFS load time.** ~35 GB of each load bypasses the page cache (RF:438-445, research
   agent), and upstream's 124 s cold load was at 1,000 MiB/s. *Mitigation*: idle unload is off;
   `MODELS_DIR` can point at local NVMe (`/home`, ~319 GB free per ninfer's SERIES) if T4's load
   time is bad — measuring that is a Follow-up, not this plan.
6. **Question: Workstation vs Server Edition.** Upstream measured a Server Edition; ours is the
   Workstation Edition (same 97,887 MiB). *How we'll settle it*: T6 measures ours; never cite
   upstream's numbers as ours (P1).
7. **Risk: the pin ages.** `main` is at 0.9.0 with 260 commits. *Mitigation*: a bump is a plan
   (RULES plan-gate); this plan records what 0.8.0 does.
8. **Question: is `config print` happy without a GPU device mapped?** *How we'll settle it*: T4
   step 2 runs it with the launcher's `--gpus` anyway; if it needs NVML even with `--profile`
   named, record it.

---

## Verification Checklist

- [x] T1: `DOCKER='sudo docker' bash -c 'source lib/common.sh'` → refusal, rc 1; `load_pins` prints the digest-pinned image; README has the three `hf download` commands; gate counts `1 b`, `env ok`
- [x] T2: `./scripts/launcher-dryrun qwen38fn` → `dryrun ok` with every listed flag asserted and every refusal arm exiting 1; sandbox leaves nothing behind
- [x] T3: `./scripts/launcher-dryrun` → `dryrun ok` for both launchers, abliterated arm included
- [ ] T4: pulled image digest == pin; both `PRINT_CONFIG=1` runs accepted; `/v1/models` → `['cosmo-6000']`; chat, tool call, include_usage, 404 for `claude-heavy`, proxy path all as stated — outputs pasted in Evidence
- [ ] T5: 27B `/v1/models` → `['cosmo-6000']` and a chat reply; DFlash2 7 in the config
- [ ] T6: Evidence holds ≥ 3 runs per cell for vLLM FP4-Local and Ignis, same session, with spread and capture names; the verdict uses "within noise" where it applies
- [ ] All: the project gate (Rule A) passes on the final commit, every applicable step `pass`
- [ ] All: the runtime tier ran (owner), **or** the tracker says the behavior went unverified at runtime
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
