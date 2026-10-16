<!-- hands-plan:v1 2026-10-16 -->
# pitfalls.md — gotchas that bit, not hazards guessed from reading

Routing + provenance: `context/AGENTS.md`; harvest trigger: RULES D2 (anything that took more than
one attempt, not a formatter re-run). Entry shape: **symptom → why → do this → don't**. Fix a stale
entry in place, dated — do not append a second entry for the same thing.

## Toolchain

- **[OBSERVED 2026-10-16] `just gate` cannot pass.** Symptom: `error: unexpected argument '-D' found`.
  Why: `justfile:16` and `RULES.md:122`/`124` pass `-- -D warnings`, which this cargo
  (`cargo 1.99.1 (5f94df478 2026-08-27)`) rejects. The gate that actually ran for Plan 02 — exit 0
  with 915 lib tests at `8d4ba29`:
  ```
  export RUSTFLAGS='-D warnings'
  cargo check --workspace --features influxdb && \
  cargo clippy --all-targets --features influxdb && \
  cargo fmt --check && \
  cargo test --workspace --lib
  ```
  `export` once, then chain with `&&` — do **not** write `RUSTFLAGS='-D warnings' cargo check … &&
  cargo clippy …`; the env prefix binds to the *first* command only, so a bare `cargo clippy` exits 0
  **on warnings** — a green gate that checks nothing. Caveat: `RUSTFLAGS` never reaches build scripts /
  proc-macros (cargo design), so read clippy's output, don't trust exit 0 alone. **Don't** "fix"
  `justfile` or `RULES.md` to match — both are authoritative; change them only with human sign-off
  (the real fix is `RUSTFLAGS` env or `-Dwarnings` with no `--`). Record the command you *actually* ran
  in the tracker's Evidence line, and note "`just gate` broken" there so the gap doesn't read as agent
  error. **Staleness trigger:** re-run `just gate` after any toolchain update; if it passes, delete
  this entry.

- **[HAZARD — verified 2026-10-16, bit once] `cargo test --workspace --lib` never executes the
  influxdb exporter's tests, so a wire-format test can be dead on arrival while the gate stays
  green.** `Cargo.toml` has `default = []`, and the exporter's test modules are
  `#[cfg(all(test, feature = "influxdb"))]` (`wire_tests`, `queue_tests`, `url_gate_tests`,
  `feature_on`). The gate's `cargo clippy --all-targets --features influxdb` *compiles* them, which
  is what hides the hole: type errors surface, assertions never run. Symptom when you filter on one:
  `0 passed; 0 failed; N filtered out` — the test is not skipped, it does not exist in that binary.
  Found because `decode_tps_min_is_written_on_every_point` asserted `contains("decode_tps_min=84.34 ")`
  — a trailing space that can never occur: `influxdb2` sorts field keys alphabetically and comma-joins
  them, so the only space following a value is the one before the timestamp, after the **last** field.
  Booleans are written `f`/`t`, not `false`/`true`, for the same reason (assert against the real
  encoding, not the Rust one). Do this: run the test command in **both** feature states (RULES.md
  step 4). Do **not** replace the plain command with `--features influxdb` — `exporters/mod.rs`'s
  `mod feature_off` is `cfg(not(feature = "influxdb"))`, and the task-80 tests proving a compiled-out
  stub refuses instead of returning a fake `Ok` compile only *without* the feature. Two commands, or
  half the matrix is blind. **Staleness trigger:** any new optional dependency that carries a
  feature-gated test module, or any gate edit that drops one of the two test lines.

## Editing the agent docs

- **[HAZARD — verified 2026-10-16, has not bitten] `AGENTS.md` is a symlink → `CLAUDE.md`; a
  write-and-rename edit silently forks it.** Verified present this session (`test AGENTS.md -ef
  CLAUDE.md` is true; root and `context/` each carry a pair). Symptom if it bites: `AGENTS.md` stops
  being a symlink and the two agent docs drift while both look normal. Why: `sed -i AGENTS.md …` and
  atomic-save editors (write temp + rename onto the path) replace the *symlink itself* with a regular
  file — `rename(2)` does not follow the destination symlink — so from then on editing one no longer
  moves the other, and a plain `diff -q AGENTS.md CLAUDE.md` still passes because the fork starts as a
  byte copy. Do this: edit `CLAUDE.md`, **never** `AGENTS.md`; health-check with `ls -l AGENTS.md`
  (expect `-> CLAUDE.md`) or `test AGENTS.md -ef CLAUDE.md`, **not** `diff -q`. Already forked: do
  **not** undo with `git restore`/`git checkout --` (banned, RULES B2) — confirm the content matches its
  `CLAUDE.md`, then `rm AGENTS.md && ln -s CLAUDE.md AGENTS.md`, re-check `-ef`, commit. Naming trap:
   "`CLAUDE.md`" is two different files — project-root (the proxy doc) and `context/CLAUDE.md` (this KB
   header); "update CLAUDE.md" unqualified means the **root** one.

## Documenting the stats line

- **[OBSERVED 2026-10-16] the compact line can change key names while every test stays green, because
  nothing renders the docs.** Symptom: Plan 03 rewrote the throughput cell to `pp=`/`tg=`/`tg>=` and
  `README.md` kept teaching `tps= 53.64/115.68`, an unlabelled bare token pair, and a
  grep-`tps>=` recipe for a cell that no longer existed — **953 lib tests passed throughout**, since
  the formatter's goldens assert the *code*'s output, not the operator contract. A stale doc line is
  worse than a missing one: an operator pastes it into a dashboard query and gets silence, not an
  error. Do this: pin doc examples to the renderer —
  `stats::formatter::tests::readme_compact_example_is_the_rendered_line` reads `README.md` at runtime
  (not `include_str!`, so editing docs does not recompile) and fails printing the real rendered lines,
  which makes regeneration a copy-paste from the failure. Assert **whole-line equality**, never
  `contains`: a substring check still passes with the stale block left in the file, which is the exact
  defect being prevented. Pair it with a negative assertion for a deleted key. Do **not** hand-type a
  sample line (this plan's hand-typed T6 golden came out 2 cells short), and do **not** relax the
  assertion to `contains` to silence it. Gate trap: the plan's Verify greps `tps=` and `reprompt=`, so
  migration prose must name retired cells by *shape* — and note `tps>=` does **not** contain the
  substring `tps=`, so that grep is weaker than it looks; record the grep's real output in Evidence
  rather than asserting it is empty. **Staleness trigger:** any change to a compact key, cell width,
  or the tier rules in `rate_group`.

- **[OBSERVED 2026-10-16] A fix can live entirely in the branch your deployment never runs, and both
  the tests and the docs will agree with you.** Symptom: the prefix-cache prefill discount was real
  code in the **llama.cpp** `timings` branch (`(prompt_n - cache_n) / prompt_ms * 1000`,
  `collector.rs:454-461`) while the live backend is vLLM — which serves no `/props` (it 404s) and
  emitted **no `timings` object in any of six captured payloads**, so that branch was dead here and
  the operator-facing promise was unverifiable from either direction. The vLLM path needed a different
  formula (`(prompt_tokens - cached) / ttft`), and until Plan 03 T4 it had none: on a warm 9,074-token
  request the raw rate read **22,025 tok/s** against a corrected **14,257**, a 55% inflation, while
   every unit test stayed green because each one fed the branch it was written for. **Why:** the two
   backends spell the same quantity differently, and a test fed one branch cannot observe that live
   traffic takes the other — green there meant only that unreachable code was self-consistent. Do
   this: before believing any backend-facing fix, prove **which branch the live backend reaches**
   (here: `/props` 404 + zero `timings` across captures), then reproduce the number against a
   **captured payload** under `captures/`, not against a hand-built fixture, and gate any cache
   arithmetic on the wire shape (see the `cached_tokens` entry below, which owns that rule). Record
   the dead-branch evidence in the tracker. Do

  **not** accept a green `cargo test` as proof a backend fix works; do **not** document behaviour of a
  branch you have never executed (the deleted README line "falls back to the backend value verbatim"
  described a conditional that does not exist — the branch always subtracts); and do **not** try to
  "verify" the llama.cpp path against the running vLLM server, which cannot reach it. **Staleness
  trigger:** a real llama.cpp backend becomes available (then execute the branch, capture it, and
  settle Plan 1 Open Unknown 1 — whether llama.cpp already excludes cached tokens from `prompt_n`,
  which if true means this branch subtracts twice), or `captures/` is deleted.

- **[OBSERVED 2026-10-16] `contains` cannot fail for a ragged line.** Symptom: a columnar log line
  whose every column had shifted still satisfied `assert!(line.contains("dur="))` and
  `assert!(line.contains("ctx="))` — substring presence is **position-blind**, which is exactly how
  the compact line drifted for a whole plan while its tests were green. Why: the defect that breaks a
  dashboard is *where* a value sits, and no amount of `contains` assertions encodes position; the T2
   contract only saw the 34 real cell failures once it probed **absolute offsets**. Same class, doc
   side: a file-wide `readme.lines().any(|l| l == rendered)` would pass while a stale duplicate block
   sat elsewhere in the file; the shipped pin test avoids that by locating the fenced block by needle
   and asserting its line count equals the fixture count (`formatter.rs:1904-1920`), which turns the
   per-line membership check into a bijection. Do this: for a columnar contract assert absolute
   offsets per cell (the

  `probe(&line, "dur=")` + `head_offsets()` pattern) and a whole-line width sum; for docs pin the
  block by a needle and compare line-by-line **in order**; measure any published `cut -c` table with
  `cut` rather than deriving it. Do **not** encode a column guarantee with `contains`, `starts_with`
  on a value, or a field count — all three are blind to interior holes. **Staleness trigger:** any new
  fixed-width field, or any contract test added with a substring assertion.

- **[HAZARD — verified 2026-10-16, caught in review] `cached_tokens` is not always a subset of the
  prompt count, and the prefix-cache discount is asymmetric.** Symptom if ignored: prefill silently
  understates by the entire cached amount on Anthropic traffic, or a dashboard's `work=` gets
  double-discounted. Why: on the OpenAI shape `usage.prompt_tokens` **already contains** the cached
  prefix, but the Anthropic branch fills `cached_tokens` from `cache_read_input_tokens`, which is
  **disjoint** from `input_tokens` — which is exactly why `context_used` there sums three fields. The
  vLLM path gates the subtraction on `cache_is_a_subset` (`collector.rs:591`, true only when
  `usage.prompt_tokens` is present); subtracting unconditionally "looks slow" rather than failing, so
  nothing downstream can see it. Asymmetry: the cache discounts the **`pp=` numerator only** — `work=`
  deliberately keeps cached tokens because it must reconcile with `sum(total_tokens)`. Do this: gate
  any cache subtraction on the wire shape, and let a full hit (`cached == prompt_tokens`) stand
  uncorrected at `debug!` (it is legitimate, and it is the most inflated number the proxy can emit).
  Do **not** write "a discount on `work=` too", and do not assume `cached > prompt` is noise — that is
  reported as a data error, unlike llama.cpp's saturating `prompt_n - cache_n`. **Staleness trigger:**
  a new usage shape, a new exporter field derived from the cache, or any edit to the prefill
  correction.
