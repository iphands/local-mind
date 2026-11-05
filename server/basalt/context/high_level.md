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

## The image — two stages, upstream's source-tree layout at `/opt/basalt`

`basalt serve` chdirs into the source dir compiled into the binary and runs `python -m serve.server`
there; `serve/` imports `tools/model_file.py`; the default expert profile is `data/profiles/`. So the
build runs in `/opt/basalt` and the runtime image ships `bin/ serve/ tools/ data/ .venv` there.

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **devel build stage → runtime stage keeping `/opt/basalt`** | 3.4 GB image, no compiler at runtime, upstream's layout | must copy exactly what `serve` needs (it missed `tools/` once — pitfalls.md) | **Chosen** (Plan 01) |
| single devel-stage image | nothing can be missing | CUDA-devel size, compiler at runtime | Rejected |
| `cmake --install` to `/usr/local` | conventional | no install target for this layout; the compiled-in source dir would be gone | Rejected |

**Revisit if:** Basalt gains an install target or stops resolving paths from its source dir.

## Launcher — command-line flags, not a generated `basalt_config.json`

Every setting can be given on the command line and `--api-key` must be; Basalt prints what it
received (`[server] engine … (from cli)`). One place holds the defaults: `./run`.

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **flags built by `./run`** | one source of defaults, printed before start, checked by gate step 5 | long command line | **Chosen** |
| a checked-in config file | readable | a second copy of the defaults; another mount | Rejected |

**Revisit if:** the settings outgrow env knobs.

## Where the pack lives — local NVMe (`models.local/`), not the NFS share

Basalt uploads weights with `O_DIRECT`; from noir's NFS share boot 2 ran at 5–10 MB/s, from local
NVMe boot 3 loaded in 40 s (`distilled.md`). Owner decision 2026-11-04.

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **local NVMe copy, gitignored `models.local/`** | 34–40 s boots | 131.7 GB of `/home` | **Chosen** |
| NFS share (`models/vllm/jesdga/…`) | no local copy | `O_DIRECT` reads; minutes to hours unless defragmented and cached | Rejected as default (`PACK=` still works) |

**Revisit if:** a boot from the defragmented NFS copy (927 extents) proves fast enough.

