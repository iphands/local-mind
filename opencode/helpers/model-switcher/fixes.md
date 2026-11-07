# Model Switcher - Fixes Log

Tracking fixes for future regression tests.

## Fix 1: Remove vim j/k navigation

**Problem:** Had vim-style j/k keys for up/down navigation which conflicted with filtering (typing 'j' or 'k' would navigate instead of filter).

**Solution:** Removed `KeyCode::Char('k')` and `KeyCode::Char('j')` from navigation handling. Only arrow keys (Up/Down) navigate now.

**Test:** Type 'j' or 'k' - should appear in filter, not move selection.

---

## Fix 2: Filter uses contains instead of starts_with

**Problem:** Filtering used `starts_with` matching, so typing "kimi" wouldn't find "moonshotai/kimi-k2".

**Solution:** Changed filter to use `contains` matching (case-insensitive).

**Test:** Type "kimi" - should match models like "moonshotai/kimi-k2-thinking-turbo".

---

## Fix 3: Escape aborts all (no partial writes)

**Problem:** Escape would skip the current agent but continue to the next, and changes were written immediately per-agent.

**Solution:** Collect all selections in memory first. Only write to disk after all agents have models selected. Escape at any point aborts everything with no disk writes.

**Test:**
1. Select model for first agent, press Esc on second agent - no files should be modified.
2. Complete all selections, then Esc on confirm - no files should be modified.

---

## Fix 4: Prominent agent name banner

**Problem:** It wasn't obvious which agent the user was selecting a model for.

**Solution:** Added large banner with agent name in uppercase, surrounded by `===` lines, colored yellow and bold. Also shows step progress (e.g., "Step 1/3").

**Test:** Visual - banner should be immediately visible and obvious.

---

## Fix 5: Up/down navigation works immediately

**Problem:** Arrow keys didn't work on initial load. The selection kept resetting to the current model on every render loop iteration.

**Solution:** Added `initialized` flag. Initial selection to current model only happens once, then arrow key movements are preserved.

**Test:** Launch program, immediately press Down arrow - selection should move down.

---

## Fix 6: Ledger of previous selections

**Problem:** No visibility into what was already selected for previous agents.

**Solution:** Show "Previous selections" section at top of screen showing each completed agent's selection. Shows old → new for changes, "(unchanged)" for same model.

**Test:** After selecting first agent's model, second agent screen should show the first agent's selection at the top.

---

## Fix 7: Confirmation before writing

**Problem:** No final review before changes were written to disk.

**Solution:** Added confirmation screen after all selections. Shows all changes with old → new. Enter/Y confirms (default), n/N/Esc aborts. No disk writes until confirmed.

**Test:**
1. Complete all selections, press Enter - changes written.
2. Complete all selections, press 'n' - no changes written.
3. Complete all selections, press Esc - no changes written.
