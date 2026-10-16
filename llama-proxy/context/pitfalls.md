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
