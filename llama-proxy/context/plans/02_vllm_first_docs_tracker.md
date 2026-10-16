# Plan 02 — VLLM-first docs + read-only vendor discovery space — Tracker

## Overview

- Status: 100% complete (6/6 tasks) — all tasks done; closing (harvest + `git mv` + SERIES) is the
  next and final step (Rule C/D).
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
| 4 | T4: root CLAUDE.md vLLM-first | `CLAUDE.md` | done | tagline/framing/vLLM-Extensions block landed; `../vendor/`+`examples/server` pointers → zero; hands-plan block + `../context/` notes refs untouched |
| 5 | T5: README + fixrate vLLM-first | `README.md`, `fixrate.md` | done | intro/diagram/API-Compat vLLM bullet + fixrate producer-doc block; all 3 Preservation greps hit; cited heading intact; box re-measured 63==63 |
| 6 | T6: triage docs + stamp PLAN.md obsolete | `PLAN.md` (+grep) | done | OBSOLETE banner stamped (not deleted); 7 framing edits + triage table (see Notes); hard grep `reverse proxy for llama`→**0**; inventory survivor = `CLAUDE.md:282` vendor pointer → **Leave** (accurate); gate 915 passed |

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
| 2026-10-16 | P3-sync verify: `ls vendor/*/.git`; `git -C vendor/<t> rev-parse HEAD`; `git add --dry-run vendor/vllm/README.md`; `git checkout -- vendor/…` | both `.git` present (vllm@`642c18d`, llama.cpp@`b6334e3`); `git add` **refused** ("Use -f"); outer `git checkout` errors "pathspec did not match" | 1 | proves the three review corrections before baking into the rule (inner repo is the revert path; ignore is outer-scoped) |
| 2026-10-16 | `ls -la vendor/llama.cpp/tools/server/README.md`; `ls vendor/llama.cpp/examples`; `ls -l AGENTS.md` (`-ef` test); `find .. -name opencode_claude_llama_notes.md` | README.md = **117 KB, real**; `examples/` present but `tools/server/` is the live server; **AGENTS.md→CLAUDE.md same inode**; notes file **found: nowhere** | 1 | T4 ground-truth probe: settles the reviewer gitignore-artifact dispute (see Notes) — `vendor/` IS on disk; the pointer target `tools/server/` is correct |
| 2026-10-16 | grep verify of reframed `CLAUDE.md`: `grep '\.\./vendor/'` + `grep 'examples/server'` + `grep -i vllm \| head -1` + hands-plan markers + `grep opencode_claude_llama_notes` | `../vendor/`=**0**, `examples/server`=**0**, first `vllm`=**L7 (Overview)**, block @298/317 intact, notes refs=**3** (L93/284/404, none in block) | 1 | dead `../vendor/` pointers eliminated, vLLM-first landed in Overview |
| 2026-10-16 | `RUSTFLAGS='-D warnings' cargo check --workspace --features influxdb` + `cargo fmt --check` + `cargo test --workspace --lib` (post-T4) | check exit 0 (cached 0.20s); fmt exit 0; **915 passed** | 1 | T4 gate — no `.rs` touched (markdown-only), gate stays green |
| 2026-10-16 | T5 Verify battery: plan greps (intro `for **vLLM**`, diagram `vLLM / llama.cpp / OpenAI`, old `proxy for [llama.cpp]` gone exit=1, `Is it really streaming?` intact, flag at README :258/:310/:803+fixrate) + 3 Preservation greps (prefix-cache :322, `tps>=` :311, absent≠zero :292-293) + box width re-measure | ALL pass; box 392/393/394 all =63 (aligned); `**vLLM**` count=**2** (intro L6 + bullet L802), not the plan's stated 1 — the plan's own intro edit also bolds vLLM, so 2 is the correct outcome (plan Verify self-inconsistent, see Notes) | 1 | T5 evidence; Preservation = Risk #1 guard all green |
| 2026-10-16 | T5 gate (post-edit, markdown-only) | `RUSTFLAGS='-D warnings' cargo check` cached 0.20s; fmt 0; **915 passed** | 1 | no `.rs` touched |
| 2026-10-16 | T6 source probe (banner assertions, ground-truthed before baking): `grep -rn <sym> src/` for the three deleted symbols + `grep 'fn find_string_end'`; `git log -1 21f0548`; `grep -n 'Streaming Fixes…' CLAUDE.md` | `calculate_completion_delta`/`apply_stream_with_accumulation_default`/`safe_completion` → **0 in `src/`** (so "Changes Made" is a false record); `find_string_end` **live @:231**; `21f0548`="big-fix 29: delete dead legacy streaming machinery"; heading verbatim carries **(Important!)** @:341 | 1 | hoodie's claims held this time (unlike the T4 vendor glob artifact) — every one verified, then adopted |
| 2026-10-16 | T6 authoritative greps verbatim (plan:287-289 inventory + plan:312-314 hard criterion; `--exclude-dir=vendor\|target\|.git` + plan's own `grep -v 'completed/\|PLAN.md\|02_'`) + gate | hard criterion → **0**; inventory → **1** survivor `CLAUDE.md:282` (→ Leave); `grep -n OBSOLETE PLAN.md`→**L1**; gate check/fmt 0, **915 passed** | 1 | first improvised grep (no plan-file exclude) matched the plan's *own* quoted Before/After — plan:291 forbids editing those; re-ran the defined command; markdown-only, no `.rs` |

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
   hide), file ended in a newline. **(P3 sync, this commit):** landing T3 made RULES P3's "not yet
   ignored" clause stale, so P3 was rewritten to the verified post-ignore state (Rule E). Review caught
   three of my own errors before they shipped: (a) "re-exposing takes `git add -f` *or an explicit
   path*" is false — `git add <ignored path>` *refuses* ("Use -f"), only `-f` forces (verified); (b)
   "an edit → nothing to revert" is false for the **outer** repo only — `vendor/{vllm,llama.cpp}` are
   real checkouts each with its own `.git` (verified: vllm@`642c18d`, llama.cpp@`b6334e3`; outer
   `git checkout -- vendor/…` errors "pathspec did not match"), so the inner repo is the revert path;
   (c) I omitted the real post-ignore data-loss risk — **`git clean -fdx` deletes the ignored tree**
   with no outer history to restore it. P3 now scopes "invisible" to the outer repo, gives the inner-
   repo revert + SHA commands, and bans `git clean -fdx`. The `.gitignore:7` comment was corrected to
   match (both reviewers: the two files must teach one recovery model). Pre-T3 wording is kept ONLY here
    in the tracker (authoritative rule states current verified state + dated commit, no archaeology).
- **(T4) The two reviewers disagreed on the one load-bearing fact, and it was a gitignore artifact —
  `ls` settled it, not either reviewer.** hoodie asserted `vendor/` does not exist and recommended
  pointing the doc at `../server/llamacpp/`; neckbeard first said the same, then caught its own blind
  spot ("`vendor/` is gitignored, so glob-based verification silently returns nothing — use `ls`").
  Both used glob/read tools that respect the ignore **T3 just landed** — so a missing tree was a tool
  artifact, not reality. Ground truth (`ls`): `vendor/llama.cpp/tools/server/README.md` = 117 KB and
  real; `AGENTS.md`→`CLAUDE.md` same inode (so hoodie's "sync the separate AGENTS.md" was moot). The
  correct pointer is `vendor/llama.cpp/tools/server/` with **no `../`** (CLAUDE.md sits at
  `llama-proxy/`, checkout at `llama-proxy/vendor/`), matching P3's own convention — hoodie's "dropping
  `../` is a bug" was backwards. Overrides recorded here per the protocol (Rule E honesty: name where
  a reviewer was overruled and why).
- **(T4) Reviewers' substantive advice was adopted; one phrasing was refused on honesty.** Both agreed:
  promote vLLM to a standalone `✅ **vLLM Extensions**` block (don't bury it under the llama.cpp
  heading), drop the framing-note blockquote + rotting date, and say "re-ordering **plus pointer
  corrections**" (not "re-ordering only" — the edit changes pointers and adds claims). neckbeard's open
  check — are the vLLM claims real or aspirational? — was closed against source before writing:
  `--enable-per-request-metrics` gates the per-request `metrics` split (`collector.rs:444/465/699`,
  `streaming.rs:1833`) and `select_max_model_len` (never `data[0]`) drives the `/v1/models` tier
  (`preflight.rs:289`, `context.rs:239`). **Refused:** hoodie's tagline "vLLM **by default**" — the
  shipped default config points at `llama.cpp:8080`, so "by default" is a false runtime claim; conveyed
  vLLM-first by list *order* + "primary **documented** backend" instead.
- **(T4) New dead pointer found, deliberately NOT fixed (scope + no valid target).** Fixing the
  `../vendor/` pointers surfaced that `../context/opencode_claude_llama_notes.md` — cited 3× (L93, L284,
  L404) as "the comprehensive client notes" — **exists nowhere** (`find … -name` → nothing), at any
  prefix: the file is gone, not mis-pathed. Left untouched: outside the human-approved `../vendor/`
  scope, no valid target exists to repoint to (a prefix swap would still be dead), and deleting three
  "see the comprehensive docs" references is a content call, not a mechanical fix. All three are
  OUTSIDE the hands-plan managed block (L298–317), so it is safely editable whenever the human decides →
  Follow-up below. (An earlier note wrongly said one sat *inside* the block; the block does not cite it.)
- **(T5) Both reviewers redirected the drafted edits; two divergences resolved against source/audience
  (Rule E).** neckbeard (correctness) verified every vLLM claim before I baked it: the producer-doc
  caveats are real (`per_request_metrics.md:20-22/60-69`) and the "measured `tps=A/B`" label is honest
  (`collector.rs:490-501`: `prompt_tps`=prompt/TTFT, `generation_tps`=completion/`generation_time_ms`
  decode-only, gated by `has_timing_split`, never the prefill-inclusive `tokens_per_second`; `metrics:null`
  → floor+cost). hoodie (usability) caught that my drafted README intro made `…or any OpenAI-compatible
  server that fixes…` a garden-path (the `that` attaches to *server*) — adopted hoodie's two-sentence split
  (kept the literal `for **vLLM**` the plan Verify needs). hoodie also objected to my `**Framing:** …
  no proxy behavior changed` line as changelog-speak in an operator-facing README; **I sided with hoodie**
  (README ≠ the contributor doc that CLAUDE.md is): dropped the label + the "no behavior changed" meta from
  README, kept only the operator reassurance; that meta lives in CLAUDE.md (T4) where the audience is the
  contributor. **Where I sided with neckbeard instead:** hoodie wanted the fixrate caveats all framed as
  "honest absence, not zero", but neckbeard correctly noted only the `n>1` case is a null-vs-zero claim —
  `--disable-log-stats` is a hard startup rejection and the CPU-overhead note is a perf caveat — so I
  scoped the "never a fabricated zero" phrasing to the `n>1` bullet only. **One hoodie override:** hoodie
  wanted the `rg --no-ignore` note dropped as an author's aside; I kept a tightened version because the
  pointer targets a gitignored tree and the hint serves the *reader* (it is exactly the P3 zero-hit trap).
- **(T5) Plan Verify self-inconsistency, not a failure.** The plan's T5 discriminator
  `grep '\*\*vLLM\*\*' README.md → 1 after` is unreachable by the plan's own contract: its drafted intro
  (line 250) *also* bolds `**vLLM**`, so after intro+bullet the count is **2**. Both hits are correct
  (L6 intro, L802 bullet). Recorded rather than chased; the real check (the API-Compatibility section gains
   a vLLM bullet) passed at L802 with `max_model_len`/`cache_config_info` content present.

### T6 triage table (every inventory hit → disposition; Rule A grep, plan:287-289 verbatim)

| Hit | Disposition |
|---|---|
| `README.md:106` `# Single backend llama-server location` | **Fixed** → "the example URL below is llama.cpp" (the config sample genuinely targets llama.cpp `:8080`, so naming it is accurate, not a target claim) |
| `README.md:202` `Point … at the proxy instead of llama.cpp directly` | **Fixed** → "instead of the backend directly" (hoodie-flagged; not enumerated by the plan's grep) |
| `CLAUDE.md:255` `# …point it at proxy instead of llama.cpp directly` | **Fixed** → "the backend directly" (curl-recipe comment) |
| `CLAUDE.md:291` `backend.host/port: llama-server location (default: localhost:8080)` | **Fixed** → `backend.url` + "the default config targets llama.cpp at `http://localhost:8080`" (hoodie/neckbeard: `BackendConfig` has `url` only, `config/mod.rs:101` — the old key name was a doc bug; neckbeard's "no `http://` under a host/port key" moot once key→`url`) |
| `e2e/README.md:3` `the **backend** (llama.cpp server)` | **Fixed** → "an OpenAI-compatible mock; its default payloads are llama.cpp-shaped" (hoodie: suite tests the *proxy*; neckbeard: keep the llama.cpp-shaped-defaults accuracy — both honored) |
| `e2e/README.md:182` `(what \`llama.cpp\` would return)` | **Fixed** → "(what an OpenAI-compatible backend would return)" — twin of :3, kept consistent |
| `CLAUDE.md:282` `llama.cpp server API: vendor/llama.cpp/tools/server/` | **Leave** — accurate pointer to the real read-only checkout; both reviewers said do not touch (T4 owns it) |
| `CLAUDE.md:203` `Supports CLI overrides for port, backend host/port` | **Leave** — CLI-override prose (`--port`/`--backend-url`), not a "llama.cpp is the target" claim; surfaced only as a `.`-regex false positive (fixed-string `grep -F` → 0) |
| `config.yaml.default:19/21` `llama-server` | **Follow-up** — non-`.md`, so outside the plan's `--include='*.md'` grep by design; it is what operators `cp` → highest-value mirror, logged below |
| `src/main.rs:47` `about = "HTTP reverse proxy for llama.cpp server"` | **Follow-up** (pre-existing) — code / observable `--help`, out of a docs plan; not matched by the `.md` grep |

- **(T6) Both reviewers redirected the drafted banner; every reviewer source-claim was ground-truthed
  before baking (the T4 gitignore-artifact lesson).** neckbeard: **drop the drafted "Superseded by Plan
  01"** — topic mismatch (Plan 01 is stats-columnar/prefill; PLAN.md is streaming-delta), the true cause
  of obsolescence is the `21f0548` deletion, not a stats plan. hoodie: **front-load "DO NOT IMPLEMENT"**
  (a bare "obsolete" invites an agent to mine the file for a plausible task), cover the **"Changes Made"
  false-completion**, and add one honest line that the **Unicode `filePath` bug is still live** so the
  stamp does not over-generalize to "ignore everything here". All adopted. Verified before writing: the
  three symbols the banner calls deleted → **0 hits in `src/`**; `find_string_end` **live @:231** (byte-
  slicing bug real, tracked by the e2e `toolcall/bad_filepath/special_chars` KNOWN BUG @`e2e/README.md`
  :238-242); `21f0548`="big-fix 29: delete dead legacy streaming machinery". The live streaming section
  is cited **descriptively** ("streaming-fixes guidance in `CLAUDE.md`"), **not** as a verbatim quote —
  hoodie flagged the real heading carries "(Important!)" @:341, so a paraphrase quoted-as-verbatim is the
  rot-to-avoid. **Rejected a reviewer false-positive:** hoodie's `safe_completion` "still referenced"
  worry — `grep -F 'backend.host/port'` and the symbol greps showed the only "survivor" was `CLAUDE.md:203`
  matched by the `.` regex wildcard (a *space*: "backend host/port"), a CLI-override line, not the L291
  config key I fixed → left untouched, out of scope.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Update CLI `about`/`long_about` strings (`src/main.rs:47`) + `main.rs:1`/`lib.rs:1` doc comments to vLLM-first | Observable `--help` behavior; code change, out of a docs plan's scope | a later code plan |
| Delete `PLAN.md` after human review (currently OBSOLETE-stamped) | Deletion is a human call; append-only until then | next resync / human |
| Consider a "which backend gives me what" table (`/props`, `/slots`, `timings`, `metrics`, `vllm:cache_config_info`, `max_model_len`) in README | Single table answers most vLLM-first positioning questions | optional README follow-up |
| **Fix the gate**: `justfile:16` + `RULES.md:122/124` use `cargo … -- -D warnings`, rejected by this cargo (`unexpected argument '-D' found`); `just gate` cannot pass as written | Reproducibility: every task's gate claim rests on this command; workaround is `RUSTFLAGS='-D warnings' cargo …` | separate bookkeeping commit (authoritative file — human sign-off first) |
| Fix root `CLAUDE.md` "Reference Implementation" `../vendor/claude-code`, `../vendor/opencode`, `../vendor/llama.cpp` — resolve to a **nonexistent** `local-mind/vendor/`; real checkouts are `llama-proxy/vendor/{vllm,llama.cpp}` | Dead pointers misroute discovery work; P3 now warns against them | **LANDED T4** — `../vendor/`→0; checkouts now `vendor/…` (no `../`), client code → upstream "not vendored here", llama.cpp→`tools/server/` |
| Resolve the 3× dead `../context/opencode_claude_llama_notes.md` reference (`CLAUDE.md` L93/L284/L404) — `find` shows the file exists **nowhere** (deleted, not mis-pathed), so no prefix swap fixes it | Cited as "the comprehensive client notes"; a dead pointer an agent will trust and chase. Needs a content call (restore the file / rewrite the refs / drop them), not a path edit | human decision — all 3 refs are outside the hands-plan block, safe to edit once decided |
| Neutralize `config.yaml.default:19/21` (`# Backend llama-server configuration`, `# Full URL to llama-server`) | Mirror drift: README/CLAUDE are now engine-neutral but the file an operator actually `cp`'s still names llama-server; flagged by **both** reviewers (neckbeard MEDIUM, hoodie "highest-value"); non-`.md`, so out of this plan's `--include='*.md'` grep by design | a later docs plan — bundle with the `src/main.rs:47` CLI-`about` row (both are the shipped default-config surface an operator sees first) |
| Deepen the e2e mock beyond `:3`/`:182`: default `/props` + `/slots` payloads stay llama.cpp-shaped (`.gguf`, `n_ctx`) | neckbeard's accuracy caveat vs hoodie's genericize — the *proxy* is under test and the doc is now engine-neutral, but an engine-neutral default payload (or a vLLM-shaped sample) is a harness change, not a doc fix; left to avoid a reviewer-verified accuracy loss | a later e2e plan — touches `e2e/src/backend.rs` default handlers (= code, out of a docs plan) |
