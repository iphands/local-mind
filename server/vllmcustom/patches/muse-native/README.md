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

## Status — it serves

Booted `NATIVE=1 SPEC=0` on 0.27.1 and confirmed end to end:

- `Resolved architecture: MuseGlimmerForConditionalGeneration` → the native
  class, and **zero** "falling back to Transformers" warnings
- weights load, `Application startup complete`, coherent output at both
  temperature 0 and the production sampling settings
- the ATEM parsers still split reasoning from content correctly
- **27.4 tok/s vs 26.9** on the fallback (400 tokens, temp 0, same prompt) — a
  ~2% difference, i.e. speed is not the reason to do this

The reason to do this is correctness. The native model applies **both**
`output_multiplier` *and* the tanh `final_logit_softcapping`
(`muse_glimmer.py:1630-1632`). The fallback path could only supply the
multiplier, via `--hf-overrides text_config.logit_scale`; the softcap was the
documented residual in `run-muse`'s BUG 2 notes. Native closes that gap, which
is why `--hf-overrides` is dropped when `NATIVE=1` — forcing `logit_scale` on
top of a model that already scales would double-apply it.

Careful reading a "looks fine" result at temperature 0: argmax is invariant to
positive scaling, so greedy output cannot reveal a wrong logit scale. The check
above is the source (`muse_glimmer.py:1630`), not the sample.

## `NATIVE=1` with `SPEC=1` — works, but do not use it

It boots and serves, after two compat shims. It is measurably **worse** than
either half on its own, so it is not the configuration to run.

Getting there needed two backports, because the PR is branched off a newer
`main` than v0.27.1 and the *speculative* paths are where that divergence bites:

| symptom on 0.27.1 | fix |
|---|---|
| `AssertionError: Model instance must have 'model' attribute` | shim 8 — the EAGLE3 aux-layer lookup, verbatim from the PR's `interfaces.py` |
| `AttributeError: 'MuseGlimmerModel' object has no attribute 'model'` | `_compat_decoder_is_its_own_model` in `muse_native.py` |

Both are the same root cause: `MuseGlimmerForCausalLM` marks its inner
`MuseGlimmerModel` AS the language model, so `get_language_model()` already
returns the decoder and there is no further `.model`. Newer main writes
`getattr(x, "model", x)` in both call sites; 0.27.1 does not.

Of the PR's five core-file edits, only those needed replaying. `speculative.py`
auto-detects the dflash method from architectures (run-muse passes it
explicitly) and both `dflash.py` RoPE-propagation changes are inapplicable —
Muse Glimmer is NEOX-style on both sides, which is already the default.

Measured, temperature 1.0, same 114-char prompt (A/C are exact comparisons; the
long prompt grew between runs so B/D are indicative only):

| scenario | `NATIVE=0 SPEC=1` | `NATIVE=1 SPEC=1` |
|---|---|---|
| A short, 1 stream | .244 / len 4.66 / pos1 .800 / 102.1 tok/s | .164 / 3.46 / .793 / 81.5 |
| B long, 1 stream | .037 / 1.56 / .456 / 21.3 | .024 / 1.36 / .353 / 18.7 |
| C short, 3 streams | .243 / 4.64 / .783 / 272.3 | .138 / 3.07 / .670 / 203.8 |
| D long, 3 streams | .039 / 1.58 / .485 / 93.4 | .030 / 1.45 / .406 / 82.6 |

Note what moved in A: `pos1` is unchanged (.793 vs .800) while `len` falls
4.66 → 3.46. The drafter's FIRST guess is as good as ever; the deeper ones get
rejected more. That is the signature of a distribution mismatch rather than a
broken drafter, and the likely cause is the softcap: the native target applies
`* output_multiplier` **and** the tanh cap, while the drafter applies only the
multiplier (`LogitsProcessor(scale=...)`, no cap). Rejection sampling compares
`p_target/q_draft`, so making the target more correct while the drafter stays
uncapped widens the gap — and a tanh cap acts on the tails, which is where the
deeper draft positions live.

Softcapping the drafter's logits to match would likely recover it, and is a real
lead if speculation is ever wanted here. It would not rescue long context: even
the better fallback numbers are a net loss past ~2k tokens.

## Note on the drafter

Upstream maps the DFlash draft head to the **stock** `qwen3_dflash`
implementation — the same class `patches/muse-dflash` subclasses. So this PR is
not expected to change DFlash behaviour, including the long-context acceptance
collapse measured in `6da9c19`. Same algorithm, same weights, same 2048-token
window.
