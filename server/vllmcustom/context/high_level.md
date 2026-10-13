# High-Level — why these dependencies, and what we rejected

Short rationale for dependency and architecture choices. Keep entries brief: deep technical
detail belongs in `distilled.md`, failure modes in `pitfalls.md`. Most of the long version already
lives in `README.md` — cite the section instead of copying it.

## CUDA toolkit: 13.2.1, not the newest and not upstream's

The deciding constraint is that this repo compiles torch, FlashInfer and vLLM itself, so the toolkit
has to be one all three are tested against (`container/versions.env:21-25`).

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **13.2.1** | newest toolkit torch 2.13.0 officially builds against | nothing else has CI on it | **Chosen** (default `CUDA_VERSION`) |
| 13.3.x | published; host driver meets its floor | absent from torch's binary matrix; nvcc changed its dry-run output and broke sccache | Rejected |
| 13.0.3 | what vLLM's own images are CI-built on | older | Kept as the deliberate fallback → its own `cu1303-sm120` tag |

**Revisit if:** torch adds a CUDA 13.3 column, or FlashInfer/vLLM move their CI.

## Everything perf-critical from source, torchvision from a wheel

The whole point of the project: upstream wheels do not emit `sm_120` for what matters
(`README.md` § *sm120-only: what the build actually emits* — FA2 came out as sm_80 SASS + PTX, FA3
as sm_75).

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **torch / FlashInfer / vLLM / torchaudio built from source** | real `12.0` / `12.0f` codegen; the FA2 patch can hook in | hours-long first build, and a vLLM bump drags torch + FlashInfer with it | **Chosen** |
| Prebuilt image (`run-legacy`) | zero build | cannot be patched or re-tagged; it is what the model card measured, so it stays for A/B | Kept, not for daily use (`qwen3.8-flash-next/run-legacy`) |
| torchvision from source | consistency | pointless — it is not perf-critical and the `+cu132` wheel exists | Rejected (wheel, `--no-deps`) |

**Revisit if:** upstream ships sm_120-native wheels for all three, which would make the whole
`container/` directory optional.

## A snapshot commit, never the moving tip

The experimental set is built from `VLLM_MAIN_SHA`, a specific commit, not from `main`.

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **Pinned sha in `container/versions.env`** | reproducible build; the flashinfer pin gets re-checked on purpose; the image's `ai.vllmcustom.vllm.ref` label makes the running container name its own source | a bump is manual work | **Chosen** |
| Track `origin/main` | always newest | a build cannot be reproduced, and the snapshot tag would lie about what it contains | Rejected |
| Snapshots under `:latest` too | convenience | the release launchers would pick up an experimental image | Rejected — snapshots never move `:latest` |

**Revisit if:** a `v0.30.0`/`v0.31.0` tag lands with the features the snapshot was taken for — then
the snapshot stops being the reason it exists. Details: `README.md` § *Snapshot builds from `main`*,
runbook: `repin.md`.

## One launcher per model family, shared plumbing in `common/`

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **`<family>/run` + `common/_run-lib.sh`** | each model owns its parsers, sampling, patches; the risky parts (image resolution, the `/models` bind mount, the `vllm serve` core flags) exist once | a strict call order, with three rules that break silently (`context/pitfalls.md`) | **Chosen** |
| One launcher + per-model config file | fewer files | every model's quirks become conditionals in one script, and the patches still need per-model mounting | Rejected |
| Copy the launcher per family | zero coupling | four copies of the bind-mount logic, and a wrong bind mount fails silently rather than failing | Rejected |

**Revisit if:** a family needs something `common/` cannot express without a special case — that is
the signal to widen the shared library, not to fork the launcher.

---
