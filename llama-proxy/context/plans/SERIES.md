<!-- hands-plan:v1 2026-10-16 -->
# llama-proxy — Plan Series

Dependency chain and status of every plan. **Read this before creating a plan. Update it whenever
a plan is added, starts, completes, or is abandoned** — in the same commit as that change.

Plan status: `pending` | `in-progress` | `blocked` | `done` | `abandoned`.
Plans further out are intentionally light: an untouched pending plan is a **hypothesis, not a
contract** — re-read and revise it before starting (RULES.md Rule C).

**Next free plan number: `03`.** <!-- hp:next-free -->

> **North star (user directive, 2026-10-16):** An OpenAI-compatible reverse proxy for LLM
> backends (VLLM, llama.cpp); fixes malformed model responses and collects/exports inference
> metrics.
>
> **Ordering principle:** Measure before optimizing.

## Currently Active <!-- hp:active -->

<!-- One line per in-progress plan, in the order they should be worked:
     - **Plan NN** — <title>: in progress, X/Y tasks done; next TN (<short title>).
     When nothing is in progress: "None in progress — next up: Plan NN (`/hands-plan resume NN`)." -->

None in progress — next up: Plan 03 (`/hands-plan resume 03`).

## Plans <!-- hp:plans -->

Keep **Notes** to ~25 words — the detail lives in the plan's tracker.

| Plan | Title | Depends on | Status | Notes |
|---|---|---|---|---|
| 01 | stats columnar + prefill fix | N/A | done | columnar compact stats + corrected prefill TPS |
| 02 | vllm-first docs + read-only vendor discovery | N/A | done | vLLM-first docs landed; `/vendor` read-only discovery + P3 rule; stale `PLAN.md` OBSOLETE-stamped |

## Abandoned / Superseded <!-- hp:abandoned -->

| Plan | Reason | Superseded by | Date |
|---|---|---|---|

*(Record the reason. A plan dropped without one gets re-attempted six months later by someone who
doesn't know it failed.)*

## Standing Constraints <!-- hp:constraints -->

Follow-ups carried from Plan 02 (full detail in `completed/02_vllm_first_docs_tracker.md`); recorded
here so they don't orphan when the tracker archives:

| Constraint / owed decision | Why it must not be lost | Owner / where |
|---|---|---|
| Delete `PLAN.md` only after **human** review — currently OBSOLETE-stamped, kept for history | append-only ethos; deletion is a human call, never an agent's | human; next resync |
| Resolve the 3× dead `../context/opencode_claude_llama_notes.md` reference (`CLAUDE.md` L93/L284/L404) — the file exists **nowhere** (deleted, not mis-pathed) | cited as "the comprehensive client notes"; a dead pointer an agent will trust and chase; needs a content call, not a path edit | human content decision (all 3 refs sit outside the hands-plan managed block) |

Deferred to a later **code/docs plan** (not forgotten, tracked in the archived tracker's Follow-ups):
the shipped-default surface still names llama.cpp — `src/main.rs:47` CLI `about` string and
`config.yaml.default:19/21` — plus deepening the e2e mock's llama.cpp-shaped default payloads; and the
authoritative gate fix (`justfile:16` / `RULES.md:122/124` `-- -D warnings`, needs human sign-off).

---

Completed plans move to `completed/`, abandoned ones to `abandoned/` (RULES.md Rule C). When this
file passes ~30 KB, move the narrative of fully closed series to `completed/SERIES_ARCHIVE.md` and
leave one summary row per series here.
