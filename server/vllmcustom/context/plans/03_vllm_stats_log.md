# Plan 03 — vLLM boot stats JSONL log

> **Status**: in-progress
> **Created**: 2026-10-16
> **Depends on**: N/A
> **Goal**: Persist the vLLM post-startup stats report as one JSONL line per boot in `./storage`, with the model name and VLLM serve args as top-level fields, append-only for historical reference.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Make the existing boot-timing report (`container/patches/boot-timing/boot_timing.py`) append a structured JSON line per boot to a jsonl file under `./storage`, including the model name and the full VLLM serve argument list.

**Deliverables**:
1. `container/patches/boot-timing/boot_timing.py` gains a `write_jsonl()` path that serializes the parsed report (durations, GPU memory, host RAM, headroom, notes) plus top-level `model` and `vllm_args` fields, appended to `$VLLM_BOOT_LOG_DIR/vllm_boot_stats.jsonl` — gated behind `VLLM_BOOT_LOG_DIR` being set.
2. `common/_run-lib.sh` creates `./storage` (when `VLLM_BOOT_LOG=1`), mounts it into the container at `/opt/vllm-storage`, and exports `VLLM_BOOT_LOG_DIR=/opt/vllm-storage`.
3. `qwen3.8-flash-next/run` opts in (`VLLM_BOOT_LOG=1`) and passes `VLLM_MODEL_NAME` and `VLLM_BOOT_ARGS` (a JSON-encoded key→value dict of the resolved serve arguments) into the container.
4. Example JSONL object documented in this plan's Context section.

**Estimated effort**: Small–Medium (half day)

---

## Scope

**In**: `container/patches/boot-timing/boot_timing.py`, `common/_run-lib.sh`, `qwen3.8-flash-next/run`, this plan + tracker, `SERIES.md`.
**Out**: other launchers (they can opt in later by setting `VLLM_BOOT_LOG=1` and passing the two env vars), reading existing `/tmp` marks, changing the text report format, any runtime/GPU-dependent verification.

---

## Context

### Pre-Identified Bug/Issue

Today the `v L L M   P O S T - S T A R T U P   S T A T S` report is **stdout only** and the raw marks live in a per-container `/tmp` dir (`boot_timing.py:23,51-52,599,703`). Once `docker run --rm` exits, both are gone. There is no historical record of boot stats, startup args, or which model was served.

Reproduction (today's behavior):

```bash
$ ./qwen3.8-flash-next/run
## ... boot text report prints to terminal, then container exits on --rm
$ ls /tmp/vllm-boot-timing/   # host-side: nothing; the dir lived inside the container
```

For a feature: the command showing today's behavior is the launcher's echoed `docker … serve …` line (`_run-lib.sh:381`) — the report is never captured to a file.

### Why [Approach]

Extend `boot_timing.py`'s pure render path (`format_report`, `boot_timing.py:585`) with a JSON serialization that reuses the same parsed `Row`/`Span` structures, then append one line to a jsonl file. The launcher already knows the model name (`$MODEL`) and the full `VLLM_ARGS` array (`_run-lib.sh:306-338`, `qwen3.8-flash-next/run:660-694`), so it passes them in as env vars. This avoids re-parsing the text report and keeps the writer inside the module that already owns the data.

Alternative considered and rejected: capture stdout on the host in the launcher script and parse the text report. Rejected because the text format is free-form and fragile to parse; the structured data already exists in-process in `boot_timing.py`.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| Boot marks file (per-container) | `$VLLM_BOOT_TIMING_DIR/marks.log`, default `/tmp/vllm-boot-timing/marks.log` | `boot_timing.py:51-52,719-733` |
| Report string | `v L L M   P O S T - S T A R T U P   S T A T S` centered in the report | `boot_timing.py:599` |
| Report trigger | uvicorn `Server.startup` callback `_report_now` | `boot_timing.py:917-926,945-946` |
| Report is stdout only, never written to disk | `format_report` returns a string; `_report_now` prints it | `boot_timing.py:585-705,917-926` |
| Container is `--rm`, marks dir is per-container `/tmp` | `docker_core_args` uses `--rm`; `_DIR` defaults to `/tmp/vllm-boot-timing` | `_run-lib.sh:243`, `boot_timing.py:51` |
| Boot-timing is mounted only by `qwen3.8-flash-next/run` | `BOOT_TIMING_DIR` mount + `PYTHONPATH=/opt/boot-timing` | `qwen3.8-flash-next/run:610-615` |
| Launcher knows `$MODEL` and full `VLLM_ARGS` before `docker run` | `vllm_core_args` builds `VLLM_ARGS`; launcher extends it; `vllm_launch` echoes then runs. `VLLM_BOOT_ARGS` passed as JSON-encoded key→value dict. | `_run-lib.sh:306-382` |
| `VLLM_BOOT_TIMING` env defaults to `1` in `qwen3.8-flash-next/run` | `BOOT_TIMING=${BOOT_TIMING:-1}` | `qwen3.8-flash-next/run:254` |
| Project gate: no compiler/CI; syntax + patch tests + preflight + image label checks | `RULES.md` § Project gate, steps 1-5 | `RULES.md:118-243` |
| `shellcheck` not installed → gate step 2 is `skip` | `RULES.md:172` | `RULES.md:170-180` |

### Prior Measurements

None for this feature — it is new. Existing boot-timing behavior was confirmed by reading source (above).

### Rejected Claims

None identified.

### Example JSONL object

One line appended to `./storage/vllm_boot_stats.jsonl` per boot:

```json
{
  "ts": "2026-10-16T14:22:10.123456Z",
  "model": "Qwen3.8-Flash-Next-mixed-NVFP4-FP8",
  "vllm_args": {
    "gpu-memory-utilization": "0.90",
    "max-model-len": "32768",
    "max-num-seqs": "256",
    "attention-backend": "flashinfer",
    "enable-prefix-caching": true,
    "enable-chunked-prefill": true,
    "enable-per-request-metrics": true,
    "dtype": "auto",
    "tensor-parallel-size": "1",
    "pipeline-parallel-size": "1",
    "async-scheduling": true,
    "trust-remote-code": true,
    "host": "0.0.0.0",
    "port": "8799",
    "engram-config": "{\"cpu_offload\": true}",
    "speculative-config": "{\"method\":\"mtp\",\"num_speculative_tokens\":2}"
  },
  "boot": {
    "total_seconds": 171.5,
    "total_formatted": "2m51s",
    "processes": [
      {
        "pid": 1234,
        "role": "api",
        "name": "APIServer (pid 1234)",
        "wall_seconds": 171.0,
        "spans": [
          {"name":"worker.load","label":"WEIGHT LOAD (model build + shards)","offset":"+1m03s","duration_seconds":138.2,"duration_formatted":"2m18s","rate":"182 MiB/s (77.2 GiB)"},
          {"name":"worker.warmup","label":"warmup: kernels + autotune + capture","offset":"+2m41s","duration_seconds":22.1,"duration_formatted":"22s"}
        ]
      }
    ],
    "gpu_memory": {
      "total_gib": 47.9, "budget_gib": 43.1, "util": 0.90,
      "weights_gib": 24.3, "kv_cache_gib": 12.7, "graphs_gib": 2.1,
      "context_gib": 3.4, "overhead_gib": 1.2, "outside_budget_gib": 1.5,
      "final_used_gib": 45.2, "headroom_gib": 0.0
    },
    "host_ram": {
      "consumed_by_boot_gib": 2.1, "engine_rss_gib": 3.4, "api_rss_gib": 0.8,
      "mem_available_gib": 110.2, "mem_total_gib": 125.0
    },
    "headroom": {
      "current_util": 0.90, "safe_util": 0.78, "freed_gib": 5.7,
      "kv_at_safe_gib": 7.0, "hard_floor": 0.71, "trade_rate_gib_per_001": 0.48,
      "floor_build_up_gib": 43.1
    },
    "notes": [
      "weight load = 81% of total boot (cold NFS read; page cache helps the next boot)",
      "flashinfer autotune result is cached under /root/.cache/vllm (~free on later boots with the same build)"
    ],
    "raw_marks_path": "/tmp/vllm-boot-timing/marks.log"
  }
}
```


---

## Step-by-Step Tasks

### T1: Grounding & key facts (this plan's Context section)

**File**: `context/plans/03_vllm_stats_log.md` (this file)

**What to do**: Confirm the key facts above by re-reading `boot_timing.py`, `_run-lib.sh`, and `qwen3.8-flash-next/run`. Record the exact line numbers where the report is rendered (`format_report` at `boot_timing.py:585`), where marks are written (`_record` at `boot_timing.py:719`), and where the launcher echoes the serve command (`vllm_launch` at `_run-lib.sh:381`).

**Before**: N/A (research task)

**After**: N/A (research task)

**Verify**: `grep -n "format_report\|_record\|vllm_launch\|VLLM_BOOT_TIMING\|BOOT_TIMING_DIR" container/patches/boot-timing/boot_timing.py common/_run-lib.sh qwen3.8-flash-next/run` → line numbers match the Key Facts table.

**Expected observation**: All cited line numbers exist and match; no fact in the table is contradicted by source.

**Commit**: `[P3][T1][context] record boot-timing and launcher facts for stats log plan`

---

### T2: Design the JSONL schema and storage layout

**File**: `context/plans/03_vllm_stats_log.md` (this file, Context section), plus design notes in tracker

**What to do**: Define the exact JSON object shape, field names, types, and the storage filename (`vllm_boot_stats.jsonl` under `./storage`). Document the schema with a complete example JSONL object (see Context below). Decide: storage dir default `./storage` relative to launcher cwd, mounted to `/opt/vllm-storage` in-container, env var `VLLM_BOOT_LOG_DIR` overrides.

**Before**: No schema exists.

**After**: Schema defined in this plan; example JSONL object recorded.

**Verify**: The example JSON object in this plan parses as valid JSON and contains `model`, `vllm_args`, `ts`, and `boot` sub-object with `total_seconds`, `gpu_memory`, `host_ram`, `headroom`, `notes`.

**Expected observation**: Example JSON is valid, self-describing, and covers every metric the text report currently shows.

**Commit**: `[P3][T2][schema] define JSONL schema and storage layout for boot stats log`

---

### T3: Add JSONL writer to boot_timing.py

**File**: `container/patches/boot-timing/boot_timing.py`

**What to do**: Add a `write_jsonl(report_rows, now, model_name, vllm_args, log_dir)` function that:
- Serializes the parsed report (total duration, per-process spans, GPU memory, host RAM, headroom, notes) to a dict
- Adds top-level `ts` (ISO-8601), `model`, `vllm_args` (dict of key→value; boolean flags map to `true`)
- Appends one compact JSON line to `<log_dir>/vllm_boot_stats.jsonl` (create dir if missing, open with `O_APPEND`)
- Is a no-op when `VLLM_BOOT_LOG_DIR` env var is unset
- Never raises (wrap in try/except; degrade silently like the rest of the module)
- Is called from `_report_now` after the text report is printed

**Before**: `_report_now` (`boot_timing.py:917`) only prints the text report to stdout; no JSONL path exists.

**After**: `_report_now` also calls `write_jsonl(...)` when `VLLM_BOOT_LOG_DIR` is set, passing `VLLM_MODEL_NAME` and `VLLM_BOOT_ARGS` from env.

**Verify**: `PYTHONDONTWRITEBYTECODE=1 python3 container/patches/boot-timing/test_boot_timing.py` → all 4 patch tests pass (gate step 3). Additionally, a small host-side check: import `boot_timing` with `PYTHONPATH` pointing at the patch dir, call `format_report` on synthetic rows, then call `write_jsonl` to a temp dir and confirm one valid JSONL line appears with `model` and `vllm_args` present.

**Expected observation**: Patch tests still pass (4/4 `ok`). JSONL file is created, append-only, one line per call, valid JSON, top-level `model` and `vllm_args` fields present.

**Commit**: `[P3][T3][boot-timing] add JSONL writer for post-startup stats log`

---

### T4: Wire the launcher to mount storage and pass model + args

**Files**: `common/_run-lib.sh`, `qwen3.8-flash-next/run`

**What to do**:
- In `_run-lib.sh` `docker_core_args()`: when `VLLM_BOOT_LOG=1`, create `./storage` (mkdir -m 0750, owned by RUN_UID), mount it to `/opt/vllm-storage:rw`, add `-e VLLM_BOOT_LOG_DIR=/opt/vllm-storage`.
- In `qwen3.8-flash-next/run`: set `VLLM_BOOT_LOG=1` (it already defaults `BOOT_TIMING=1`), add `-e VLLM_MODEL_NAME="$MODEL"`, add `-e VLLM_BOOT_ARGS="$(python3 -c 'import json,sys; print(json.dumps(dict(sys.argv[1:])))' "${VLLM_ARGS[@]}")"` — or a helper that converts the bash arg array into a JSON object of key→value, with bare flags (no `=` value) mapping to `true`.
- Storage mount goes after the cache mount in `docker_core_args` (order doesn't matter functionally).

**Before**: `docker_core_args` has no storage mount; `qwen3.8-flash-next/run` sets `BOOT_TIMING=1` but no `VLLM_MODEL_NAME`/`VLLM_BOOT_ARGS`.

**After**: `./storage` is created and mounted; `VLLM_BOOT_LOG_DIR`, `VLLM_MODEL_NAME`, `VLLM_BOOT_ARGS` are set in the container when `VLLM_BOOT_LOG=1`.

**Verify**: `bash -n common/_run-lib.sh && bash -n qwen3.8-flash-next/run` → rc 0 (gate step 1). Host-side dry-run: `DOCKER=docker PATH=/tmp/gate7/stub:$PATH CACHE_DIR=/tmp/gate7/cache MODEL_ROOT=/tmp/gate7/model ./qwen3.8-flash-next/run --dry-run` (if supported) or inspect the echoed `docker … serve …` line for `-v ... ./storage ... -e VLLM_BOOT_LOG_DIR=... -e VLLM_MODEL_NAME=... -e VLLM_BOOT_ARGS=...`.

**Expected observation**: The echoed docker command includes the storage mount and the three env vars; `VLLM_BOOT_ARGS` is a JSON-encoded key→value dict; `./storage` is created on first run.

**Commit**: `[P3][T4][launcher] mount ./storage and pass model name + VLLM args to container`

---

### T5: Verify end-to-end with the project gate

**File**: all touched files + `SERIES.md`

**What to do**: Run the full project gate (Rule A steps 1-5) on the final state:
1. Syntax check over tracked files (counts must stay correct: 27 bash / 24 python after adding nothing new, or bumped if a file is added)
2. shellcheck → `skip` (not installed)
3. Patch tests: `ls container/patches/*/test_*.py | wc -l` → 4, each `ok`
4. Preflight read-only for both pin sets → `## preflight OK`, exit 0
5. Image label check (cheap lines only; `container/archs` is per-build)

Also confirm `VLLM_BOOT_LOG_DIR` write path is guarded (no-op when unset) and append-only behavior works.

**Before**: Gate baseline is green (per RULES.md:469-474).

**After**: Gate still green; no new failures introduced.

**Verify**: Gate steps 1-5 all report `pass` or `skip` (step 2 is `skip`, expected).

**Expected observation**: `FAIL` count = 0; step 2 reported as `skip`; patch tests 4/4 `ok`; preflight `## preflight OK` for both sets.

**Commit**: `[P3][T5][gate] verify boot-timing JSONL writer passes project gate`

---

### T6: Commit plan + tracker + SERIES

**Files**: `context/plans/03_vllm_stats_log.md`, `context/plans/03_vllm_stats_log_tracker.md`, `context/plans/SERIES.md`

**What to do**: Commit the plan, tracker, and SERIES update together (Rule B, tracker row in same commit).

**Before**: Plan 03 does not exist; SERIES next free number is 03.

**After**: Plan 03 + tracker committed; SERIES has a `pending` row for Plan 03; next free number bumped to 04.

**Verify**: `git log --oneline -1` shows the commit with format `[P3][plan] add vLLM boot stats JSONL log`; `SERIES.md` next free = 04.

**Expected observation**: Commit exists, SERIES updated, plan ready for `resume`.

**Commit**: `[P3][plan] add vLLM boot stats JSONL log plan`

---

## Critical Files

| File | Change | Priority |
|---|---|---|
| `container/patches/boot-timing/boot_timing.py` | Add `write_jsonl()` + call from `_report_now`; read `VLLM_MODEL_NAME`/`VLLM_BOOT_ARGS`/`VLLM_BOOT_LOG_DIR` from env | P0 |
| `common/_run-lib.sh` | `docker_core_args`: create `./storage`, mount to `/opt/vllm-storage`, set `VLLM_BOOT_LOG_DIR` when `VLLM_BOOT_LOG=1` | P0 |
| `qwen3.8-flash-next/run` | Set `VLLM_BOOT_LOG=1`, `VLLM_MODEL_NAME="$MODEL"`, `VLLM_BOOT_ARGS` as JSON-encoded key→value dict from `VLLM_ARGS` | P0 |
| `context/plans/SERIES.md` | Add Plan 03 row (`pending`), bump next free to 04 | P1 |

---

## Open Questions / Risks

1. **`VLLM_BOOT_ARGS` as dict, not array**: bash array → JSON object key→value. Bare flags (e.g. `--async-scheduling`, no `=` value) map to `true`; args with `=` split at the first `=`. *Mitigation*: write a small bash→JSON helper in the launcher. *How we'll settle it*: decide in T3/T4, record in tracker.
2. **Storage dir ownership**: `./storage` must be owned by RUN_UID (1000) like the cache dir. *Mitigation*: mirror the cache-dir ownership assertion pattern from `_run-lib.sh:200-208`. *How we'll settle it*: T4 asserts ownership, same as cache.
3. **Only qwen3.8-flash-next mounts boot-timing today**: jsonl logging works only for that launcher unless others also mount the patch dir. *Mitigation*: note in plan; other launchers opt in by setting `VLLM_BOOT_LOG=1` and mounting boot-timing. *How we'll settle it*: out of scope for this plan; documented as a follow-up.
4. **Append concurrency**: if two containers boot simultaneously writing to the same jsonl, lines could interleave. *Mitigation*: single-server-by-design (README: one model at a time, same name/port). Not a real risk here.

---

## Verification Checklist

- [ ] T1: Key Facts table line numbers match `boot_timing.py`, `_run-lib.sh`, `qwen3.8-flash-next/run` source.
- [ ] T2: Example JSONL object in this plan is valid JSON; contains `model`, `vllm_args` (dict key→value, not array), `ts`, `boot.total_seconds`, `boot.gpu_memory`, `boot.host_ram`, `boot.headroom`, `boot.notes`.
- [ ] T3: `container/patches/boot-timing/test_boot_timing.py` → 4/4 `ok`; `write_jsonl` creates `vllm_boot_stats.jsonl`, appends one valid JSON line per call, no-ops when `VLLM_BOOT_LOG_DIR` unset.
- [ ] T4: `bash -n` passes on both touched scripts; echoed `docker … serve …` includes storage mount + `VLLM_BOOT_LOG_DIR`/`VLLM_MODEL_NAME`/`VLLM_BOOT_ARGS`.
- [ ] T5: Project gate steps 1-5 all `pass`/`skip` (step 2 `skip` expected), zero `FAIL` lines; counts 27 bash / 24 python unchanged.
- [ ] All: findings harvested into `context/` (Rule D) — distilled.md entry for the jsonl schema.
- [ ] All: plan + tracker committed with SERIES row; next free = 04 (Rule C).

---
