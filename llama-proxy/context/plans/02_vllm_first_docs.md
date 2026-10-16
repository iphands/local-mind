# Plan 02 — VLLM-first docs + read-only vendor discovery space

> **Status**: in-progress
> **Created**: 2026-10-16
> **Depends on**: N/A (Plan 01 is `done`; this plan only cites its `fixrate.md`)
> **Goal**: Re-frame the project's own markdown to lead with **vLLM** (still naming llama.cpp and any OpenAI-compatible backend), and make `./vendor/*` a gitignored, read-only discovery space with written rules for using it.
> **Agent**: implementation agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: The proxy serves vLLM first, but every project doc opens "reverse proxy for **llama.cpp** server". Re-order the framing to vLLM-first — naming llama.cpp and any OpenAI-compatible server as fully supported — and formalise `./vendor/{vllm,llama.cpp}` as a gitignored, read-only tree the agent reads to learn how backends behave, with the rules for doing so written down.

**Deliverables**:
1. `context/plans/RULES.md` gains a **P3** project rule: vendor sources are read-only discovery space (cite `path:line`, never patch, `rg --no-ignore` escape hatch).
2. `context/CLAUDE.md` KB map gains a `vendor/` row + a "backend-discovery facts" routing row, so the next agent knows the tree exists and where its findings go.
3. `.gitignore` gains `/vendor/` (root-anchored), with the ignore's effect *observed*, not assumed.
4. Root `CLAUDE.md` (=`AGENTS.md` symlink) and `README.md` re-framed vLLM-first, with a **preservation list** so no load-bearing per-backend fact is deleted.
5. `fixrate.md` narrowed to add the how-to-get-the-split recipe + a `vendor/vllm` source pointer (it already names the vLLM metrics path).
6. A triage pass over tracked docs: stale/llama.cpp-only claims fixed or filed as Follow-ups; root `PLAN.md` (plans fixes for machinery deleted in 21f0548) stamped `OBSOLETE`.

**Estimated effort**: Small–Medium (half day) — one file per task, prose re-ordering + a rules section + a gitignore line; the only real risk is deleting a fact that was load-bearing.

---

## Scope

**In**: `.gitignore`; root `CLAUDE.md`/`AGENTS.md`; `README.md`; `fixrate.md`; `context/CLAUDE.md`/`AGENTS.md`; `context/plans/RULES.md` (P3 addendum only); `PLAN.md` (obsolete-stamp only).

**Out** (record a blocker/Follow-up rather than expanding):
- **Any `src/` change.** The CLI `about = "HTTP reverse proxy for llama.cpp server"` (`src/main.rs:47`) and module docs will lag the vLLM-first docs — this is a **known deviation**, filed as a Follow-up, not silently edited (a code change drags in the gate for a docs plan, and `--help` output is observable behavior).
- **Renaming the crate/binary** (`llama-proxy`, `src/api/llama.rs`). The name is historical; vLLM-first is positioning, not a rename.
- **Rewriting backend-detection code** (`preflight.rs` `is_llama_cpp`, `context.rs` `/props`-vs-`/v1/models` split, `collector.rs` `timings` comments, `kv_labels.rs`/`compat.rs` fixtures). These name llama.cpp because the *behavior* is llama.cpp-specific — that is accuracy, not framing. **Do NOT "vLLM-first" them.**
- **Fabricating metrics examples.** The README example boxes may switch the model *name* to a vLLM-style one, but throughput numbers stay illustrative and labelled — never invent a measured figure (P1).

If the work must cross this line, **stop** and record the blocker in the tracker.

---

## Context

The north star in `SERIES.md` already says "an OpenAI-compatible reverse proxy for LLM **backends (VLLM, llama.cpp)**" and the operator's directive is "FIRST AND FOREMOST build for a VLLM background". But the docs still read llama.cpp-first because the proxy was built against llama.cpp. That mismatch costs real money: the metrics an operator reads (`tps=`, `context_total`) are backend-dependent, and an operator on vLLM who reads llama.cpp-first docs will misread `tps>=` as a vLLM regression and miss the one flag (`--enable-per-request-metrics`) that turns it into a measured split.

Separately, `./vendor/{vllm,llama.cpp}` is a full upstream checkout the operator dropped in to *learn* how the backends behave. It is currently untracked and un-ignored, and there is no rule telling an agent it is read-only or how to search it (a gitignored tree is invisible to ripgrep by default). Formalising it turns "go read vLLM source" from tribal knowledge into a cited, repeatable move.

### Pre-Identified Bug/Issue

Not a bug — this is the command showing today's llama.cpp-first framing and the vendor gap, verbatim (run 2026-10-16):

```bash
$ grep -n 'reverse proxy for' README.md CLAUDE.md
README.md:6:An HTTP reverse proxy for [llama.cpp](https://github.com/ggerganov/llama.cpp) server that fixes malformed LLM responses, ...
CLAUDE.md:7:`llama-proxy` is an HTTP reverse proxy for llama.cpp server that:

$ grep -n 'llama.cpp server / OpenAI' README.md
391:│          llama.cpp server / OpenAI-compatible API           │

$ git ls-files vendor/ | wc -l      # vendor/ is present on disk but untracked
0
$ cat .gitignore
target/
/captures/

$ grep -n 'calculate_completion_delta\|apply_stream' PLAN.md | head -3
9:... the `calculate_completion_delta` function has complex logic ...
20:... the `apply_stream_with_accumulation_default` method ...   # both DELETED in 21f0548
```

### Why [Approach]

**Re-order + preserve**, not rewrite. Rejected alternative: a from-scratch vLLM-first rewrite of each doc. It lost because the docs carry load-bearing per-backend facts (llama.cpp `timings` is the only source of a measured prefill/decode split; `/props`+`/slots` are llama.cpp-only; the prefix-cache prefill correction) — a rewrite drops them, and the README *already* documents them correctly. We only flip which backend is named first and add the vLLM flag; the "Preservation List" below is the contract.

**Rules before gitignore** (T1/T2 before T3). Rejected alternative: gitignore first, document later. It lost because once `/vendor/` is ignored, ripgrep-based search silently skips it — the rules must carry the `rg --no-ignore` escape hatch, and writing them first means the author hits the blindness instead of inheriting it.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| Next free plan number | `02` | `hp-scan --brief` `NEXT computed=02` on 2026-10-16 |
| `AGENTS.md` is a symlink → `CLAUDE.md` (root **and** `context/`) | one edit covers both | `ls -l AGENTS.md context/AGENTS.md` on 2026-10-16 |
| `.gitignore` currently only `target/`, `/captures/` | 2 lines, no vendor | `cat .gitignore` on 2026-10-16 |
| `vendor/` untracked | `git ls-files vendor/` → empty; `git status` → `?? vendor/` | 2026-10-16 |
| Environment `rg` **did** list a gitignored tree when passed explicitly (`rg --files target` printed `target/...`) | so blindness is not guaranteed *here*, but the escape hatch is portable insurance | `rg --files target` on 2026-10-16 |
| README `### Is it really streaming?` + "streaming docs in README.md" are cited **by heading from `CLAUDE.md`** | do not rename those headings | grep 2026-10-16: `grep -n 'Is it really streaming' CLAUDE.md` hits. `src/proxy/handler.rs` has **only** a generic "the README/AGENTS client notes" comment at `:3002` — it is NOT a heading citation, so the heading guard is a `CLAUDE.md` fact, not a `src/` one |
| `fixrate.md` already names the vLLM split path | lines ~47 (Anthropic adapter strips `metrics`), ~387 (`--enable-per-request-metrics`), ~411 (`vllm:generation_tokens_total / vllm:request_decode_time_seconds_sum`) | grep on 2026-10-16 → T5 is a **narrow**, not a new claim |
| `src/main.rs:47` `about = "HTTP reverse proxy for llama.cpp server"` | user-facing `--help` string, lags vLLM-first docs | grep on 2026-10-16 → Follow-up, out of scope |

### Rejected Claims

- **"PLAN.md needs its bug fixes implemented."** No — `calculate_completion_delta` and `apply_stream_with_accumulation_default` were **deleted in 21f0548** (`CLAUDE.md` "Streaming Fixes: No Per-Chunk Delta Calculation"). PLAN.md is stale, not a backlog. It gets an `OBSOLETE` stamp (T6), not execution.
- **"Make the docs say vLLM everywhere, including code."** No — backend-detection names llama.cpp because the behavior is llama.cpp-specific. Rewriting those is a correctness regression dressed as framing.
- **"The vLLM `tps=A/B` split might be a derived number dressed as measured."** No — verified 2026-10-16 at `src/stats/collector.rs:490-502`: `prompt_tps` comes from `time_to_first_token_ms` and `generation_tps` from `generation_time_ms` (vLLM's own decode-only interval, excluding queue + prefill — documented at `:469-472`), **never** from the prefill-inclusive `tokens_per_second`; `has_timing_split` is set only when a backend timing is finite (`:503`). So the "measured" label is honest for vLLM — do NOT "fix" the collector.
- **"Add a `vendor/README.md` to explain the convention."** No — it lives inside the ignored tree, so it is invisible to every clone and every gitignore-respecting tool. The convention goes in a **tracked** path (T1/T2).

---

## Step-by-Step Tasks

Ordering: **T1 → T2 → T3** (write the vendor rules before you make the tree invisible to grep), then T4, T5, T6 (independent docs). Each task: doc-shaped Verify **and** the project gate. The gate is necessary but, for markdown, not sufficient — Rule A demands you *observe* the doc, so each Verify greps for the specific claim flipping.

### T1: Add the vendor-discovery P3 rule to `context/plans/RULES.md`

**File**: `context/plans/RULES.md`

**What to do**: Append a `### P3 — Vendor sources are read-only discovery space` subsection to the **Project Rules** section (`## Project Rules`, after P2, before `## Series-Scoped Rules`). Content contract:
- `vendor/{vllm,llama.cpp}` are upstream checkouts for **reading only**: never edit, never commit, never `git add`. They are gitignored (T3), so a stray edit is silently lost — treat them as read-only regardless.
- Cite backend behavior as `vendor/<project>/<path>:<line>`, same as any other source (`path:line`).
- **Escape hatch (verbatim)**: gitignored trees are skipped by ripgrep by default — `rg --no-ignore -n '<symbol>' vendor/vllm/`, or `grep -r --include='*.py'`. State this so a future agent does not conclude "vLLM has no such code" from a silent zero-hit search.
- A backend-behavior fact learned from vendor goes to `context/` (routing row added in T2), tagged `[REPO]` with the `path:line`.

**Before**: *(new subsection — file ends P2 with no P3; `## Project Rules` section ends at the `---` before `## Series-Scoped Rules`)*

**After**:
```markdown
### P3 — Vendor sources are read-only discovery space

`vendor/vllm` and `vendor/llama.cpp` are upstream checkouts dropped in to learn how the backends
actually behave. They are **read-only**: never edit, never `git add`, never commit. They are
gitignored (Plan 02), so an edit there is silently lost.

1. **Cite them like any source**: `vendor/vllm/<path>:<line>`.
2. **gitignored trees are invisible to ripgrep by default** — search them explicitly:
   `rg --no-ignore -n '<symbol>' vendor/vllm/`. A zero-hit search is not evidence the code is
   absent; re-run with `--no-ignore` before concluding.
3. A backend-behavior fact learned here goes in `context/` tagged `[REPO]` with its `path:line`.
```

**Verify**:
```bash
grep -n 'P3 — Vendor sources are read-only' context/plans/RULES.md   # → 1 match
grep -n -- '--no-ignore' context/plans/RULES.md                       # → match (escape hatch present)
cargo check --workspace --features influxdb -- -D warnings && cargo clippy --all-targets --features influxdb -- -D warnings && cargo fmt --check && cargo test --workspace --lib
```
Expected: the two greps hit (the rule + the escape hatch both landed); gate exits 0.

**Commit**: `[llama-proxy][P2][T1][vendor-rules] add P3 read-only vendor discovery rule` — *tracker row in the same commit (Rule B).*

### T2: Register vendor in the `context/` KB map

**File**: `context/CLAUDE.md` (`context/AGENTS.md` is a symlink to it — one edit covers both; edit `CLAUDE.md`).

**What to do**: Two additions, no deletions:
1. In the **Map** table, a row: `vendor/vllm`, `vendor/llama.cpp` | upstream backend sources, **read-only + gitignored** | learning exact vLLM/llama.cpp behavior (metrics, `/props`, gauges); search with `rg --no-ignore`.
2. In the **Where Findings Go** table, a row: "A backend-behavior fact from vendor/*" → `distilled.md` (or the topic file) | `[REPO]` + `path:line`.

**Before**: *(Map table currently lists only `plans/*` rows; Where Findings Go lists confirmed-fact/bug/dependency/progress rows — no vendor row)*

**After**: *(two new table rows as described; every existing row untouched; the file's leading
skill stamp and its section markers stay in place — this task only adds rows, it deletes and
renames nothing)*

**Verify**:
```bash
grep -n 'vendor/vllm' context/CLAUDE.md                       # → ≥1 match (Map row present)
grep -n 'Read-only\|read-only' context/CLAUDE.md              # → match
diff -q context/AGENTS.md context/CLAUDE.md && echo SYMLINK-OK # → SYMLINK-OK (still same file)
cargo check --workspace --features influxdb -- -D warnings && cargo clippy --all-targets --features influxdb -- -D warnings && cargo fmt --check && cargo test --workspace --lib
```
Expected: vendor row present, still-symlink confirmed, gate exits 0.

**Commit**: `[llama-proxy][P2][T2][docs] register vendor discovery in context KB map` — *tracker row in the same commit.*

### T3: Gitignore `/vendor/` (and observe the ignore took)

**File**: `.gitignore`

**What to do**: Add a root-anchored `/vendor/` line (root-anchored to match the existing `/captures/` style and avoid ignoring a future first-party `src/vendor/`). Add a short comment naming it the read-only learning tree.

**Before**:
```
target/
/captures/
```

**After**:
```
target/
/captures/
# Read-only upstream backend sources for learning behavior (Plan 02 / RULES P3). Search with
# `rg --no-ignore`; never edit, never commit.
/vendor/
```

**Verify**:
```bash
# Authoritative: the git ignore is the fact under test (N = whatever line /vendor/ landed on).
git check-ignore -v vendor/vllm/docs/features/per_request_metrics.md   # → `.gitignore:<N>:/vendor/	<that path>` (rule matches)
git status --porcelain vendor/ ; echo "exit=$?"   # → (empty) exit=0 — vendor no longer shows as ??
# OBSERVE the grep blindness — record whatever number you get; do NOT assert 0. An explicit path
# like `vendor` bypasses the PARENT .gitignore (exactly why Key Facts saw `rg --files target` list
# an ignored tree), so a non-zero count there is EXPECTED. Only the ROOT traversal proves blindness.
echo "RIPGREP_CONFIG_PATH=${RIPGREP_CONFIG_PATH:-unset}"   # → explains rg's ignore behaviour here
rg --files 2>/dev/null | grep '^vendor/' | wc -l          # → 0 (a normal root search skips the ignored tree; wc -l exits 0, unlike grep -c)
rg --no-ignore --files vendor | wc -l                       # → > 0 (escape hatch still reads the tree)
cargo check --workspace --features influxdb -- -D warnings && cargo clippy --all-targets --features influxdb -- -D warnings && cargo fmt --check && cargo test --workspace --lib
```
Expected: `git check-ignore` matches and `git status --porcelain vendor/` is empty — **those two are the pass/fail**. The `rg` numbers are recorded as an observation in the Evidence table (not asserted).

**Expected observation**:
- **Confirms if**: `check-ignore` prints the `/vendor/` rule AND `git status --porcelain vendor/` is empty (tree git-ignored); and `rg --files | grep -c '^vendor/'` = 0 while `rg --no-ignore --files vendor` > 0 (invisible to a normal search, readable with the flag).
- **Refutes if**: `git status --porcelain vendor/` still lists `?? vendor/` (the ignore did not apply — check the anchor / a trailing-space typo), **or** `git check-ignore -v` prints no `/vendor/` rule even though `git status` is empty (ignored via `.git/info/exclude` or a global `core.excludesFile`, not by the file we just edited — a false pass).
- **Within noise if**: `rg --files vendor` (explicit path) lists files — expected, an explicit arg bypasses the parent ignore; only the **root** `rg --files` skipping `vendor/` is the blindness signal.

**Commit**: `[llama-proxy][P2][T3][gitignore] ignore read-only /vendor tree` — *tracker row in the same commit.*

### T4: Re-frame root `CLAUDE.md` (=`AGENTS.md`) vLLM-first

**File**: `CLAUDE.md` (root; `AGENTS.md` is a symlink to it — do **not** edit `AGENTS.md` separately).

**What to do**: Re-order framing to vLLM-first across the Project Overview and Client Compatibility, without deleting any fact:
- Line 7 `### `llama-proxy` is an HTTP reverse proxy for llama.cpp server that:` → lead with **vLLM**, then "…or llama.cpp, or any OpenAI-compatible server". Keep the bullets (proxy / fix / metrics / export).
- Line 15 `The proxy sits between **llama.cpp server** and AI client tools` → "between **an LLM backend (vLLM, llama.cpp, or any OpenAI-compatible server)** and AI client tools".
- The `✅ **llama.cpp Extensions**` heading stays (it is accurate — those endpoints are llama.cpp-specific). Under it, add a sentence naming vLLM as the primary target and its real data sources — e.g.: *"The proxy is written for vLLM first: it reads vLLM's per-request `metrics` block (server started with `--enable-per-request-metrics`) for the measured prefill/decode split and `/v1/models` `max_model_len` for the context window (`src/proxy/context.rs`); the llama.cpp `timings`/`/props`/`/slots` path above is the equivalent for llama.cpp backends."*
- Add one **"Framing note (2026-10-16)"** line near the top: vLLM is the primary documented backend; llama.cpp and any OpenAI-compatible server remain fully supported; **no behavior changed** — this is documentation re-ordering (Plan 02); the `llama-proxy` name is historical, a rename is out of scope.

**Preservation List** (may move in priority order, must NOT be deleted): three-tier `tps=` explanation; "absent ≠ measured zero"; the prefix-cache prefill correction; the `/props`→`/v1/models` `context_total` provenance; llama.cpp-only `timings`/`/props`/`/slots`; the vram-estimate shim body-shape rules; the honest-local-answers principle.

**Before** (`CLAUDE.md:7`, `:15`):
```
`llama-proxy` is an HTTP reverse proxy for llama.cpp server that:
...
The proxy sits between **llama.cpp server** and AI client tools, supporting:
```

**After**: vLLM-led equivalents per "What to do"; the `<!-- hands-plan:begin v1 --> … <!-- hands-plan:end -->` block at the file's bottom is **untouched**.

**Verify**:
```bash
grep -n -i 'vllm' CLAUDE.md | head -1                          # → FIRST vLLM hit is now in the Project Overview (a line well before the pre-existing vLLM mentions at :52-53)
grep -n 'written for vLLM first\|vLLM (and llama.cpp\|proxy for vLLM' CLAUDE.md   # → the new vLLM-first framing sentence(s) present
grep -n 'reverse proxy for llama.cpp server' CLAUDE.md; echo "exit=$?"  # → no match, exit=1 (old line gone)
grep -c 'hands-plan:begin v1' CLAUDE.md                         # → 1 (block begin intact)
grep -c 'hands-plan:end' CLAUDE.md                              # → 1 (block end intact — the whole block survived, not just its opening)
ls -l AGENTS.md                                                 # → `AGENTS.md -> CLAUDE.md` (STILL a symlink; `diff -q` alone cannot prove this)
cargo check --workspace --features influxdb -- -D warnings && cargo clippy --all-targets --features influxdb -- -D warnings && cargo fmt --check && cargo test --workspace --lib
```
Expected: the new vLLM-first sentences are present and the first `vllm` grep hits in the Overview (not just the pre-existing :52-53 mentions), the old exact llama.cpp-only line is gone, both hands-plan markers survive, `AGENTS.md` is still a symlink, gate exits 0.

**Commit**: `[llama-proxy][P2][T4][docs] make root CLAUDE.md vLLM-first` — *tracker row in the same commit.*

### T5: Re-frame `README.md` vLLM-first + narrow `fixrate.md`

**Files**: `README.md`, `fixrate.md` (both framing only; keep them one commit — the fixrate change is one cross-reference sentence that rides the same "vLLM is primary" theme; if fixrate grows beyond that, split it into its own commit).

**What to do (README.md)**:
- Line 6 intro: lead with vLLM — "An HTTP reverse proxy for **vLLM** (and llama.cpp or any OpenAI-compatible server) that fixes malformed LLM responses…". Keep the badges.
- Line 391 diagram backend cell: `llama.cpp server / OpenAI-compatible API` → `vLLM / llama.cpp / OpenAI-compatible API`.
- Ensure `--enable-per-request-metrics` is named where the measured `tps=A/B` split is explained (highest-UX-value line — without it a vLLM operator reads `tps>=` and thinks the proxy is slow). It already appears at **`README.md:256` and `:308`**; confirm both stay prominent.
- **`## API Compatibility` (README.md:795)** lists `**llama.cpp Extensions**: timings, /props, /slots` (`:800`) with **no vLLM counterpart** — add a `**vLLM**` bullet: `metrics` (requires `--enable-per-request-metrics`), `/v1/models` `max_model_len`, and the `vllm:cache_config_info` gauge the shim scrapes. This is the most-scanned "does vLLM work?" block; a vLLM-first intro that leaves it llama.cpp-only is positioning without substance.
- Add a one-line **behavior-unchanged note** near the intro (mirrors T4's framing note).
- **Do NOT rename** the `### Is it really streaming?` heading (cited verbatim from `CLAUDE.md`). Example model *names* may switch to a vLLM-style id; **throughput figures stay illustrative** — do not fabricate measured numbers (P1).

**What to do (fixrate.md)**: it already names the vLLM split path (Key Facts). Add a single cross-reference sentence pointing an operator at how to *get* the split and citing the producer doc `vendor/vllm/docs/features/per_request_metrics.md` (`rg --no-ignore` to read it) — that doc carries three operator caveats worth one line each: the flag is rejected when `--disable-log-stats` is also set, it adds CPU cost at high concurrency, and `metrics` is `null` for `n > 1` (an honest-absence case matching the "absent ≠ zero" rule). Do not restate what fixrate already has.

**Verify**:
```bash
grep -n -i 'proxy for vllm\|for \*\*vLLM\*\*' README.md | head   # → intro now leads vLLM
grep -n 'proxy for \[llama.cpp\]' README.md; echo "exit=$?"       # → no match, exit=1 (old intro gone)
grep -n 'vLLM / llama.cpp / OpenAI' README.md                     # → diagram cell updated
grep -n 'Is it really streaming?' README.md                       # → heading unchanged
grep -n -- '--enable-per-request-metrics' README.md fixrate.md    # → present (README :256 & :308; fixrate too)
# vLLM-first substance: the API Compatibility section gains a vLLM bullet beside llama.cpp's:
grep -n 'llama.cpp Extensions' README.md                          # → :800, llama.cpp bullet still present
grep -n '\*\*vLLM\*\*' README.md                                  # → 0 today, 1 after: the new bullet's bold label (discriminator)
grep -n 'vLLM' README.md | grep -i 'max_model_len\|cache_config_info'   # → secondary content check (NOTE: pre-matches :256/:308/:943, so NOT the proof on its own)
# PRESERVATION (the load-bearing README facts MUST survive the re-order — this is the Risk #1 guard):
grep -n 'Prefill TPS is corrected' README.md                      # → prefix-cache correction paragraph present
grep -n 'tps>=' README.md | head -1                               # → three-tier table intact
grep -n 'not "zero cached"\|absent is' README.md                   # → "absent ≠ measured zero" wording present
cargo check --workspace --features influxdb -- -D warnings && cargo clippy --all-targets --features influxdb -- -D warnings && cargo fmt --check && cargo test --workspace --lib
```
Expected: README intro + diagram lead vLLM, old `[llama.cpp]` intro gone, cited heading preserved, flag named at both sites, the new vLLM API-Compatibility bullet present, **all three Preservation greps hit**, gate exits 0.

**Commit**: `[llama-proxy][P2][T5][docs] make README/fixrate vLLM-first` — *tracker row in the same commit.*

### T6: Triage tracked docs + stamp stale `PLAN.md` obsolete

**File(s)**: `PLAN.md` (stamp) + any tracked `.md` the grep surfaces (fix or file a Follow-up per hit).

**What to do**:
1. Inventory:
   ```bash
   grep -rn -i 'llama\.cpp server\|llama-server\|reverse proxy for llama' --include='*.md' . \
     --exclude-dir=vendor --exclude-dir=target --exclude-dir=.git \
     | grep -v '^\./context/plans/completed/\|^\./PLAN\.md\|^\./context/plans/02_vllm_first_docs'
   ```
   `--exclude-dir=vendor` also stops the search walking the multi-hundred-MB vendored checkouts (never in scope). **Never let this grep match *this plan file*** — its Before/After blocks quote the old framing *deliberately* as evidence; excluding it is not a false negative, and the executor must NOT "fix" the plan's own quoted text to force a pass. For each remaining hit not already handled by T4/T5: fix the framing or file a Follow-up row (e.g. the README config-sample comment `# Single backend llama-server location` — decide whether T5 owns it or it is a Follow-up).
2. `PLAN.md`: prepend an `> **[OBSOLETE — 2026-10-16]** …` banner stating it plans fixes for `calculate_completion_delta` / `apply_stream_with_accumulation_default`, deleted in 21f0548; point to `context/plans/completed/01_stats_columnar_prefill.md`. Do **not** delete the file (append-only ethos; deletion is a human call — file it as a Follow-up to remove after review).
3. Record the triage table (every hit + disposition) in the tracker.

**Before** (`PLAN.md:1`):
```
# Plan for llama-proxy Bug Fixes
```

**After**:
```
> **[OBSOLETE — 2026-10-16]** Plans fixes for `calculate_completion_delta` /
> `apply_stream_with_accumulation_default`, both deleted in 21f0548 (see CLAUDE.md "Streaming
> Fixes"). Superseded by Plan 01. Retained for history only.

# Plan for llama-proxy Bug Fixes
```

**Verify**:
```bash
grep -n 'OBSOLETE' PLAN.md                                     # → banner present
grep -rn -i 'reverse proxy for llama' --include='*.md' . \
  --exclude-dir=vendor --exclude-dir=target --exclude-dir=.git \
  | grep -v '^\./context/plans/completed/\|^\./PLAN\.md\|^\./context/plans/02_' | wc -l  # → 0 (the plan file itself is excluded — it quotes the old framing as evidence)
cargo check --workspace --features influxdb -- -D warnings && cargo clippy --all-targets --features influxdb -- -D warnings && cargo fmt --check && cargo test --workspace --lib
```
Expected: PLAN.md stamped; no tracked doc still claims llama.cpp is *the* target backend except documented Follow-ups; gate exits 0.

**Commit**: `[llama-proxy][P2][T6][docs] triage docs; stamp stale PLAN.md obsolete` — *tracker row in the same commit.*

---

## Critical Files

| File | Change | Priority |
|---|---|---|
| `context/plans/RULES.md` | Add P3 read-only vendor rule + `rg --no-ignore` escape hatch | P0 |
| `.gitignore` | Add `/vendor/` (root-anchored); observe ignore took | P0 |
| `context/CLAUDE.md` | Map row + routing row for vendor discovery | P1 |
| `CLAUDE.md` (root, =`AGENTS.md` symlink) | vLLM-first overview; hands-plan block untouched | P1 |
| `README.md` | vLLM-first intro + diagram; add `## API Compatibility` vLLM bullet; keep cited headings; flag `--enable-per-request-metrics`; Preservation greps | P1 |
| `fixrate.md` | One cross-reference to the split recipe + vendor pointer (already names the path) | P2 |
| `PLAN.md` | `OBSOLETE` banner (stale — targets deleted machinery) | P2 |

---

## Open Questions / Risks

1. **Risk: the vLLM-first rewrite deletes a load-bearing llama.cpp fact** (`timings`, `/props`, `/slots`, prefix-cache correction, the three-tier `tps=` table). *Mitigation*: the **Preservation List** in T4 is the contract; the load-bearing facts live in `README.md`, so **T5's Verify greps each one** (prefix-cache paragraph, `tps>=` table, "absent is not zero cached") after the re-order — the contract is a checkable command, not prose.
2. **Risk: gitignoring `/vendor/` blinds a future agent's grep into a false "vLLM has no such code."** *Mitigation*: T1 writes the `rg --no-ignore` rule **before** T3 ignores the tree; T3's Verify records the root-traversal blindness (`rg --files | grep -c '^vendor/'`=0 vs `--no-ignore`>0) in Evidence.
3. **Risk: editing `AGENTS.md` instead of `CLAUDE.md` forks the two agent docs.** *Mitigation*: Key Facts confirm `AGENTS.md` is a symlink → `CLAUDE.md`; tasks edit `CLAUDE.md`, and T4's Verify runs `ls -l AGENTS.md` (a symlink check — `diff -q` alone passes even after `sed -i` silently replaces the symlink with a regular file).
4. **Question: does the CLI `about` string (`src/main.rs:47`) get the vLLM-first treatment too?** *How we'll settle it*: **deferred — out of scope** (a code change pulls the gate into a docs plan and `--help` is observable behavior). Filed as a Follow-up for a later code plan; this plan records the deviation.
5. **Question: delete `PLAN.md` or just stamp it?** *How we'll settle it*: stamp now (T6); deletion is a **human** call — Follow-up row.

---

## Verification Checklist

- [x] T1: `grep 'P3 — Vendor sources are read-only' RULES.md` → 1 match; `grep -- '--no-ignore'` → hit; gate exits 0
- [x] T2: `grep 'vendor/vllm' context/CLAUDE.md` → ≥1 match; `diff -q context/AGENTS.md context/CLAUDE.md` → SYMLINK-OK; gate exits 0
- [x] T3: `git check-ignore -v vendor/vllm/docs/features/per_request_metrics.md` → `/vendor/` rule AND `git status --porcelain vendor/` empty (the pass/fail); `rg --files | grep -c '^vendor/'`=0 and `rg --no-ignore --files vendor`>0 recorded as observation; gate exits 0
- [ ] T4: new vLLM-first sentence present + first `vllm` hit in Overview; `grep 'reverse proxy for llama.cpp server' CLAUDE.md` → no match; BOTH hands-plan markers present; `ls -l AGENTS.md` shows `-> CLAUDE.md`; gate exits 0
- [ ] T5: README intro + diagram lead vLLM; `Is it really streaming?` heading intact; flag named at :256 & :308; `grep '\*\*vLLM\*\*' README.md` = 1 (the new API-Compatibility bullet — the discriminating check); **all three Preservation greps hit**; gate exits 0
- [ ] T6: PLAN.md `OBSOLETE` present; no un-triaged `reverse proxy for llama` hit; gate exits 0
- [ ] All: the project gate (Rule A) passes on the final commit
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
