# Plan Updates & Corrections

**Date**: Thu Aug 13 2026  
**Purpose**: Document what to update, drop, or correct in the plans based on current source code

---

## ✅ Already Correct (No Changes Needed)

The plans correctly identify:
- Context comes from `/props` + `/v1/models` (not `/slots`)
- `/metrics` is a pass-through (use `/proxy/metrics` instead)
- `ProxyState` is a struct literal (no `::new()`)
- Two call sites for `fetch_context_total`
- `e2e/` crate is the test harness (not `tests/integration/`)
- `ServerConfig` has no serde defaults
- `toolcall_null_index` config key mismatch bug

---

## 🗑️ Items to DROP from Plans

### 1. **Early Exit Optimization in Fix Registry** (Priority 2)
**Status**: NO-OP  
**Reason**: `applies()` already provides early exit. The `modified` flag in the draft was never used.  
**Action**: Remove this item entirely from the plan.

### 2. **Per-Client Backoff** (Priority 1.3, Step 4)
**Status**: DROPPED  
**Reason**: Neither Claude Code nor Opencode sends `X-Client-ID` header, so backoff would become global lockout.  
**Action**: Remove Step 4 from 1.3.

### 3. **`stats.fetch_context` Config Option** (Priority 2.3)
**Status**: DOESN'T EXIST  
**Reason**: `StatsConfig` only has `{ enabled, format, log_interval }`.  
**Action**: Remove mention of this option from config updates.

### 4. **`dashmap` Dependency** (Throughout)
**Status**: NOT NEEDED  
**Reason**: Context caching already exists in `context.rs` using `OnceLock<RwLock<HashMap>>`.  
**Action**: Remove all mentions of `dashmap` from the plan.

### 5. **Streaming Mode Startup Warning** (Priority 2.2)
**Status**: DROPPED  
**Reason**: `StreamingMode` is `Disabled | Fake | Accumulator`. Default is `Fake`. Warning would be noise.  
**Action**: Remove Priority 2.2 entirely.

---

## 🔧 Items to UPDATE

### 1. **Priority 1.3: Concurrent Limiting**

**Current text says**: "Check concurrent limit at `handler.rs:264`"

**Correction**: Check must be **AFTER** the pass-through match at `handler.rs:317-337`, not before.

**Why**: If checked before, `/health`, `/props`, `/slots`, `/v1/models`, `/metrics` all get 429'd at capacity—exactly when monitoring needs them.

**Update the code snippet**:
```rust
// WRONG (before routing):
if current >= max { return 429; }

// RIGHT (after pass-through match):
match (&method, path) {
    // ... pass-through routes ...
}
// NOW check limit for completion routes only:
if is_completion_route && current >= max { return 429; }
```

---

### 2. **Priority 1.2: Context Fetch Failure Logging**

**Current text**: "Add warning at both call sites"

**Missing**: **Negative caching is required** to prevent log spam.

**Update**: Add this requirement:
```rust
// Add WARNED_BACKENDS set to track which URLs we've warned about
static WARNED_BACKENDS: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();

pub async fn warn_context_fetch_failed_once(backend_url: &str, model: &str) {
    let warned = WARNED_BACKENDS.get_or_init(|| RwLock::new(HashSet::new()));
    // Check if already warned
    if warned.read().await.contains(backend_url) { return; }
    // Mark as warned and log
    warned.write().await.insert(backend_url.to_string());
    tracing::warn!(...);
}
```

**Why**: Without this, a backend that doesn't support `/props` or `/v1/models` will spam one warning **per request** forever.

---

### 3. **Priority 1.4: Config Key Mismatch**

**Current text**: "Rename `toolcall_null_index_fix` to `toolcall_null_index`"

**Better option**: **Accept both spellings** in `registry.rs:229` for backward compatibility:

```rust
pub fn configure(&mut self, config: &HashMap<String, FixModuleConfig>) {
    for (name, module_config) in config {
        // Normalize: strip trailing "_fix" for comparison
        let normalized = name.strip_suffix("_fix").unwrap_or(name);
        for fix in &self.fixes {
            let fix_name = fix.name().strip_suffix("_fix").unwrap_or(fix.name());
            if normalized == fix_name {
                self.enabled.insert(name.clone(), module_config.enabled);
                break;
            }
        }
    }
}
```

**Why**: Some users may have already configured `toolcall_null_index_fix` in their configs. Accepting both is safer.

---

### 4. **Priority 1.1: Metrics Endpoint Placement**

**Current text**: "Add `/proxy/metrics` arm before the pass-through match"

**Correction**: The arm should be placed **AFTER** backend selection OR made independent of it.

**Why**: If placed before backend selection and the code tries to call `load_balancer.select()`, it will 404 when no backend matches the (missing) model field.

**Update**: Either:
1. Make `/proxy/metrics` independent of backend selection (check path first, return early)
2. Place it after the pass-through match but before the completion routes

---

### 5. **Testing Section (1.3)**

**Current text**: "Use `e2e/` crate"

**Missing detail**: `e2e/` is a **separate crate** with its own `Cargo.toml` and `Cargo.lock`, not a workspace member.

**Update**: Add this note:
```
⚠️ `e2e/` is a standalone crate, not part of the workspace. Run tests with:
  cd e2e && cargo test
NOT with `cargo test` from the root.
```

---

## 📋 Summary of Changes

| Item | Action | Priority |
|------|--------|----------|
| Early exit optimization | Drop | Low |
| Per-client backoff | Drop | Low |
| `stats.fetch_context` option | Drop | Low |
| `dashmap` dependency | Drop | Low |
| Streaming warning | Drop | Low |
| Concurrent limit placement | Update | **HIGH** |
| Negative caching | Update | **HIGH** |
| Config key normalization | Update | Medium |
| Metrics endpoint placement | Update | Medium |
| `e2e/` crate note | Update | Low |

---

## 🎯 Recommended Order

1. **Update Priority 1.3** (concurrent limit placement) - Critical for correctness
2. **Update Priority 1.2** (negative caching) - Critical for avoiding log spam
3. **Update Priority 1.4** (config key) - Medium priority, fixes real bug
4. **Update Priority 1.1** (metrics placement) - Low priority, minor detail
5. **Drop all NO-OP items** - Cleanup
6. **Add `e2e/` note** - Documentation

---

## ✅ What's Already Correct

The plans got these right:
- ✅ Context from `/props` + `/v1/models` (not `/slots`)
- ✅ Use `/proxy/metrics` (not `/metrics`)
- ✅ `ProxyState` is struct literal (no `::new()`)
- ✅ Two call sites for `fetch_context_total`
- ✅ `ServerConfig` has no defaults
- ✅ `toolcall_null_index` key mismatch
- ✅ `e2e/` is the test harness
- ✅ `ProxyHandler` is not Clone

---

## 📝 Final Checklist

Before implementing, verify:
- [ ] All dropped items removed from plan
- [ ] Concurrent limit placement corrected
- [ ] Negative caching requirement added
- [ ] Config key normalization option added
- [ ] Metrics endpoint placement clarified
- [ ] `e2e/` crate note added
- [ ] All `dashmap` mentions removed
- [ ] All `stats.fetch_context` mentions removed

---

**Ready to implement?** Start with the HIGH priority updates first, then drop the NO-OP items.
