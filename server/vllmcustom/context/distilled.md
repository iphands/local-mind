# Distilled — confirmed facts for vllmcustom

Compact, confirmed learnings: formats, protocols, tool/library behavior, measured limits. Read
before new work; add an entry whenever something is confirmed — **above the `## Open Questions`
section**, which always stays last. Keep it dense.

**Provenance discipline:** every entry carries a tag — `[SOURCE]` (upstream code, cite
`path:line`), `[DOC]` (documentation), `[REPO]` (this repo's own code/config), `[LIVE YYYY-MM-DD]`
(observed on a running system). **Do not upgrade a tag without doing the work.**

<!-- Entry shape:

## <Topic — the rule in one line> [TAG][TAG]

- The fact, stated so it can be checked.
- The evidence: command + verbatim output, or `path:line`.
- What it means for this project.
-->

## The host-runnable test surface is exactly four files, and two patches have none [LIVE 2026-10-13]

- `container/patches/{boot-timing,draft-load-filter,sm120-gemm-plans,spec-live}/test_*.py` run on
  the host with plain `python3 <file>` (stdlib only, no vLLM, no pytest — pytest is not installed).
  Each prints `ALL TESTS PASS` and exits 0.
- `muse/patches/atem-parsers/selftest.py` looks host-runnable from its docstring but is not: it
  imports `transformers` and `vllm.parser.parser_manager`, so it runs in the image via
  `./muse/run --selftest`.
- `muse/patches/muse-embed-norm/` (five shims, one of which prevents the
  `assert isinstance(model_output, tuple)` crash) and `muse/patches/muse-dflash/` have **no test at
  all**. Anything touching them is verified at runtime or not at all — say which.

## Only overlays that copy an upstream file carry a content hash [REPO]

- `container/patches/{boot-timing,draft-load-filter,sm120-gemm-plans,spec-live}/UPSTREAM` state
  that no `BASE_SHA256` is present **deliberately**: they are post-import hooks, so there is no
  upstream file to hash — a renamed target just no-ops and the boot report says
  `hooks NOT applied`.
- `qwen3.8-flash-next/patches/ple-ct-ignore/UPSTREAM` does carry a hash, and the **launcher**
  checks it against the copy inside the image, not against the host tree. A host-side hash check
  would compare the wrong artifact, so gate step 5 does not do one.
- Consequence: "add a check that the patches match upstream" is mostly a no-op here. The check that
  matters is `./container/archs` for the compiled extensions, and the launcher's own refusal.

## This host reaches docker only through sudo [LIVE 2026-10-13]

- `docker info` as the user: `permission denied while trying to connect to the docker API at
  unix:///var/run/docker.sock`. `sudo -n docker info` → `29.7.2`. The account is in groups
  `iphands,sudo`, not `docker`.
- Every script here calls bare `docker` (`common/_run-lib.sh:require_image`, `container/archs`,
  `container/push`), so each needs a `sudo -n` prefix from an agent shell.

---

## Open Questions

- `./container/archs` has not been run against the current `df6c7ad` snapshot image (nor against
  `187a0eb`, its predecessor) — the arch
  table is asserted by the build's own patch script, not re-measured. Confirm before trusting it.
- Whether the FA2 `_vllm_fa2_C` extension is still sm_120-native on the main snapshot (the README
  verified the *patch anchors* on 2026-09-22, not the emitted SASS).
