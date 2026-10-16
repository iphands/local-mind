# Plan 02 — VLLM-first docs + read-only vendor discovery space — Tracker

## Overview

- Status: 50% complete (3/6 tasks)
- Start date: 2026-10-16
- Plan: `context/plans/02_vllm_first_docs.md`
- Evidence: doc-framing plan — evidence is the grep output each Verify prints (the claim flipping
  vLLM-first) plus the `git check-ignore` / `rg` blindness pair in T3. No live perf numbers, so
  the Evidence table records commands + observed output, not measurements.

## Resume Instructions

1. Read the project's root `AGENTS.md`/`CLAUDE.md`, `context/plans/RULES.md` (in full), then
   `02_vllm_first_docs.md`. Note the **Preservation List** in T4 — it is the contract.
2. Environment: no live backend needed. `AGENTS.md` is a **symlink** to `CLAUDE.md` (root and
   `context/`) — always edit `CLAUDE.md`, never `AGENTS.md`, and **never `sed -i AGENTS.md …`**:
   write-and-rename tools (`sed -i`, most editors' save) replace the symlink with a regular file,
   after which `diff -q AGENTS.md CLAUDE.md` still passes while the two docs have silently forked.
   `vendor/{vllm,llama.cpp}` are on disk, untracked; after T3 they are gitignored, so search them
   with `rg --no-ignore`.
3. Ordering: **T1 → T2 → T3** (write the vendor rule + KB row *before* ignoring the tree, so the
   escape hatch is in place before grep goes blind), then T4, T5, T6 (independent). T4 must not
   touch the `hands-plan:begin` block; T5 must not rename the "Is it really streaming?" heading.
4. These three artifacts (plan + tracker + SERIES row) land first, before any task, as
   `[llama-proxy][P2][plan] add vllm-first docs + read-only vendor discovery` (RULES.md Rule B
   bookkeeping tag — do NOT fold them into T1's commit). Then commit per task as
   `[llama-proxy][P2][T<n>][<topic>] <summary>` (plan number **not** zero-padded), with **this
   tracker's row in the same commit**. When T1 starts, flip plan `Status` → `in-progress`, this row
   → `in-progress` under SERIES *Currently Active*, and set `Start date` — all in T1's commit.

## Open Unknowns

- Does any tracked doc outside CLAUDE.md/README.md/fixrate.md still claim llama.cpp is *the*
  target? → **assigned T6** (T6 greps with `--exclude-dir` and files a disposition row per hit).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: P3 read-only vendor rule + `rg --no-ignore` | `context/plans/RULES.md` | done | added at :262; wording corrected to reality (see Notes) |
| 2 | T2: register vendor in context KB map | `context/CLAUDE.md` | done | Map + Findings rows; wording diverged from draft (see Notes) |
| 3 | T3: gitignore `/vendor/`, observe ignore took | `.gitignore` | done | landed as `/vendor` (symlink cover); P3 state-sync follow-on |
| 4 | T4: root CLAUDE.md vLLM-first | `CLAUDE.md` | pending | keep hands-plan block + Preservation List |
| 5 | T5: README + fixrate vLLM-first | `README.md`, `fixrate.md` | pending | don't rename cited headings |
| 6 | T6: triage docs + stamp PLAN.md obsolete | `PLAN.md` (+grep) | pending | stamp, don't delete |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-16 | `hp-scan --brief .` | `NEXT computed=02` | 1 | plan number source |
| 2026-10-16 | `ls -l AGENTS.md context/AGENTS.md` | both → `CLAUDE.md` symlinks | 1 | edits go to CLAUDE.md |
| 2026-10-16 | `rg --files target` | listed gitignored `target/` files | 1 | env rg shows ignored tree when passed explicitly |
| 2026-10-16 | `git check-ignore -v vendor/vllm/README.md` | **exit 1 — not ignored**; `git status` → `?? llama-proxy/vendor/` | 1 | invalidated P3's drafted "gitignored, edit silently lost" premise → corrected |
| 2026-10-16 | `RUSTFLAGS='-D warnings' cargo check/clippy …` + `cargo fmt --check` + `cargo test --workspace --lib` | check/clippy/fmt exit 0; 915 passed | 1 | T1 gate. `just gate` (`-- -D warnings`) errors in this cargo — see Follow-ups |
| 2026-10-16 | T2 verify: `grep 'vendor/vllm' context/CLAUDE.md` + `grep 'read-only' …` + `diff -q context/AGENTS.md context/CLAUDE.md` | both greps hit at `:19`; `SYMLINK-OK`; check/fmt exit 0 | 1 | two KB rows added (Map + Findings); no `.rs` touched |
| 2026-10-16 | `git check-ignore -v vendor/vllm/README.md` (post-T3) | **`llama-proxy/.gitignore:8:/vendor`** exit 0 — source column names OUR file (not `info/exclude`/global) | 1 | causal proof the ignore came from the edited file |
| 2026-10-16 | `git status --porcelain vendor/` (post-T3) | empty, exit 0 (was `?? llama-proxy/vendor/`) | 1 | tree now git-ignored |
| 2026-10-16 | rg blindness pair (observed, not asserted) | root `rg --files`→ grep `^vendor/`=**0**; `rg --no-ignore --files vendor`=**11212**; `RIPGREP_CONFIG_PATH=unset` | 1 | invisible to normal search, fully readable with `--no-ignore` — P3 recipe works |

_(T3 ran: the `rg` blindness pair — root `^vendor/`=0 vs `--no-ignore` vendor=11212 — is recorded in the Evidence table above.)_

## Notes / Deviations

**Anything the plan asserted that turned out wrong, and what was done instead (Rule E).**

- (pre-exec) fixrate.md was found to already name the vLLM split path (`:47`, `:387`, `:411`), so
  T5's fixrate change was narrowed from "add the path" to "add a cross-reference + vendor pointer."
- **(T1) The plan's drafted P3 was invalid as written.** It asserted the vendor tree "is gitignored
  (Plan 02), so an edit there is silently lost." Reality (verified 2026-10-16): `git check-ignore
  vendor/vllm/README.md` → exit 1, `git status` → `?? llama-proxy/vendor/` — **not ignored** (T3 has
  not run; the root `.gitignore` `/vendor` is workspace-root-anchored and misses `llama-proxy/vendor/`),
  and "silently lost" is the **wrong hazard** — gitignoring keeps files on disk; the real danger is a
  `git add -A` staging the checkout. P3 was rewritten (Rule E) to state the verified current state,
  attribute the ignore to T3, name the `git add -A` hazard, widen the ripgrep recipe
  (`--no-ignore --hidden -g '!**/build/**'`), disambiguate `vendor/` vs the dead `../vendor/`, and
  route findings to `context/distilled.md`. Plan item "T1" marked **done with correction**; the
  drafted wording is superseded, not executed.
- **(T1) The project gate command is broken in this toolchain.** `RULES.md:122/124` and `justfile:16`
  run `cargo check/clippy … -- -D warnings`, which this cargo rejects
  (`unexpected argument '-D' found`); `just gate` cannot pass as written. Ran the equivalent
    `RUSTFLAGS='-D warnings' cargo check/clippy` + `cargo fmt --check` + `cargo test --workspace --lib`
  (all exit 0, 915 tests pass). Not silently rewritten (gate is authoritative) → Follow-up below.
- **(T2) The drafted Map row was corrected before writing (Rule E), per both reviewers.** The plan's
  T2 spec drafted it as "…`vendor/vllm`, `vendor/llama.cpp` | upstream backend sources, **read-only +
  gitignored** …search with `rg --no-ignore`". Two defects caught in review: (a) "**gitignored**" as a
  present-tense fact repeats the exact claim blocked in T1 — the tree is not ignored until T3; and
  (b) a truncated `rg` recipe in the Read-when column is a lossy copy of P3's canonical command (drops
  `-g '!**/build/**'`, the guard against a stale build faking a hit) sitting in a *trigger* column.
  Written instead as P3's canonical `vendor/vllm/` + `vendor/llama.cpp/` spelling with an explicit base
  anchor `(project root: llama-proxy/vendor/)`, marked **outside this KB**, carrying the *real* hazard
  (`never edit, never git add`) and pointing at RULES P3 for the ignore-state + full search recipe. The
  Findings row uses P3's concrete cite form `vendor/<tree>/<path>:<line>` + `[REPO]`. Dropping the
  "gitignored from T3" clause also removes the stale-by-construction cell, so no T3 update is owed here.
- **(T3) The drafted comment and pattern were corrected in review (Rule E).** The plan drafted the
  ignore as `/vendor/` with a comment offering only `rg --no-ignore` and citing "Plan 02". Both
  reviewers flagged the comment as a *divergent partial* of P3's canonical recipe (drops `--hidden`
  and `-g '!**/build/**'`) that reads complete → replaced with the full flags + a `context/plans/RULES.md
  (P3)` pointer (hoodie: "Plan 02" rots once the plan closes; P3 outlives it). Pattern changed
  `/vendor/` → **`/vendor`** (no trailing slash) so a *symlinked* `vendor` can't slip past a dir-only
  match; the leading `/` still anchors it to `llama-proxy/`, so it neither matches `src/vendor/` nor the
  workspace-root `/vendor`. Comment also now warns an edit here is invisible/unrevertable by git (hoodie)
  — which is the honest hazard *post*-ignore. Verified safe: `git ls-files vendor/`=0 (nothing tracked to
  hide), file ended in a newline. **Follow-on (neckbeard's T1 ask):** landing T3 makes RULES P3's
  "not yet ignored" clause stale — syncing P3 to the post-ignore state ships as `[P2][T3][docs]` next.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Update CLI `about`/`long_about` strings (`src/main.rs:47`) + `main.rs:1`/`lib.rs:1` doc comments to vLLM-first | Observable `--help` behavior; code change, out of a docs plan's scope | a later code plan |
| Delete `PLAN.md` after human review (currently OBSOLETE-stamped) | Deletion is a human call; append-only until then | next resync / human |
| Consider a "which backend gives me what" table (`/props`, `/slots`, `timings`, `metrics`, `vllm:cache_config_info`, `max_model_len`) in README | Single table answers most vLLM-first positioning questions | optional README follow-up |
| **Fix the gate**: `justfile:16` + `RULES.md:122/124` use `cargo … -- -D warnings`, rejected by this cargo (`unexpected argument '-D' found`); `just gate` cannot pass as written | Reproducibility: every task's gate claim rests on this command; workaround is `RUSTFLAGS='-D warnings' cargo …` | separate bookkeeping commit (authoritative file — human sign-off first) |
| Fix root `CLAUDE.md` "Reference Implementation" `../vendor/claude-code`, `../vendor/opencode`, `../vendor/llama.cpp` — resolve to a **nonexistent** `local-mind/vendor/`; real checkouts are `llama-proxy/vendor/{vllm,llama.cpp}` | Dead pointers misroute discovery work; P3 now warns against them | ride T4 (same file) or a later docs pass |
