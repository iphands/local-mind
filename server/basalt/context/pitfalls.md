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

---

# The image boots the mock engine but not a real pack: `serve/` imports `tools/model_file.py`

## Problem
[OBSERVED 2026-11-04] The first image (`27770926a3f6`) shipped `bin/`, `serve/`, `data/` and the
venv, passed every gate step and served the mock engine — then the first real `./run` died right
after `[server] legend`:
`File "/opt/basalt/serve/engine/modelfile.py", line 14 … from model_file import ModelFile` →
`ModuleNotFoundError: No module named 'model_file'`. `modelfile.py` does
`sys.path.insert(0, ROOT / "tools")` and imports by bare name, so a grep for `from tools` /
`import tools` over `serve/` (the check made while planning) found nothing, and the mock engine
never imports `serve.engine.modelfile` (it has no model file). Upstream's `docs/building.md` does
say an install or container image ships `serve/` **and `tools/`** — recorded in `distilled.md` at
init and still left out of the Dockerfile.

## How to avoid
The image ships all of `tools/` (448 KB). Gate step 6 imports every non-test `serve.*` module in the
image, so a missing file fails before a boot; on the broken image it printed exactly
`FAIL(imports) serve.engine.modelfile: ModuleNotFoundError: No module named 'model_file'`. When
upstream's docs list what an install ships, copy that list — don't derive it from imports.

## Sources
- basalt: `Dockerfile` (runtime-stage `COPY`s); `serve/engine/modelfile.py:12-14` at `dc047a77`
- basalt: `context/plans/RULES.md` § Project gate, step 6
