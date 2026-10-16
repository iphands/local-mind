# Plan 03 — columnar compact stats + honest vLLM prefill — Tracker

## Overview

- Status: 43% complete (3/7 tasks)
- Start date: 2026-10-16
- Plan: `context/plans/03_columnar_stats_honest_prefill.md`
- Evidence: `captures/vllm_metrics_2026-10-16_*.json` (gitignored, RULES P2) for the backend payload;
  rendered compact blocks and `cargo test` output for the formatter; the corrected-rate table below.
  Test environment: live vLLM `0.33.0.dev20261015+g4d9a8d6` at `cosmo.lan:8700`
  (`model=cosmo-6000`, `max_model_len=262144`); formatter unit tests need no backend.

## Resume Instructions

1. Read the project's root `AGENTS.md`/`CLAUDE.md` (and the parent's, in a sub-project),
   `context/plans/RULES.md` (in full), then `03_columnar_stats_honest_prefill.md`, then
   `context/pitfalls.md` (both entries bind this plan: the gate command and the `AGENTS.md` symlink).
2. Environment: `cargo 1.99.1`. `RULES.md`'s literal gate command (`-- -D warnings`) is **rejected**
   by this cargo — run the gate as
   ```bash
   export RUSTFLAGS='-D warnings'
   cargo check --workspace --features influxdb && \
   cargo clippy --all-targets --features influxdb && \
   cargo fmt --check && \
   cargo test --workspace --lib
   ```
   `export` once (an env prefix on the first command only leaves the rest unchecked).
   Baseline at plan creation (2026-10-16): exit 0, **915 passed / 0 failed**.
   T1 and T5 also need the live backend reachable (`curl -m 3 http://cosmo.lan:8700/version`).
3. Ordering: **T1 before T4** (T4's denominator choice is T1's output — do not code the correction on
   the stale code comment). **T2 before T6** (T2 lands the red contract test `#[ignore]`d so the gate
   stays green; T6 removes the attribute and makes it pass — keep both outputs for the Evidence table).
   **T3 before T4 and T6** (both consume the `Option<f64>` rates). Then T5, T6, T7.
   One task per commit; T6 and T7 both touch `README.md`, so T6 does not touch it at all.
4. Commit per task as `[proxy][P3][T<n>][<topic>] <summary>`, with **this tracker's row in the same
   commit**; the scope tag is `[proxy]`, never `[llama-proxy]` (RULES Rule B addendum).
5. First task: also flip the plan metadata `Status` to `in-progress`, this SERIES row to
   `in-progress` under *Currently Active*, and `Start date` above — all in T1's commit.

## Open Unknowns

1. **What does `time_to_first_token_ms` exclude on this vLLM fork, and what other keys does the fork
   serve?** The whole correction's denominator depends on it and the current code comment describes a
   different build. **RESOLVED by T1, 2026-10-16** (`captures/vllm_metrics_2026-10-16_*.json`, live
   vLLM `0.33.0.dev20261015+g4d9a8d6`). `metrics` carries exactly six keys: `time_to_first_token_ms`,
   `generation_time_ms`, `queue_time_ms`, `mean_itl_ms`, `tokens_per_second`, `speculative_decoding`
   (the last is `null` on this build). There is **no** `prefill_time_ms` and **no**
   `prompt_tokens_per_second`, so ttft is the only prefill window the fork exposes and T4's
   denominator is correct as planned. `metrics.tokens_per_second` is
   completion/(ttft+generation_time) — an **end-to-end** rate that includes prefill, so the proxy must
   never read it as a decode rate. `queue_time_ms` measured 0.0077–0.0118 ms over n=5, i.e. noise:
   **not subtracted**. Whether ttft contains it is unmeasurable at that load and immaterial; if it
   does, the corrected rate is *understated*, not inflated (a too-large denominator deflates — the
   reasoning in the first draft of this task was inverted). Correctness of the discount itself, same
   9074-token request cold vs warm: cold `cached=0 ttft=666.28` → 13,619 (raw ≡ corrected); warm
   `cached=3200 ttft=449.55` → raw **20,184** vs corrected **13,066**. Over 4 warm repeats of that
   identical request the corrected figure holds at mean 12,858 (12,770–13,066, 2.3% spread) while the
   raw figure swings 19,727–20,184 — the inflation tracks cache depth, which is the bug's signature.
   Numerator-independent corroboration: those 3200 tokens moved from compute to cache between the cold
   and warm runs and ttft fell 216.72 ms, so the engine's own rate is 3200/0.21672 = **14,765 tok/s**
   — 8% from the corrected number, 46% from the raw one.
 2. **The one-sided-absent shape of `tok=`** (`tok=186417/   n/a` vs a different filler). Decide in T6,
    record the choice and the test that pins it.
 3. **Do `ctx`/`tok` stay `{:>6}`, or widen to `{:>7}`?** **Raised by T2, needs the human — deliberately
    not decided in code.** A 1M-token vLLM window (`1048576`) and a prompt of similar size are 7 digits
    and do not fit the plan's pinned `{:>6}`, so the head cannot stay aligned for them. Widening both
    symmetrically costs 2 cells of head (129 → 133) against Open Question 7's width concern; leaving them
    means a 1M-context deployment gets a ragged head. T2 took the conservative route: the contract keeps
    the plan's 6-cell fields, s2 uses the widest **in-band** values (`999999/999999`), and
    `an_over_wide_context_is_never_clamped` (non-ignored) pins the one property that must hold either way
    — an out-of-band number shifts the columns rather than being clamped, rounded or dropped. The contract
    test must not be "fixed" by editing `CELL_CTX`/`CELL_TOK` until this is answered.
 4. **`stream=sync`: confirmed a documentation change, not just a formatter change.** `README.md`'s
    "Is it really streaming?" currently instructs operators to read bare `sync` for `fake` mode, so T6's
    `stream=sync` must land with that sentence, or the docs misdescribe the log line. README also still
    lists `cache` *inside* the head and a `reprompt=N/2` cell `format_compact` does not emit — both are
    T7's job to regenerate from rendered output.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Establish what vLLM's `metrics` object actually contains | `captures/` | done | 6 payloads captured; **no** `prefill_time_ms` ⇒ ttft confirmed as T4's denominator; `queue_time_ms` ≈0.01 ms ⇒ not subtracted; corrected pp mean 12,858 (n=4) |
| 2 | T2: Write the columnar contract as a red test | `src/stats/formatter.rs` | done | lands `#[ignore]`d; **34 cells red**, all genuine and all clearable by T6; golden positive control added (ASCII + CJK) and it caught a wrong *expectation* |
| 3 | T3: Make an unmeasured rate absent, not zero | `src/stats/collector.rs`, `src/exporters/influxdb.rs`, `src/stats/formatter.rs` | done | `prompt_tps`/`generation_tps` → `Option<f64>`; `total_tps`/`decode_tps_min` stay `f64`. **Beyond plan:** compact tier-1 gate moved onto decode-presence, `pretty` follows it, one `tps_tier()` predicate, 4 lying fixtures fixed, gate gained the feature-on test line |
| 4 | T4: Correct the vLLM prefill rate for prefix-cache hits | `src/stats/collector.rs` | pending | five Context rows are the test data; none may exceed 15k |
| 5 | T5: Add the prefill plausibility canary | `src/exporters/mod.rs`, `src/proxy/handler.rs`, `src/config/mod.rs` | pending | first-WARN-then-1-in-100; counter exact |
| 6 | T6: Rewrite `format_compact` against the contract | `src/stats/formatter.rs` | pending | pinned head, free tail, `cache=` last |
| 7 | T7: Regenerate the docs from the rendered line and harvest the lesson | `README.md`, `CLAUDE.md`, `context/pitfalls.md` | pending | delete `README.md:334`; pin the README example with a test |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-16 | `RUSTFLAGS='-D warnings'` + check/clippy/fmt/test --workspace --lib | exit 0; 915 passed / 0 failed | 1 | baseline at plan creation; not a measurement |
| 2026-10-16 | `curl cosmo.lan:8700/{v1/models,props,version}` | vLLM 0.33.0.dev20261015+g4d9a8d6; `/props` 404 | 1 | proves the llama.cpp `timings` branch is dead on this box |
| 2026-10-16 | derived from 5 live log lines: `corrected_pp = (prompt−cache)/(prompt/prompt_tps)` | 6,421 / 11,748 / 12,491 / 12,797 / 13,021 tok/s | 5 distinct requests | **pre-implementation prediction**, computed before any code changed; T4 must reproduce it ±1%. Cluster 11.7k–13k + one 6.4k; the cache-3200 control row moves least (15,098 → 12,797), which is the signature of a correct discount. Not N repeats of one request — do not report it as a mean |
| 2026-10-16 | `curl -m 120 $B/v1/chat/completions -d @/tmp/opencode/req{1,2}.json` × 6 → `captures/vllm_metrics_2026-10-16_*.json` | `metrics` keys = `time_to_first_token_ms`, `generation_time_ms`, `queue_time_ms`, `mean_itl_ms`, `tokens_per_second`, `speculative_decoding`; **no** `prefill_time_ms`/`prompt_tokens_per_second`; `speculative_decoding: null` | 6 payloads (1 short-cold, 1 long-cold, 4 long-warm) | build 0.33.0.dev20261015+g4d9a8d6; settles Open Unknown 1 — ttft is the only prefill window the fork serves |
| 2026-10-16 | identical 9074-token request repeated warm, n=4 | corrected `pp = (prompt−cached)/ttft`: 12,770 / 12,776 / 12,822 / 13,066 | 4 | mean **12,858**, spread 296 = **2.3%**; the *raw* `prompt/ttft` over the same runs reads 19,727–20,184 (+55%), so the bug's magnitude tracks cache depth, not request cost |
| 2026-10-16 | same request on a first-touch prompt (cold prefix cache) | `cached=0`, `ttft=666.28ms`, raw ≡ corrected = 13,619 | **1** | cold is **not repeatable**: vLLM exposes no prefix-cache flush, so this is a single run, never a mean (RULES P1) |
| 2026-10-16 | `queue_time_ms` read off every capture | 0.0077 / 0.0086 / 0.0089 / 0.0096 / 0.0098 / 0.0118 ms | 6 | ≈ 0 ⇒ **not** subtracted. Whether ttft contains it is unmeasurable at this load, and if it does the rate is *understated*, not inflated — see Follow-ups |
| 2026-10-16 | derived marginal rate: 3200 cached tokens ÷ Δttft (666.28 − 449.55 ms) | engine compute rate **14,765 tok/s** | 1 cold/warm pair | the only check that does **not** depend on the numerator choice: 8% from the corrected figure, 46% from the raw one |
| 2026-10-16 | T2 gate: `RUSTFLAGS='-D warnings'` + check/clippy/fmt/`test --workspace --lib` | exit 0; **919 passed / 0 failed / 1 ignored** (baseline 915; +4 = discrimination, head-constants, golden control, over-wide witness) | 1 | not a measurement |
| 2026-10-16 | `cargo test --lib stats::formatter -- --ignored` (the red contract) | 1 failed, **34 cell(s) wrong** | 1 | decomposition, each row independently attributable: **19** label offsets moved (`dur`×5, `ctx`×5, `finish`×5, `stream`×4) + **5** `tok=` ABSENT + **1** `stream=` ABSENT (s1 non-streaming prints bare `sync`) + **5** rate-group width (9/67/34/20/4 vs 23) + **3** trailing whitespace (s1, s3, s4 — s2/s5 end on `concurrent=` so have none) + **1** control char (`\n`, s5's forged `finish=stop\nforged 2`). Reconciles with the first red run: 19+1 = 20 label failures, and the +4 is exactly the whole-line assertions added in review |
| 2026-10-16 | golden positive control `the_head_offsets_describe_a_buildable_line` vs a **hand-typed** expectation | first hand-typed golden measured **127 cells, not 129** (a dropped space after the model field and after `stream=ok`); regenerated from a display-width script, both literals now assert 129 at offsets 0/47/61/79/97/112 | 1 | **[OBSERVED]** the positive control earned its keep: a 34/34 red cannot distinguish "formatter is ragged" from "expected offsets are wrong", and here it caught a wrong *expectation*. Both reviewers had asked for this test sight unseen |
| 2026-10-16 | T3 gate: `RUSTFLAGS='-D warnings'` + check / clippy `--all-targets` / fmt / **test twice** (`--workspace --lib`, then `--workspace --lib --features influxdb`) | exit 0; **925 passed / 0 failed / 1 ignored** feature-off and **947 / 0 / 1** feature-on | 3 identical feature-on runs | baseline 919; +6 tests authored. The 947−925 = **22 tests execute under the feature that the gate never ran** — hence the 3 repeats, which are for `queue_tests` (threaded, loopback sockets, never once executed here) |
| 2026-10-16 | `cargo test --lib measured_rate_fields_are_omitted_when_absent` **without** the feature flag | `0 passed; 0 failed; 926 filtered out` | 1 | **[OBSERVED]** the test is not *skipped* — it does not exist in that binary. `Cargo.toml` has `default = []` and the exporter test modules are `cfg(all(test, feature = "influxdb"))`. clippy **compiles** them, so type errors surface while assertions never run: the gate was green over ~20 dead tests |
| 2026-10-16 | `decode_tps_min_is_written_on_every_point` (pre-existing, untouched by T3) under `--features influxdb` | FAILED both asserts: expects `decode_tps_min=84.34 ` and `decode_tps_min=0 ` with a trailing space; the wire emits `decode_tps_min=84.34,duration_ms=…` | 1 | **[OBSERVED]** dead on arrival since the day it was written, **not** a T3 regression — T3 alters only `prompt_tps`/`generation_tps` presence. influxdb2 sorts field keys and comma-joins them, so the only space after a value is the one before the timestamp, after the **last** field. Same family: bools emit `f`/`t`, never `false`/`true` — which also sank the first draft of the new exporter test |
| 2026-10-16 | `cargo test --lib stats::formatter -- --ignored` after T3 | 1 failed, **34 cell(s) wrong** — profile identical to T2 | 1 | T3 left the contract untouched: still exactly the 34 T6 owns. Asserted rather than assumed, because T3 rewrote the very tier logic the head is built from |
| | | | | |

*(T1 adds the captured payload rows here; T4 adds the post-fix rates from a real run, with the build
SHA — RULES P1: a number without its input, build and spread cannot be cited by a later plan.)*

## Notes / Deviations

**(Rule E)** Anything the plan asserted that turned out wrong, including negative and inconclusive
results. Pre-seeded so the next session does not re-litigate them:

- Plan 1's tracker records T4 as `pending` against a `5/5` Overview, and its commits are tagged
  `[llama-proxy]` against a `[proxy]` rule. Both are history (Rule B2) — do not "fix" them; this plan
  supersedes the claims, not the commits.
- Plan 1's prefill correction is **correct code on a branch this deployment never takes**. "The fix
  shipped and the numbers did not change" is expected, not a mystery.
- The compact line's `tps=` cell being one string for two independently-sourced numbers is the reason
  `tg= 0.00` can already appear on a vLLM body carrying only ttft (`collector.rs:503` sets one bool
  from either timing). T3/T6 address it; it is not a regression introduced here.
- Reviewers (both) recommended `prefill=`/`decode=` over `pp=`/`tg=`, `out=` alone over `tok=`, and 2
  decimals over 3 on the k-scale. The human chose `pp=`/`tg=`, `tok=A/B`, and `NN.NNNk` — those win;
  the objections are recorded in the plan's Rejected Claims and Context rather than silently dropped.

**Deviation** — the round-2 review (both reviewers, on the T1+T2–T4 design) overrode the plan in five
places. Each was accepted and is implemented as below, not as the plan's body text says:

- `has_timing_split` does **not** stay as written (plan T3). It becomes
  `prompt_tps.is_some() || generation_tps.is_some()`, because `influxdb.rs:213` writes the flag to the
  wire *as the truth about timing*, and a rate-derived definition is the only one that stays honest
  once the rates are `Option`. Tier selection in `format_compact` keeps reading this same flag — no
  timing at all ⇒ both rates `None` ⇒ tier 2 (floor + work), which is the right answer.
- `finite_rate` keeps its `f64` signature and its documented `0.0 == absent` contract for the
  millisecond fields; a **sibling** `finite_rate_opt -> Option<f64>` serves the rate fields. Changing
  the signature instead would have made `Some(finite_rate(inf)) == Some(0.0)` — the fabricated zero
  smuggled *inside* an `Option`, which is the exact defect T3 exists to remove. Reachable via a
  derived division overflow (`u64::MAX / f64::MIN_POSITIVE`).
- The plan calls `has_throughput_signal` and `format_compact` "untouched" by T3. **False** — neither
  compiles against `Option<f64>`. Both are edited in T3; `format_compact` renders a per-side `n/a`,
  never `unwrap_or(0.0)`, which would preserve the very `tg= 0.00` this plan kills.
- A **second fabrication site** the plan missed: `extract_timings`'s `predicted_per_second`
  `.unwrap_or(0.0)` (`collector.rs:361-365`, the llama.cpp branch) — `None` on missing.
- T4's `p <= c` guard is **three-way**, not two-way: `c == p` is *legitimate* (a full prefix-cache hit
  on an identical retry) ⇒ `None`; `c > p` is arithmetically impossible ⇒ keep the backend value
  verbatim **and** WARN+counter (reuses T5's first-then-1-in-100 pattern); `c == 0` ⇒ verbatim.
- The canary's default is **30000, not 15000**: the two populations are far apart (bug raw 48k–580k vs
  legitimate corrected max 13,619), so 30k is >2× the real max and <⅓ the bug floor. The human's 15k
  stays as **T4's acceptance assertion** (over 15k ⇒ suspect a bug), which is where it was always
  meant to bite. `raw_pp` is included in the WARN fields so a WARN line identifies itself as the old
  bug or a new one.

**Deviation / T2 review record** — round 3 (both reviewers, on the contract test itself). One real bug
in the test was found and fixed, and four things were pinned that T6 must not rediscover:

- **[OBSERVED] The first version of the rate-width assertion was unsatisfiable by design.** It computed
  `occupied = dur - (CELL_MODEL + SEP)` = `47 - 23` = **24** and compared it to `CELL_RATE` = **23**, so
  a *perfectly columnar* line failed it and T6 could never have gone green. The rate cell is bracketed
  by **two** separators (after the model cell, before `dur=`), so the derivation is
  `dur - (CELL_MODEL + 2*SEP)`. It is now `rate_group_cells()`, one definition shared by the contract
  test **and** the golden control — the reason it survived a fully-red run is that nothing green ever
  executed it. **Do not "simplify" `2 * SEP` back to `SEP`.**
- The rate-width check is **derived from** the `dur=` offset, so it is not independent evidence: the 5
  rate-width rows restate the 5 `dur=` rows. Read the 34 as ~29 distinct facts, not 34.
- **`cache=` and `reprompt=` are in the TAIL, not the head.** `HEAD` has seven cells and neither is one.
  This is load-bearing, not tidiness: `cache=` renders between `ctx` and the token counts today, and
  while it does, the `tok=`/`stream=`/`finish=` offsets on s2 and s4 **cannot reach their targets** —
  cache is variable-width and unpadded. If a cell ever moves into the head, `HEAD_CELLS` changes in the
  same commit and the plan's table is updated with it.
- **`stream=sync` is an operator-visible output change, decided here rather than at T6.** Today
  `format_compact` renders bare `sync` when `streaming == false`, which is why the contract reports
  `stream=` ABSENT for s1. T6 renders `stream=sync` padded into the 7-cell badge field **and** updates
  README's "Is it really streaming?" grep guidance (`sync` vs `stream=ok`) in the same commit, or the
  docs start lying about the log line.
- `s1.streaming = false` is **load-bearing** — the only fixture row exercising the non-streaming shape,
  and the cheapest line in the file to flip in order to silence one red. Commented in place.
- The python display-width script used to draft the goldens is a **drafting aid; the Rust assertion is
  the oracle**. They agree on CJK but `unicodedata`/`wcwidth` and `unicode-width` diverge on
  ambiguous-width and emoji classes, so wide-glyph fixtures stay **CJK-only, no emoji**, or the two
  oracles drift and the next bug is a width-library disagreement.

**Deviation / T3 review record** — both reviewers, on the tier gate and on the exporter. The plan
said T3 was a type change; it became a display-semantics change, and every step below is something
`cargo test` did not tell us.

- **The compact tier-1 gate moved from `has_timing_split` to decode-presence, and this cannot
  fabricate a split.** Proof, not reassurance: `has_timing_split` has exactly **one** writer —
  `prompt_tps.is_some() || generation_tps.is_some()` — so `generation_tps.is_some()` ⟹
  `has_timing_split`, which makes the new tier-1 set a strict **subset** of the old one. Every line
  that printed a split before prints a split now. What it closes is the half split: the old gate
  could fire on prefill-only and print `tps=12345.00/   n/a` — a big number in the column the eye
  lands on, under a key that means *measured*.
- **Tier 1 does NOT additionally require a counted completion, and must not start to.** The vLLM path
  cannot yield `generation_tps=Some` without a count; the llama `timings` path can (it reads
  `predicted_per_second` unconditionally, and the `predicted_n` fallback writes `None` when the key is
  simply absent), so a self-contradictory backend body can. Rendering the backend's own rate there is
  garbage-in-honestly-reported; requiring a count would drop the only real measurement on the line and
  leave a prompt-only `work=` standing alone — that is the regression, not the opposite.
- **`format_pretty` was moved onto the same gate. The plan did not ask for this.** One reviewer called
  the old flag-gate a documented divergence, the other a must-fix; the must-fix won because pretty is
  the **default** format: a half split printed `Prompt Processing: 880000` beside `Generation: n/a`
  under a measured heading and **never entered the floor branch**, so the format most operators read
  had no bound on decode at all. The measured prefill is kept — as its own labelled row, not as a split
  — and the footer now distinguishes `(no backend decode rate…)` from `(no backend split…)`. A third
  box shape exists, so the frame-width test gained a third pass.
- **One tier predicate, `tps_tier()`, shared by `format_compact`, `format_pretty` and the alignment
  test.** That test had been re-deriving tiers from `has_timing_split` and had silently drifted from
  the renderer — "all three tiers appear in the fixture" was on its way to being satisfiable without
  the formatter ever rendering what the test believed. Gate predicate is `is_some_and(|v| v > 0.0)`,
  matching `tps_side`'s render predicate and `has_throughput_signal`'s second lock: `is_some()` alone
  would let a directly-built `Some(0.0)` produce the very half-row this gate exists to kill.
- **Four fixtures were lying** (`has_timing_split = false` with the rates left populated) — a state the
  sole writer of that field cannot produce. Two of them were **passing while testing the wrong branch**:
  the cache-cell test had silently moved to tier 1, losing `cache=`-beside-`work=` coverage, and the
  pretty frame test would have measured the split rows twice. A fixture self-consistency assertion
  (`flag == rates-are-some`) now guards the class, which is what the failing test lacked.
- **Rejected, with reason:** a `fields()` line-protocol parser for the exporter tests. Its stated
  rationale (`completion_tokens` substring-matches `prompt_tokens`) is simply false, so the existing
  absence assertions are not passing for the wrong reason. Filed for T7 rather than rewriting green
  tests inside T3.
- **The project gate gained a second test command (RULES.md step 4), per Rule A #5** — and **both**
  lines are required, never one replacing the other: `exporters/mod.rs`'s `mod feature_off` is
  `cfg(not(feature = "influxdb"))`, so the task-80 tests proving a compiled-out stub refuses instead of
  returning a fake `Ok` exist only in the feature-**off** run. Full write-up in `context/pitfalls.md`.
  A dead pre-existing test (`decode_tps_min_is_written_on_every_point`) had to be fixed first or the
  flipped gate was red on arrival — kept in this commit because the gate update that exposed it is
  mandated by this task.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| **Prerequisite, T6's first commit:** rate *magnitudes* in `the_contract_fixture_discriminates` (digit counts of `pp`/`tg`/`work`) | ~3 lines. Without it, softening s2's `prompt_tps = 88_000.0` to `53.64` leaves every assertion green and silently removes `CELL_RATE = 23`'s only justification — Plan 01's bug one level up, in the exact column this plan exists to defend | T6 |
| **Prerequisite, T6:** whole-line width sum (`width(line) == HEAD_CELLS + present tail cells`) plus a ~24-cell cap on `finish` | the only assertion bounding the deliberately-untruncated finish value and catching a hole right of `finish=`; `content_filter` must still overflow visibly, but a 4 KB `stop_reason` must not become a paragraph in every journal entry | T6 |
| Optionals as a **bit tuple** rather than a popcount, and badge `distinct == 5` | hardens coverage already claimed rather than closing a contract hole; popcount erases *which* optional varied | T6 |
| Field-count / `split(' ')` check on the rendered line | **deliberately not added**, and both reviewers concurred: it is blind to interior holes, the head is covered by absolute offsets, and the tail is free-form so `cut -d' '` is unsafe regardless. Recorded so it is not re-proposed | — |
| Extra compact cells: `reason=` (reasoning tokens are in pretty/json but not compact), `node=<host:port>` for per-node attribution, a `req_id` tail cell, a startup column legend | raised in review; each is a real operator gap, none is this plan's deliverable, and each widens the head | Plan 03 Open Question 6 → a later stats plan |
| Exercise the llama.cpp `timings` prefill branch against a real llama.cpp backend, or delete it | it is unexecuted on this box and still cites Plan 1's unresolved Open Unknown 1 (`collector.rs:372-380`) | Plan 03 Open Question 4; needs a llama.cpp backend |
| Flip `stats.max_plausible_prefill_tps` to a `const` if it is never tuned | both reviewers: a config field is a permanent surface for a constant operators will not touch | Plan 03 Open Question 2 |
| Reconcile the two `SERIES.md` dead `../context/opencode_claude_llama_notes.md` references while touching docs anyway | already a Standing Constraint; T7 is the nearest docs touch | human decision (SERIES Standing Constraints) |
| Saturate the backend and regress `time_to_first_token_ms` against `queue_time_ms` to settle whether vLLM's ttft includes queue time | T1 measured `queue_time_ms` at ~0.01 ms (n=6), so the question is **immaterial at this load and deliberately not subtracted** — but at saturation it would matter, and the direction is a rate that reads *low*, not high. The first draft of T1 reasoned this backwards; recorded so nobody "fixes" it the wrong way | a later load-testing plan; needs a controllable load generator |
| Surface `prompt_tokens_details.created_cache_tokens` separately from `cached_tokens` | neckbeard: it is a different quantity (tokens the engine *wrote* to cache this turn) and folding it into the discount would be wrong; it is operator-interesting on its own | a later stats plan |
