## 2026-04-23T17:58:00Z: Design decisions for bug fixes

### Decision 1: Fix ordering — Crash-first, then correctness, then accuracy
- CRITICAL bugs (panics, data corruption) must be fixed first
- HIGH bugs (data loss, 502 errors) are next
- MEDIUM/LOW can be deferred if needed

### Decision 2: Minimal, targeted fixes over refactoring
- Each fix should be a surgical change to the specific bug
- No large-scale refactoring unless it directly prevents future bugs
- Preserve existing API and behavior for unchanged code

### Decision 3: Test-first approach for each fix
- Before making changes, run existing tests to establish baseline
- After changes, run tests to verify no regression
- Add new tests specifically for the bug being fixed
- For streaming fixes, verify with the e2e test framework

### Decision 4: Fix #2 (premature return) requires careful design
- Cannot simply replace `return` with `continue` — the function returns a tuple
- Need to redesign the early-exit logic to per-index suppression
- Will use a flag-based approach: `fix_applied` flag to control post-fix behavior
