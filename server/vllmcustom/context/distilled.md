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

## This project reaches the docker socket directly — it uses no privilege helper [LIVE 2026-10-13, amended 2026-10-13 (Plan 02)]

- Socket ownership after the owner's host fix, measured 2026-10-13: `iphands:iphands 660` — owned by
  the invoking uid, **not group-owned**, so there is no group to join and no re-login step. Plain
  `docker version` returns `29.7.2` at rc 0 with no privilege of any kind.
- **`sudo` is absent from the normal operating environment** and must not be expected: the documented
  `VLLM_REF=main ./container/build` died on `timeout: failed to run command 'sudo': No such file or
  directory` (exit 127). Two provenance warnings: the *agent's* container ships `/usr/sbin/sudo` and
  `/usr/sbin/docker`, so never infer host facts from that shell; and Plan 01's tracker carries a dated
  row measuring `sudo -n -l` → `(ALL) NOPASSWD: ALL`. Both were true of their own environment on the
  same date. The durable statement is "this project uses no privilege helper", not "this box has no
  sudo".
- `container/build`, `container/archs` and every launcher define a `DOCKER` array defaulting to plain
  `docker`, **refuse a value naming `sudo`**, and probe `docker version` before the first call that
  could misreport why it did not. `container/build` also refuses uid 0, with no escape hatch.
- **History, in order, because each version was once correct.** (1) Every script called bare `docker`
  and an agent shell had to prefix it. (2) Plan 01 routed all of it through `${DOCKER[@]}`, default
  `sudo -n docker`, and the consequence *inverted*: prefixing a script **disables** the override,
  because `env_reset` drops `DOCKER`, so the script silently takes its own default — a gate step that
  runs the wrong command and still exits 0. `container/build` and the launchers began refusing uid 0.
  (3) **Plan 02 (2026-10-13) removed the sudo machinery entirely**: default is plain `docker`,
  `ALLOW_ROOT` is deleted (and announces itself if set), the gate's raw docker lines lost their
  prefix, and 7a's freed assertion slot now asserts the new guard. What survived from (2) is the shape
  — `${DOCKER[@]}` everywhere, probe before first use, never run privileged — **and the root refusal**,
  kept deliberately because `_writable` tests `-w`, which is always true for root, so nothing else in
  the tree can catch a privileged run before it force-checks-out the four hand-editable trees.
- Bare and not `DOCKER`-aware, unchanged: `container/push`, `bench/_bench-lib.sh`, `bench/bench-wrapper`,
  `bench/bench-context`, the ten `scripts/*` that shell out. Since Plan 02 that is no longer a
  difference worth tracking — plain `docker` is now what everything uses, so they work or fail exactly
  when everything else does. The two root-side scripts remain `container/gc-setup` and
  `container/slice-setup`, by design, both via `su -c`.

---

## Open Questions

- `./container/archs` has not been run against the current `df6c7ad` snapshot image (nor against
  `187a0eb`, its predecessor) — the arch
  table is asserted by the build's own patch script, not re-measured. Confirm before trusting it.
- Whether the FA2 `_vllm_fa2_C` extension is still sm_120-native on the main snapshot (the README
  verified the *patch anchors* on 2026-09-22, not the emitted SASS).
