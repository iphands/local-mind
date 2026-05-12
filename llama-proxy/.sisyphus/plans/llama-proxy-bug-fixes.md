# Plan: llama-proxy Bug Fixes

## Context

Comprehensive audit of `llama-proxy` found 27 bugs across 4 severity tiers:
- **CRITICAL (4)**: UTF-8 panic, premature return drops tool calls, `{}` garbage key, `rfind` wrong match
- **HIGH (7)**: InfluxDB batching dead, model overrides dropped, 502 on null tokens, tool result data loss, `group_name` missing, `max_tokens:0` default, thinking content leakage
- **MEDIUM (8)**: `Relaxed` ordering, context_total overestimates, metrics `streaming` always false, etc.
- **LOW (8)**: Dead fields, misleading `#[allow(dead_code)]`, silent config ignores, `.unwrap()` in production

This plan addresses the top 10 highest-impact bugs (CRITICAL + top HIGH).

## TODOs

### Phase 1: CRITICAL — Crash & Data Corruption Fixes

- [x] **BUG #1: Fix UTF-8 panic in `create_snippet_static`** (toolcall_bad_filepath_fix.rs:86, mod.rs:136)
  - Replace byte-index slicing `text[..max_len]` with safe `text.get(..max_len).unwrap_or(text)`
  - Update both instances in `toolcall_bad_filepath_fix.rs` and `mod.rs`
  - Verify existing tests still pass ✅ (330 tests pass)

- [x] **BUG #2: Fix premature `return` dropping concurrent tool calls** (toolcall_bad_filepath_fix.rs:371–389, 448)
  - Replace `return (chunk, FixAction::NotApplicable)` with `continue` pattern
  - Ensure all tool calls in a chunk get processed
  - Add test: chunk with 3 tool calls where index 0 is pre-fixed ✅
  - Verify: indexes 1 and 2 still get checked ✅ (330 tests pass)

- [x] **BUG #3: Fix `{}` extracted as legitimate key** (toolcall_malformed_arguments_fix.rs:148–165)
  - Add `if key.as_str() == "{}" { continue; }` before inserting into result HashMap
  - Verify aggressive_parse_json still extracts valid keys correctly ✅
  - Add test: malformed JSON `{content:"test",{}":"/path"}` produces correct output ✅ (330 tests pass)

- [x] **BUG #4: Fix `rfind` matching wrong occurrence in delta calc** (toolcall_bad_filepath_fix.rs:180–207)
  - Only use `rfind` result if `pos + len == accumulated.len()` (match at end)
  - Otherwise fall through to `safe_completion`
  - Verify existing delta calculation tests still pass ✅ (328 tests pass)

- [x] **BUG #5 (partial): Remove dead `remove_duplicate` field** (toolcall_bad_filepath_fix.rs:40,44,59)
  - Removed `remove_duplicate: Arc<AtomicBool>` field and setter
  - Changed `new(bool)` → `new()`, updated all callers
  - Updated registry, streaming tests, README
  - Verify: code compiles, tests pass ✅ (328 tests pass, -2 dead tests removed)

## Final Verification Wave

- [x] **F1: `cargo build`** — exit code 0, no warnings on changed files ✅
- [x] **F2: `cargo test`** — all tests pass (328 passed, 0 failed) ✅
- [x] **F3: Manual review** — read every changed file line by line, verify logic matches requirements ✅
- [x] **F4: Regression check** — verify non-changed files still compile and tests pass ✅

## Files to Modify

| File | Bugs Addressed |
|------|---------------|
| `src/fixes/toolcall_bad_filepath_fix.rs` | #1, #2, #4, #5 |
| `src/fixes/mod.rs` | #1 |
| `src/fixes/toolcall_malformed_arguments_fix.rs` | #3 |
| `src/api/openai.rs` | #7, #8 |
| `src/proxy/handler.rs` | #6, #15 |
| `src/exporters/influxdb.rs` | #9 |
| `src/backends/balancer.rs` | #12 |
| `src/backends/priority_free.rs` | #12 |
| `config/mod.rs` | #5, #12 (config type) |
