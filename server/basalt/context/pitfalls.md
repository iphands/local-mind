# Pitfalls & Gotchas — basalt

Every bug, gotcha and trap — **especially anything that took more than one attempt to fix.** Read
before new work. Tag entries that actually bit `[OBSERVED YYYY-MM-DD]`; untagged entries are
hazards seeded from reading, not incidents.

Cross-cutting entries also go up to `../../../context/pitfalls.md`; project-specific ones stay here.

<!-- Entry template (~200 words per half; quote the symptom verbatim):

# <Pitfall title>

## Problem
[OBSERVED YYYY-MM-DD] What happened, the exact error/output, why it was confusing, what was
tried first and why that failed.

## How to avoid
The fix, the rule that prevents it, and how to detect it early next time.

## Sources
- <project/sub-project>: `<short_filename>` (`<function_name>`)
-->

---

# The "NInfer" QUASAR 27B looks like a Basalt model, and is not

## Problem
Seeded from reading at init (2026-11-04), not yet an incident. The first request for this project
named `models/vllm/jesdga/Qwen3.8-27B-QUASAR-DFlash2-nvfp4-NInfer` for `./run`. The HF author
(`jesdga`) is also Basalt's author and publishes Basalt packs, so the artifact reads as Basalt's.
It is `qwen3_8_27b_quasar.ninfer`: a dense Qwen3.8-27B file for NInfer. Basalt loads only
Qwen3.8-Flash-Next `.basalt` packs; handed the `.ninfer` file it would fail at load, after the
container started.

## How to avoid
`./run` checks the pack before starting anything: the path ends in `.basalt` and exists, else it
dies naming `../ninfer` for `.ninfer` files. Recorded as out of scope in `plans/SERIES.md`.

## Sources
- basalt: `context/distilled.md` (Basalt loads only Qwen3.8-Flash-Next …)
- the artifact's `README.md` (`library_name: ninfer`)
