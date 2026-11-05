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

## Attesting the Basalt pin `dc047a77` — against the object store, never against upstream's narrative

**Audit-method decision taken during Plan 02 T2 (2026-11-05), pending owner ratification** — recorded here
because this is where decisions are audited for ownership. Made after three plausible attestation methods
were run and each returned a confident answer to a different question. The constraint: the pin we ship is
`jesdga95/basalt` `main`'s tip on the day it was pinned, **untagged**; its commit signature is GitHub's own
key on a **GitHub-created squash merge** and is unverifiable from this clone (`No public key`), basalt's
three release tags are annotated but **unsigned**, and the project declares a fork of `Niko1221/Strata`
whose published history has since been **rewritten** — so "diff it against upstream" is not a stable
operation.

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **GitHub's tree object for the pin, compared to our `pin^{tree}` + per-blob SHA both directions** | One request, no quota dependence beyond it, resolves a single changed file against the first parent | one authority attesting itself; the signature is the forge's, not a contributor's | **Chosen** — establishes "these are the objects GitHub serves", no more |
| Software Heritage as the third party | Independent of GitHub; holds the ancestor project and **our root commit** | holds no origin and no revision for `jesdga95/basalt` or the pin, so "nothing" is claimed only about what its listings enumerate (its snapshot ref lists cut off at exactly 1,000 refs) | **Partial** — corroborate where it exists |
| Diff / `log upstream..pin` / `cherry` / shared-object counts against `Niko1221/Strata` | looks like the standard answer | no merge-base exists, so `A..B` is just `B`; and in a *partial* clone, lazy-fetch makes our own objects look like shared ancestry (one run "found" 710 shared commits a pristine clone does not contain) | Rejected — wrong tool, fails plausibly rather than erroring |
| Asking the peer repo's commit API for our SHAs | cheap, no clone | GitHub resolves it against the repo's **object store**, not its default branch; produced a false fork boundary here | Rejected |
| Trusting commit messages, author fields, `Co-Authored-By` trailers, tag targets | free | the ancestor's published line **was** rewritten, and the only measured delta across byte-identical commit pairs is the removal of AI co-authorship trailers; GitHub's `fork=false` had already proved a weak signal | Rejected as evidence; kept as **reading-priority** signal only, disclosed as such |

**Before writing the sentence "this commit is upstream's",** read `audit/index.md` § *If you are about to
run something against upstream*; attest by object identity — `pin^{tree}` and per-blob SHA against the
forge's tree object for the same commit — and never by `log`/`cherry`/`bisect` against `Niko1221/Strata`,
nor by anything the forge's *metadata* implies about lineage. Before repeating the history-rewritten
finding, read `audit/wave0/t2-lineage.txt` § 0: four methods that produced confident wrong answers here.

**Revisit if:** each with the moment someone would actually look — at **every T13 pin bump**, (a)
`git tag -v` against a published key shows the new pin sitting on a **signed** release (then attestation is
one command); (b) the recorded Software Heritage origin lookup
(`audit/wave0/t2-lineage.txt` § 7) returns an origin for `jesdga95/basalt`; (c)
`git log --show-signature -20` on the new pin shows a non-zero count of contributor signatures rather than
forge signatures.

**Reuse:** the method — trust hash pins, trees, blobs and patch-ids, and name the anchor (snapshot hash +
date) beside every external claim — generalises to every vendored pin here; it is recorded for reuse in the
monorepo's `../../../context/pitfalls.md`. See `audit/index.md` § *If you are about to run something
against upstream* for the commands.

## Where the pack lives — local NVMe (`models.local/`), not the NFS share

Basalt uploads weights with `O_DIRECT`; from noir's NFS share boot 2 ran at 5–10 MB/s, from local
NVMe boot 3 loaded in 40 s (`distilled.md`). Owner decision 2026-11-04.

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **local NVMe copy, gitignored `models.local/`** | 34–40 s boots | 131.7 GB of `/home` | **Chosen** |
| NFS share (`models/vllm/jesdga/…`) | no local copy | `O_DIRECT` reads; minutes to hours unless defragmented and cached | Rejected as default (`PACK=` still works) |

**Revisit if:** a boot from the defragmented NFS copy (927 extents) proves fast enough.

