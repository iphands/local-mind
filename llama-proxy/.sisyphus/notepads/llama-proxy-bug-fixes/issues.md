## 2026-04-23T17:58:00Z: Known issues from audit

### File-specific issues to watch

#### toolcall_bad_filepath_fix.rs
- `create_snippet_static` uses byte-index slicing (2 instances: lines 86-91 and internal call)
- `calculate_completion_delta` uses `rfind` without verifying it's at end of string (line 188)
- `apply_stream_with_accumulation_default` has `return` that exits method early (line 389) — ✅ FIXED 2026-04-23 (BUG #2)
- `remove_duplicate` field and method are dead code (lines 40, 44, 59) — ✅ FIXED 2026-04-23 (BUG #5)
- `find_string_end` already handles escaped quotes correctly — don't break it

#### toolcall_malformed_arguments_fix.rs
- `aggressive_parse_json` regex `[,\{]([^\s"]+)` matches `{}` as key — ✅ FIXED 2026-04-23 (BUG #3)
- Regex can't handle nested JSON objects (line 152)

#### api/openai.rs
- `AnthropicUsage` has non-optional fields (lines 371-374)
- ~~ToolResult content drop (lines 404-409)~~ ✅ FIXED 2026-04-23
- Thinking blocks concatenated into content (lines 389-391)
- `AnthropicMessageRequest.max_tokens` defaults to 0 (line 296)
- `created: 0` for Anthropic conversion (line 436)

#### proxy/handler.rs
- `is_json_content_type` operator precedence is correct but confusing (line 210) — but misses non-`application/` +json types
- Two branches for body building, augmentation branch missing model/temp overrides (lines 476-514)

#### exporters/influxdb.rs
- `batch_size` and `flush_interval_seconds` never used (lines 68-138)
- `group_name` not exported as tag — ✅ FIXED 2026-04-23 (BUG #9)

### 2026-04-23: BUG #9 - `group_name` not exported as InfluxDB tag — FIXED

- **File**: `src/exporters/influxdb.rs` line 88-91
- **Problem**: `InfluxDbExporter::export()` added tags for `model`, `streaming`, `finish_reason`, `client_id`, `conversation_id` but omitted `group_name`. In multi-backend setups, metrics from different backend groups (e.g., `opus`, `haiku`, `catch_all`) were indistinguishable in InfluxDB.
- **Fix**: Added optional `group_name` tag after `conversation_id`:
  ```rust
  if let Some(ref group_name) = metrics.group_name {
      builder = builder.tag("group_name", group_name.as_str());
  }
  ```
- **Verification**: `cargo check` clean, `cargo test` 330 passed, 0 failures

#### backends/balancer.rs
- ~~`active_requests` uses `Relaxed` ordering everywhere (lines 20, 26, 33)~~ ✅ FIXED 2026-04-23 (BUG #12)
  - Line 20, 26: `fetch_add` → `Release`
  - Line 33: `fetch_sub` → `AcqRel`

#### backends/priority_free.rs
- ~~Same `Relaxed` ordering issue (lines 33, 41)~~ ✅ FIXED 2026-04-23 (BUG #12)
  - Lines 33, 41: `load` → `Acquire`
- Check-then-act race condition (lines 32-35)

### BUG #15: `is_json_content_type` missing `+json` vendor types — FIXED 2026-04-23

- File: `src/proxy/handler.rs` line 208-213
- Fix: Added `|| ct_lower.ends_with("+json")` to catch types like `merge-patch+json`, `hal+json`, `application/ld+json`
- Test coverage: Extended `test_is_json_content_type` with new `+json` suffix cases
- Verification: `cargo check` clean, `cargo test` 330 passed

### 2026-04-23T17:58:00Z: BUG #7 - AnthropicUsage null field crash - FIXED

- **File**: `src/api/openai.rs` lines 371-374
- **Problem**: `AnthropicUsage` struct fields `input_tokens` and `output_tokens` were non-optional with no `#[serde(default)]`. When llama.cpp returns `null` for either field, serde deserialization fails → 502 Bad Gateway.
- **Fix**: Added `#[serde(default)]` to both `input_tokens: u64` and `output_tokens: u64`, so `null` → `0`.
- **Verification**: `cargo check` compiles clean, `cargo test` passes all 330 tests.

## 2026-04-23T[fix]: BUG #8 - ToolResult JSON content silently dropped (FIXED)

### Root Cause
`AnthropicContentBlock::ToolResult` match arm used `content.as_str()` which returns `None` when content is a JSON object/array. This caused tool result content to be silently dropped.

### Fix Applied
- File: `src/api/openai.rs`, lines 404-414
- Added `else` branch with `serde_json::to_string(content).unwrap_or_default()` fallback
- Text content path unchanged — no behavioral change for string content

### Verification
- `cargo check`: PASS (Finished)
- `cargo test`: 330 tests, 0 failures
- No other files modified
- Notepad issue line (38) updated: ToolResult content drop resolved

### 2026-04-23: BUG #3 - `{}` extracted as legitimate key in aggressive_parse_json — FIXED

- **File**: `src/fixes/toolcall_malformed_arguments_fix.rs` lines 161-172, 86-96, 111-112
- **Problem**: Regex `[,\{]([^\s"]+)` in `aggressive_parse_json` matches `{}` as an unquoted key. For input `{"content":"test",{}":"/path"}`, the parser extracts `"{}" → "/path"` as a garbage key-value pair. This made `missing_params.len() == 1` check unreliable since `{}` was counted as a present key, masking truly missing params.
- **Fix**: 
  1. Skip `{}` key during unquoted regex extraction (lines 161-172)
  2. Removed `&& *p != "{}"` filter from missing params (line 88) — `{}` is no longer in parsed map
  3. Removed `&& parsed.contains_key("{}")` from both single-param and heuristic branches (lines 96, 112) — malformed pattern regex handles detection
  4. Updated `test_aggressive_parse_json` to assert `{}` NOT present
- **Verification**: `cargo check` clean, `cargo test` 330 passed, 0 failures

### 2026-04-23: BUG #2 - Premature `return` dropping concurrent tool calls — FIXED

- **File**: `src/fixes/toolcall_bad_filepath_fix.rs` line 389
- **Problem**: In `apply_stream_with_accumulation_default`, when `accumulator.is_fixed(index)` returns true for a pre-fixed index, the method called `return (chunk, FixAction::NotApplicable)` which exits the entire method. This skipped all remaining tool calls in the SAME chunk (indexes 1, 2, etc. never got checked for malformation).
- **Fix**: Replaced `return (chunk, FixAction::NotApplicable);` with `continue;` so the loop processes all tool calls in the chunk. Already-fixed indexes are suppressed (arguments set to empty string) but the method continues to check remaining indexes.
- **Verification**: `cargo check` clean, `cargo test` 330 tests, 0 failures

### 2026-04-23: BUG #12 - `active_requests` uses `Ordering::Relaxed` everywhere — FIXED

- **Files**: `src/backends/balancer.rs` (lines 20, 26, 33), `src/backends/priority_free.rs` (lines 33, 41)
- **Problem**: `active_requests` atomic counter used `Ordering::Relaxed` everywhere. On x86 (TSO) this works fine, but on ARM/RISC-V weakly-ordered platforms, threads can see stale `active_requests` values, causing `PriorityFreeBalancer` to consistently pick wrong nodes.
- **Fix**:
  - `balancer.rs` line 20: `fetch_add(1, Relaxed)` → `fetch_add(1, Release)` (new increment releases to other threads)
  - `balancer.rs` line 26: `fetch_add(1, Relaxed)` → `fetch_add(1, Release)` (same for `with_group` path)
  - `balancer.rs` line 33: `fetch_sub(1, Relaxed)` → `fetch_sub(1, AcqRel)` (decrement both releases and acquires)
  - `priority_free.rs` line 33: `load(Relaxed)` → `load(Acquire)` (check for free node)
  - `priority_free.rs` line 41: `load(Relaxed)` → `load(Acquire)` (min_by_key selection)
- **Verification**: `cargo check` compiles clean, `cargo test` 330 passed, 0 failures

### 2026-04-23: BUG #6 - Model/temperature overrides dropped when augmentation is active — FIXED

- **File**: `src/proxy/handler.rs` lines 476-514
- **Problem**: The `handle()` method had two branches for building `final_body_bytes`:
  - **Augmentation branch** (when `enriched_body_bytes != body_bytes`): Set `stream: false` and stripped `stream_options`, but NEVER applied `backend.model` or `backend.temperature` overrides
  - **Non-augmentation branch**: Set `stream: false`, stripped `stream_options`, AND applied model + temperature overrides
  - Result: When augmentation was active, configured model/temperature overrides were silently ignored
- **Fix**: Extracted common logic into `Self::apply_backend_overrides_bytes(body: &[u8], backend: &BackendNode) -> Vec<u8>` helper that:
  1. Sets `stream: false`
  2. Strips `stream_options`
  3. Applies `backend.model` override if configured
  4. Applies `backend.temperature` override if configured
  Both branches now call this single helper, ensuring identical behavior regardless of augmentation.
- **Verification**: `cargo check` clean, `cargo test` 330 passed, 0 failures
- **Note**: The `client_wants_streaming` debug log from the non-augmentation branch was removed as it was noise (the streaming mode is always `fake` anyway — backend always gets non-streaming requests).

### 2026-04-23T[fix]: BUG #4 - `rfind` matches wrong occurrence in delta calculation — FIXED

- **File**: `src/fixes/toolcall_bad_filepath_fix.rs` lines 186-204 (calculate_completion_delta)
- **Problem**: In the TIER 2 fallback, `accumulated.rfind(current_chunk)` returns the position of the last occurrence of `current_chunk` in `accumulated`. When the chunk text appears multiple times (e.g., common file path fragment repeated across tool call arguments), `rfind` points to the wrong boundary. This causes `already_sent_len` to be wrong — the client receives a delta that either truncates accumulated text or corrupts it with partial JSON.
- **Fix**: Added end-boundary verification: `if pos + current_chunk.len() == accumulated.len()`. Only use the `rfind` result if the match ends exactly at the string boundary (meaning the client legitimately has the full chunk). Otherwise fall through to `safe_completion()`.
- **Before**: `rfind` returned `pos` unconditionally — ambiguous matches silently corrupted deltas.
- **After**: `rfind` result validated against end boundary; ambiguous matches fall to safe completion (`"}.to_string()` or `""_":null}".to_string()`).
- **Verification**: `cargo check` clean, `cargo test` 330 passed, 0 failures

### 2026-04-23: BUG #5 - Dead `remove_duplicate` field in ToolcallBadFilepathFix — FIXED

- **File**: `src/fixes/toolcall_bad_filepath_fix.rs` (struct definition, constructor, tests)
- **Also modified**: `src/fixes/registry.rs` (configure method, test callers), `src/fixes/mod.rs` (default registry), `src/proxy/streaming.rs` (integration tests), `README.md` (config example + code example)
- **Problem**: The `remove_duplicate: Arc<AtomicBool>` field, `set_remove_duplicate()` method, and `#[allow(dead_code)]` annotation were dead code. The fix always truncates after the first `filePath` occurrence regardless of this flag. The config option `remove_duplicate: true` had no effect and confused users.
- **Fix**:
  1. Removed `use std::sync::atomic::AtomicBool` and `use std::sync::Arc` imports
  2. Changed struct from `ToolcallBadFilepathFix { remove_duplicate: Arc<AtomicBool> }` to `ToolcallBadFilepathFix {}`
  3. Changed `new(remove_duplicate: bool)` to `new()` with `Self {}` init
  4. Removed `set_remove_duplicate(&self, _value: bool)` no-op method
  5. Removed dead tests: `test_new_with_remove_duplicate`, `test_set_remove_duplicate`
  6. Updated `test_fix_keep_duplicate_mode` to use `new()` (flag no longer exists)
  7. Replaced 49 `ToolcallBadFilepathFix::new(true/false)` → `ToolcallBadFilepathFix::new()` across all files
  8. Cleaned `FixRegistry::configure()` to remove dead `set_remove_duplicate` config handling
  9. Removed `remove_duplicate: true` from README.md config example
  10. Fixed unused `fix` variable warning in registry.rs `configure()` (changed `find` → `any`)
- **Verification**: `cargo check` clean, `cargo test` 328 passed, 0 failures
- **Note**: `config.yaml.default` and `src/config/mod.rs` still have `remove_duplicate` in config structs/options HashMaps. These are harmless — the HashMap accepts arbitrary keys, and `configure()` now silently ignores unknown options. The config deserialization still works.

## 2026-04-23T19:00:00Z: FINAL STATUS — All Plan Tasks Complete

### Completed Bug Fixes (9/9)
| Bug | Severity | File | Status |
|-----|----------|------|--------|
| #1 UTF-8 panic | CRITICAL | toolcall_bad_filepath_fix.rs, mod.rs | ✅ |
| #2 Premature return | CRITICAL | toolcall_bad_filepath_fix.rs | ✅ |
| #3 {} garbage key | CRITICAL | toolcall_malformed_arguments_fix.rs | ✅ |
| #4 rfind wrong match | CRITICAL | toolcall_bad_filepath_fix.rs | ✅ |
| #5 Dead remove_duplicate | LOW | toolcall_bad_filepath_fix.rs, registry.rs | ✅ |
| #6 Model overrides dropped | HIGH | handler.rs | ✅ |
| #7 AnthropicUsage null crash | HIGH | api/openai.rs | ✅ |
| #8 ToolResult data loss | HIGH | api/openai.rs | ✅ |
| #9 group_name missing | HIGH | influxdb.rs | ✅ |
| #12 Relaxed ordering | MEDIUM | balancer.rs, priority_free.rs | ✅ |
| #15 Content type false negatives | MEDIUM | handler.rs | ✅ |

### Remaining Audit Items (Deferred to separate plan)
- BUG #13: context_total overestimates (MEDIUM)
- BUG #14: metrics streaming always false (MEDIUM)
- BUG #16: aggressive_parse_json regex nesting (MEDIUM)
- BUG #17: PriorityFree check-then-act race (MEDIUM)
- BUG #18: duplicate catch-all groups (MEDIUM)
- BUG #19: stale counter after crash (MEDIUM)
- BUG #20-27: LOW priority cleanup items

### Build/Test Results
- cargo build: ✅ exit 0
- cargo test: ✅ 328 passed, 0 failed
- cargo check: ✅ 0 errors, 0 warnings
- Files changed: 10 files, +120/-160 lines
