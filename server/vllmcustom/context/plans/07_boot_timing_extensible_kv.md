# Plan 07 — boot-timing: account for vLLM's extensible KV cache

> **Status**: pending
> **Created**: 2026-11-03
> **Depends on**: N/A (Plan 03 shipped the report this fixes)
> **Goal**: The boot report's KV row, outside-budget line, headroom section and KV cross-check show the KV cache vLLM actually committed, on images with the extensible KV cache on (vllm#56492, default from `ae525e9`).
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Hook `Worker.extend_kv_cache` in `container/patches/boot-timing/boot_timing.py`. On exit it records the committed KV bytes and a whole-device + host memory sample (`mem.kvext`). The report uses that sample when it exists, and holds back the KV-derived rows when it can't account for the KV.

**Deliverables**:
1. A new optional hook target, `vllm.v1.worker.gpu_worker.Worker.extend_kv_cache` → span `worker.kv_extend`, mem mark `mem.kvext` (`kv_bytes`, `blocks`, plus the usual device/host fields).
2. Report changes:
   - KV = `kv_bytes`;
   - outside-budget subtracts it, because the extensible KV cache is mapped outside torch's allocator;
   - final device/host numbers come from `mem.kvext`;
   - the KV cross-check reads "sized after warmup";
   - the token label is withheld.
3. A guard for the drifted case. If the KV delta at `initialize_from_config` is under half of vLLM's own budget and no `mem.kvext` exists, the KV / outside / headroom rows are withheld and a note says why.
4. `test_boot_timing.py` covers the extensible boot, the drift guard, the optional-miss path and the unfired-span exemption; `UPSTREAM` hook table updated.

**Estimated effort**: Small (2 h) for T1; T2 is one owner boot.

---

## Scope

**In**: `container/patches/boot-timing/` (patch, test, UPSTREAM), this plan, its tracker, `SERIES.md`, `context/` findings.
**Out**:
- the launchers;
- the headroom *formulas* (hard/safe floor semantics are unchanged; only their inputs are fixed);
- the `KV cache (~Nk tokens)` label for hybrid models (see Rejected Claims).

If a fix needs to cross this line, **stop** and record the blocker in the tracker.

---

## Context

The first `ae525e9` boot (2026-11-03, `nvidia/Qwen3.8-Flash-Next-FP4-Local`, `run-fp4local` defaults) printed a memory section that contradicts vLLM's own log. This is the WATCH item the repin left in `container/patches/boot-timing/UPSTREAM` ("vllm#56492 turns on an extensible KV cache …").

### Pre-Identified Bug/Issue

The report as the owner pasted it (excerpt, verbatim):

```text
     KV cache                                          0.0 GiB  █                0.0%
     budget headroom (granted but unused)             18.2 GiB  ███              19.7%
   outside budget (driver, other)                   14.7 GiB
   could drop to  0.97 safe   -> frees 0.5 GiB · KV 0.0 -> 3.7 GiB
   hard floor     0.93        -> below this: no KV allocatable, vLLM refuses to boot
   · KV cross-check: measured vs vLLM's own budget differ by 17.7 GiB
```

vLLM's own log of the same boot (`docker logs vllm`, 2026-11-03, verbatim, ANSI stripped):

```text
[gpu_worker.py:843] Available KV cache memory: 17.66 GiB
[extensible_kv_cache.py:598] Memory after warmup: 6.51 GiB free, 13.06 GiB committed to the KV cache, 1.54 GiB kept for transient activations; 373 KV cache blocks (16.86 GiB) fit within the requested 92.68 GiB. Torch may reserve up to 76.04 GiB.
[extensible_kv_cache.py:480] Extended KV cache to 373 blocks (16.86 GiB).
```

The raw marks (`/tmp/vllm-boot-timing/marks.log` in that container, mem rows only):

```text
mem.kv.e	alloc=78674678272,res=78798389248,free=21867528192,total=102066159616
mem.kv.x	alloc=78675672576,res=78800486400,free=21815099392,total=102066159616
mem.final	alloc=78906873344,res=79274442752,free=6991642624,total=102066159616,rss=33121628160,...
```

`kv.x − kv.e` is 0.95 MiB of `alloc`: nothing is committed at `initialize_from_config`. `mem.final` has `free` = 6.51 GiB, which is exactly vLLM's "Memory after warmup … 6.51 GiB free" line, so it is taken *before* the extension to 16.86 GiB.

### Why hook `extend_kv_cache`

**Chosen**: wrap `Worker.extend_kv_cache` (`vllm/v1/worker/gpu_worker.py:1048` at `ae525e9`). The engine calls it once, after `compile_or_warm_up_model` (`vllm/v1/engine/core.py:414`). By its exit the cache holds its final size, and `model_runner.extensible_kv_cache.physical_bytes` is the exact mapped byte count. It rides the existing wrapper with zero new mechanism.

**Rejected**:
- **Parse vLLM's "Extended KV cache to N blocks (X GiB)" log line.** It is rounded to 0.01 GiB and couples us to log wording.
- **Read `torch.cuda` counters only.** The extensible cache is VMM-mapped outside torch's allocator, so `alloc`/`res` never see it. That blindness is the whole bug.
- **Pass `--no-enable-extensible-kv-cache` in the launchers.** That changes the product to suit the meter.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| Extension seam | `Worker.extend_kv_cache(self, num_blocks)` → `extend_kv_cache(self._v2_model_runner(), …)` | `gpu_worker.py:1048-1051` in a throwaway `--rm` of `…gae525e9`, 2026-11-03 |
| Called once, after warmup | `if extensible: … self.model_executor.extend_kv_cache(num_blocks)` right after `compile_or_warm_up_model()` | `vllm/v1/engine/core.py:407-414`, same container |
| Committed bytes | `ExtensibleKVCache.physical_bytes` ("Physically mapped bytes, including granule rounding"); `num_committed_blocks` | `vllm/v1/worker/extensible_kv_cache.py:271-273, :104`, same container |
| Runner attribute | `GPUModelRunnerV2.extensible_kv_cache: ExtensibleKVCache \| None`; `_v2_model_runner()` is a cast of `self.model_runner` | `vllm/v1/worker/gpu/model_runner.py:659`, `gpu_worker.py:1044-1046` |
| KV is outside torch's allocator | `mem.final`: used − res = 14.71 GiB = 13.06 GiB committed (vLLM log) + 1.65 GiB, and context is 1.40 GiB | arithmetic on the marks above, 2026-11-03 |
| Old (non-extensible) shape for comparison | KV 17.0 GiB, outside 1.8 GiB, hard floor 0.79 | `bench/bench-logs/flashnext-matrix-20261018-184027/humming-k3.log:349-366` |
| Seam absent on older images | `extensible_kv_cache.py` first appears at `ae525e9` for this repo (not in the 40fe396-era report) | unconfirmed for older shas — so the hook is **optional** (no `hooks NOT applied` line when absent) |

### Rejected Claims

- **"The token label `KV cache (~Nk tokens)` should come back."** It has been absent since before this plan: the 2026-10-18 report prints a bare `KV cache` too. `_kvcfg_detail` indexes `a[1][0]`, but `initialize_from_config` now takes one `KVCacheConfig` (`gpu_worker.py:937`). On this hybrid model, blocks × `block_size` isn't the token count anyway (373 blocks vs vLLM's 913,829 tokens). This plan only *withholds* the label on extensible boots. Fixing it is a Follow-up, not this plan.
- **"The hard floor should include the 1.54 GiB transient reserve."** The old path already ignored the profile peak in `hard` and showed it in the build-up instead. That's a semantics question, out of scope.

---

## Step-by-Step Tasks

### T1: Hook `extend_kv_cache` and render from it

**File**: `container/patches/boot-timing/boot_timing.py`, `test_boot_timing.py`, `UPSTREAM`

**What to do**:
- `TARGETS["vllm.v1.worker.gpu_worker"]` gains `dict(qual="Worker.extend_kv_cache", name="worker.kv_extend", mem="kvext", optional=True)`.
- `_apply` records no `miss` for an `optional` spec.
- `_mem_sample` gets a `kvext` exit branch: base + host + `kv_bytes`/`blocks` from `a[0].model_runner.extensible_kv_cache`, each under its own guard.
- `LABELS` gains the span. A new `_OPTIONAL_SPANS` keeps it out of "spans not seen this boot" (both the text and the JSONL paths).
- New helpers: `_kv_ext(m)`, `_fin(m)` (`mem.kvext` when it carries `kv_bytes`, else `mem.final`), and `_kv_withheld(m)` (no ext, and the KV `alloc` delta is under half of `mem.kvbudget`).
- `_mem_total`, `_gpu_parts`, `_render_host` and the JSONL host/GPU blocks read `_fin(m)`.
- `_gpu_parts`: kv = ext, else None when withheld, else the delta. outside = total − free − res − ext (None when withheld).
- `_mem_tokens` returns None on ext boots.
- `_mem_notes`:
  - ext boots get a "sized after warmup" line (profile budget → committed, blocks) instead of the measured-vs-budget comparison;
  - withheld boots get one explanatory note.
- Test: the existing synthetic (non-extensible) boot must render byte-for-byte the same numbers. Add the following cases:
  - an extensible boot shaped like the real one, with numeric asserts on KV, outside, hard/safe and the note;
  - the withheld case;
  - an `extend_kv_cache` mark without `kv_bytes`;
  - `_apply` on a fake module lacking the method (no miss) vs. having it (wrapped);
  - the unfired exemption.

**Verify**:
```bash
python3 container/patches/boot-timing/test_boot_timing.py | tail -1     # → ALL TESTS PASS
# the real seam, in a throwaway container (no GPU): the hook wraps the real method
docker run --rm --entrypoint python3 -v "$PWD/container/patches/boot-timing:/opt/boot-timing:ro" \
  -e PYTHONPATH=/opt/boot-timing -e VLLM_BOOT_TIMING_DIR=/tmp/bt "$IMAGE:$VTAG" -c \
  'import boot_timing as bt, vllm.v1.worker.gpu_worker as g; print(getattr(g.Worker.extend_kv_cache,"__boot_timed__",False))'
# → True   (plus gate steps 1–5, RULES.md § Project gate)
```
Expected: the test passes. The real `Worker.extend_kv_cache` is wrapped. The old synthetic boot's report is unchanged.

**Commit**: `[P7][T1][boot-timing] account for the extensible KV cache`

### T2: Owner boot — the report agrees with vLLM's log (gate step 6)

**File**: none (evidence only, tracker row)

**What to do**: The owner runs `./qwen3.8-flash-next/run-fp4local` on `…gae525e9` with the T1 tree mounted and pastes the boot report. Compare it with the same boot's `Extended KV cache to N blocks (X GiB)` and `Memory after warmup` lines.

**Verify**:
```bash
docker logs vllm 2>&1 | grep -E 'Extended KV cache|Memory after warmup|KV cache  |outside budget|sized after warmup|hard floor'
```

**Expected observation**:
- **Confirms if**:
  - the KV row equals vLLM's "Extended … (X GiB)" within 0.1 GiB;
  - the note says "sized after warmup";
  - outside-budget is in the 1–3 GiB range of the 2026-10-18 report (1.8);
  - the hard floor sits near the old 0.79, not 0.93;
  - no `hooks NOT applied` and no `spans not seen … worker.kv_extend`.
- **Refutes if**:
  - the KV row reads 0.0 or is withheld;
  - or outside-budget still holds about 15 GiB.
- **Noise**: n/a. This is accounting identity, not a measurement. One boot is enough to confirm the arithmetic, but not a timing.

**Commit**: `[P7][T2][boot-timing] record the owner boot that verifies KV rows`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `container/patches/boot-timing/boot_timing.py` | new optional hook + `mem.kvext`; report reads it | P0 |
| `container/patches/boot-timing/test_boot_timing.py` | extensible / withheld / optional-miss cases | P0 |
| `container/patches/boot-timing/UPSTREAM` | hook table row; WATCH item resolved | P1 |
| `context/distilled.md` | KV-outside-torch fact under the `ae525e9` section | P2 |

---

## Open Questions / Risks

1. **Risk: an optional hook can vanish silently on a later repin.** *Mitigation*: the withheld-KV guard. With no `mem.kvext` and a KV delta under half of vLLM's budget, the report withholds the KV rows and says so instead of printing 0.0. The UPSTREAM re-grep list names the target too.
2. **Risk: `physical_bytes` includes granule rounding, so it reads a little above vLLM's "373 blocks (16.86 GiB)".** *Mitigation*: that is the right number for a *memory* report (it is what is mapped). T2's 0.1 GiB tolerance covers it.
3. **Question: is `model_runner` the V2 runner whenever `extend_kv_cache` runs?** *How we'll settle it*: upstream asserts `use_v2_model_runner` inside `_v2_model_runner()` (`gpu_worker.py:1045`), so a non-V2 worker would raise before our exit sample. T2 observes it on the real boot.

---

## Verification Checklist

- [ ] T1: `python3 container/patches/boot-timing/test_boot_timing.py` → `ALL TESTS PASS`, including the extensible-boot numeric asserts; the throwaway-container check prints `True` for `Worker.extend_kv_cache.__boot_timed__`
- [ ] T2: the owner boot's KV row equals vLLM's "Extended KV cache … (X GiB)" within 0.1 GiB, and outside-budget is under 3 GiB
- [ ] All: the project gate (Rule A) passes on the final commit, every step `pass` or the skip named in the tracker
- [ ] All: step 6 ran (T2), **or** the tracker says the behavior went unverified at runtime
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
