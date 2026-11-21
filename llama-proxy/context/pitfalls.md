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
  error. **Re-verified 2026-11-21** on `cargo 1.100.0` / `rustc 1.100.0`: unchanged, and `just gate`
  run verbatim reproduces `error: unexpected argument '-D' found` at `justfile:16` — so it is the
  form that is broken, not someone's quoting. **Staleness trigger:** re-run `just gate` after any
  toolchain update; if it passes, delete this entry.

- **[OBSERVED 2026-11-21] The `--features influxdb` half of the gate cannot build on this box, and it
  looks like your diff broke it.** Symptom: `cargo check --workspace --features influxdb` (and
  `clippy --all-targets --features influxdb`) exit 101 with `failed to run custom build command for
  openssl-sys v0.9.111` → "only compatible with OpenSSL (1.0.2 through 1.1.1, or 3), or LibreSSL 3.5
  through 4.2.x". Default features build fine in the same tree, so the failure appears exactly when
  you add the flag. Why: the influxdb feature pulls `reqwest 0.11` → `native-tls` → `openssl-sys`,
  which caps below the system's `OpenSSL 4.0.3`; the default-feature chain is `reqwest 0.12.28` with
  no native-tls (`cargo tree --features influxdb -i openssl-sys --depth 3` is the one-command proof).
  Environment drift, not repo drift — the same legs passed on 2026-10-16. Do this: prove pre-existing
  with a **control on clean HEAD** (`git worktree add --detach /tmp/… HEAD`, its own
  `CARGO_TARGET_DIR`, same command — identical 101), confirm with `grep 'Checking llama-proxy'` that
  your crate was never reached, then state the **coverage delta** in the tracker rather than calling
  the gate green: `cargo test --workspace --lib` runs 955 vs the ~976 the feature-on matrix would,
  and name which tests are the difference. `context/CLAUDE.md`'s "run both feature states" is
  currently unsatisfiable here — say so, don't imply you ran it. Do **not** hack it green with
  `OPENSSL_DIR`, a vendored OpenSSL, or `--no-default-features` (the feature-off stub tests and the
  feature-on tests cover different code; see the entry above); the fix is a dependency bump or moving
  that chain to rustls, which is a human call. **Staleness trigger:** any `cargo update` touching
  `openssl-sys`/`native-tls`/`reqwest`, an `influxdb-client` upgrade, or an OpenSSL change; if the
  legs build, delete this entry.

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

- **[OBSERVED 2026-11-21] The head's documented `cut -c` offsets stop matching on a real log line,
  because ANSI escapes live *inside* the prefix.** Symptom: README says the rate
  group is `cut -c1-22` (`1-23` until the same-day cell trim on this entry's date — the number moves
  whenever a cell width changes, which is the entry's point); against `journalctl`/a captured line it returns `^[[2m2026-11-21T20:36:25` — and after a
  naive `sed 's/.*llama_proxy::stats: //'` the offsets look fine, so the bug only shows on the raw
  shape an operator actually pipes. Why: `main.rs:119` builds the subscriber as
  `fmt().with_env_filter(..).init()` without `.with_ansi(false)`, and tracing-subscriber 0.3.22
  enables colour by default with **no tty check** — so the escapes survive a redirect to a file
  (verified: a redirected log still carries them). They wrap the timestamp, the level and the target
  separately, i.e. *before* the `tg=` glyph, which is why pattern recipes survive and byte counting
  does not. Do this: strip colour **and** prefix before positional work —
  `sed -e 's/\x1b\[[0-9;]*m//g' -e 's/.*llama_proxy::stats: //'` — or start the proxy with
  `NO_COLOR=1` **in the proxy's own environment**: it is read by the process that formats the
  line, so exporting it on the pipeline that greps the log changes nothing (verified A/B on a
  live stats line: escapes gone, plain `cut -c1-22` correct — that slice as of 2026-11-21, `1-23` when first measured; the finding is untouched by which number it is). `grep`/`sed` extraction needs no
  pre-pass: they match, not count. Do **not** write that colour "cannot be turned off", and do not infer tty-ness from a
  terminal session — check the crate you depend on (`~/.cargo/registry/src/*/tracing-subscriber-*/src`
  is two commands away) before documenting upstream behaviour. **Staleness trigger:** any change to
  the subscriber setup in `main.rs` (a `.with_ansi(false)` makes this entry obsolete), or a
  tracing-subscriber upgrade.

- **[OBSERVED 2026-11-21] A fixed-width field with two label renderings shares ONE value width, so
  "trim a cell from each rendering" is a ONE-cell saving, not two.** Symptom: an option list handed
  to a human that under-promises the saving, and cell arithmetic that does not close. Why:
  `rate_group` writes `tg=` (3 cells) on tiers 1/3 and `tg>=` (4 cells) on tier 2, and the group's
  contract is that both put the number's **right edge on the same cell** — so the label's extra `>`
  is paid for out of the value field. An 11-cell `tg` field was `{:>8.1}` on tier 1 and `{:>7.1}` on
  tier 2; one cell off gives `{:>7.1}`/`{:>6.1}`, and the two tiers' **ceilings then differ by an
  order of magnitude on purpose** (99,999.9 vs 9,999.9). Do this: count in **value cells**, and state
  a ceiling as "the largest value whose RENDERING fits", never as a numeric range — `{:>6.1}` of
  9,999.95 is `10000.0`, seven characters in a six-cell field, so the real ceiling is below the
  nominal range. When the freed cell gets spent, check which trade the human was actually offered:
  this trim could buy line length *or* `pp`'s headroom, not both. **Instance:** the 2026-11-21
  rate-cell trim (group 23 → 22, `HEAD_CELLS` 129 → 128); I offered a two-cell saving and corrected
  it mid-decision. **Staleness trigger:** any proposal to resize a multi-rendering fixed-width field,
  or a new tier added to `rate_group`.

- **[OBSERVED 2026-11-21] A published start offset survives two shapes of review because the slice
  it documents comes back non-empty.** Symptom: README's offset table read `tail | 130-` while the
  tail's first key measured **131** — cell 130 was the head/tail separator, so `cut -c130-` returned
  the right tail behind a leading space and nothing looked wrong. It had stood that way since Plan
  03's first table, inside a row that claimed the table was "measured". Why: a start offset is right
  only if the key's **first glyph** sits at that cell, and "the slice contains the payload" is a
  weaker property that a separator satisfies for free. In the session that caught it, both a reviewer
  and I re-derived the number by arithmetic and **both were a cell out**; `cut` was the only thing
  that was right. Do this: measure every start offset as `line.find(key) + 1` or
  `cut -cN-N | cat -A`, on the widest and the narrowest rendered line, and say in the table which
  cell the separator occupies — a reader who guesses will guess exactly the wrong way, as I did.
  **Staleness trigger:** any change to a cell width (everything right of it moves), or any new row in
  a `cut -c` table.

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
