# High-Level — why these dependencies, and what we rejected

Short rationale for dependency and architecture choices. Keep entries brief: deep technical
detail belongs in `distilled.md`, failure modes in `pitfalls.md`.

<!-- Entry shape:

## <Area — e.g. HTTP framework>

The deciding constraint, in one or two sentences.

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **chosen-lib** | … | … | **Chosen** (used in: <component>) |
| alternative | … | … | Rejected — <one-line reason> |

**Revisit if:** <the condition under which this choice should be re-opened>.
-->

---

## Engine for this sub-project — Basalt, for Flash-Next only

Decided at init (owner, 2026-11-04). The request was a Basalt container on `:8700`; Basalt runs
only Qwen3.8-Flash-Next, so the dense 27B QUASAR `.ninfer` artifact first named was moved out.

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **Basalt + a Flash-Next `.basalt` pack** | What the engine is built for; one-file packs; VRAM + RAM expert tiers; OpenAI/Anthropic API | Flash-Next only; young (0.2.x), flags change without shims | **Chosen** |
| Basalt + the 27B QUASAR `.ninfer` | — | Basalt cannot load it | Rejected — wrong engine |
| Run the 27B QUASAR in `../ninfer` instead of building Basalt | Already has a 27B profile and a container | Not what was asked for here | Out of scope here — a `../ninfer` plan if wanted |

**Revisit if:** Basalt adds a dense-27B or `.ninfer` loader, or the owner wants the QUASAR 27B
served (that goes to `../ninfer`).
