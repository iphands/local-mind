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

## No vLLM patch replaces an upstream file, so nothing carries a content hash [REPO, amended 2026-10-15]

- All four patches under `container/patches/` (`boot-timing`, `draft-load-filter`,
  `sm120-gemm-plans`, `spec-live`) are post-import hooks: a `sitecustomize`/`.pth` mount that wraps
  upstream objects looked up by name at import time. Their `UPSTREAM` files state that no
  `BASE_SHA256` is present **deliberately** — there is no upstream file to hash, and a renamed target
  just no-ops, with the boot report printing `hooks NOT applied`.
- The exception, so do not over-read the heading above: `container/patches/vllm-flash-attn-arch.py`
  is not a hook. It is a `FetchContent` `PATCH_COMMAND` that edits the fetched vllm-flash-attention
  `CMakeLists.txt`, and it asserts each edit lands exactly once. It guards itself by anchor rather
  than by sha, so it still carries no `BASE_SHA256` — but it does modify upstream content, and it is
  the row `./container/archs` exists to prove.
- There **used to be** one whole-file overlay: `qwen3.8-flash-next/patches/ple-ct-ignore/`, which
  copied `ngram_embedding.py` and carried a `BASE_SHA256` that the **launcher** compared against the
  copy inside the image, never the host tree (a host-side check would compare the wrong artifact, so
  gate step 5 does not do one). vllm#59431 landed the same branch upstream on 2026-09-30, the overlay
  was inert from `47de9d4` on, and it was **deleted at the `4d9a8d6` repin**. Its base file had moved
  +221/−17 in that last range, so it was unrecoverable as an overlay regardless.
- Consequence: "add a check that the patches match upstream" is a no-op here — there is no base file
  to match. The checks that matter are `./container/archs` for the compiled extensions, the host-side
  `container/patches/*/test_*.py` (gate step 3), and the hooks' own runtime `NOT applied` lines.

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
