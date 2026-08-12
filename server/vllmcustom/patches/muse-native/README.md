# muse-native — upstream's in-flight Muse Glimmer support, loaded without a rebuild

vLLM 0.27.1 has no Muse Glimmer. `run-muse` therefore serves it through the
generic Transformers backend:

```
WARNING [utils.py:217] TransformersMultiModalForCausalLM has no vLLM
implementation, falling back to Transformers implementation.
```

That fallback is why `patches/` exists: it drops the embedding RMSNorm, ignores
`output_multiplier`, reads `layer_types` from the wrong config level, and
silently fails to capture the aux hidden states DFlash needs.

[vllm-project/vllm#51655][pr] adds a native implementation. This directory
vendors the files that PR **adds** and loads them out of tree, so the native
path can be exercised **without rebuilding the image** — which matters because a
rebuild here is multi-hour and the PR is still moving.

[pr]: https://github.com/vllm-project/vllm/pull/51655

## Use it

```bash
./scripts/muse-native-sync     # refresh vendor/ from the PR, report what moved
NATIVE=1 ./run-muse            # serve with it — no rebuild
```

Confirm it took: the fallback warning is gone and the log says

```
[muse-native] upstream 99a10304dce8 (2026-08-11, pr 51655)
INFO [model.py:645] Resolved architecture: MuseGlimmerForConditionalGeneration
```

`NATIVE=0` (the default) is the shimmed Transformers-backend path, unchanged.

`vendor/` is **gitignored** (`.gitignore:1`), matching how this repo treats every
other vendored tree. It is regenerable and pinned: `UPSTREAM` records the exact
commit, so `muse-native-sync` reproduces it byte for byte. A fresh clone must run
the sync once before `NATIVE=1` will work — `run-muse` says so if you forget.

## Tracking upstream

```bash
./scripts/muse-native-sync --check    # writes nothing; exits non-zero if upstream moved
./scripts/muse-native-sync            # pull the new revision in
```

`--check` is cheap enough to run from cron or a pre-flight. `UPSTREAM` records
the vendored commit; the sync prints `MOVED since last sync` and a per-file
`new`/`CHANGED`/`same`/`STALE` line, so a refresh shows exactly what upstream
touched.

Once the PR merges into a release `./build` pins, delete this whole directory
and the `NATIVE` knob — that is the win condition.

## What is vendored, and what is not

**Vendored, verbatim, never edited** — files the PR *adds* under `vllm/` whose
path mentions muse/glimmer. Nothing by those names exists in 0.27.1, so they can
only add modules, never shadow core vLLM. Because there are no local edits, a
refresh is a copy and never a merge. `muse_native.py` resolves them under their
real dotted names through a `MetaPathFinder`, which also means load order does
not matter and a file added by a future revision is picked up with no changes
here.

**Not vendored** — the PR's edits to *existing* core files. Copying those
wholesale would drag in unrelated changes from a newer tree. There are only a
few, and they are replayed instead:

| upstream | replayed in |
|---|---|
| `registry.py:530` `MuseGlimmerForConditionalGeneration` → `MuseGlimmerForCausalLM` | `run-muse` `--model-class-overrides` |
| `registry.py:631` `DFlashMuseGlimmerAssistantModel` → `qwen3_dflash.DFlashQwen3ForCausalLM` | `patches/muse-dflash` (a subclass, for the `encoder.*` rename) |
| `transformers_utils/config.py:104-107` four `muse_glimmer*` config classes | shim 7 in `patches/muse-embed-norm/sitecustomize.py` |

`muse-native-sync` prints those core-file lines on **every** run. If they change,
that is the signal to update the replays above.

## Status

Verified against 0.27.1, offline:

- all five vendored modules import
- `_CONFIG_REGISTRY` resolves `muse_glimmer` → upstream's `MuseGlimmerConfig`
- vLLM resolves `MuseGlimmerForConditionalGeneration` → the native class, and the
  fallback warning is gone

**Not yet verified: serving.** Import and resolution say nothing about whether
weight loading, the multimodal processor and the attention wiring work on
0.27.1. That needs a boot.

Also untested: `NATIVE=1` together with `SPEC=1`. Upstream drives the drafter
with the stock `DFlashQwen3ForCausalLM`, same as `patches/muse-dflash` does, so
they should compose — but the draft-config shim (shim 3) and upstream's
`MuseGlimmerAssistantConfig` both want to supply the same fields, and only one
of them should win. Expect to have to sort that out.

## Note on the drafter

Upstream maps the DFlash draft head to the **stock** `qwen3_dflash`
implementation — the same class `patches/muse-dflash` subclasses. So this PR is
not expected to change DFlash behaviour, including the long-context acceptance
collapse measured in `6da9c19`. Same algorithm, same weights, same 2048-token
window.
