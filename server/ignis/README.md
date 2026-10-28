# ignis — Ignis for Qwen3.8 Flash-Next and 27B on the RTX PRO 6000

[Ignis](https://github.com/gpillon/ignis) (a single-GPU Rust + C++/CUDA engine for Qwen3.8 that
targets `SM120a` only), run from the pinned upstream image `ghcr.io/gpillon/ignis:0.8.0`, serving
Qwen3.8-Flash-Next or the Qwen3.8-27B on `:8700` as **`cosmo-6000`** — the port and model name
`llama-proxy` already sends to the vLLM daily driver in `../vllmcustom`, so the proxy needs no
change. Ignis's built-in `rtxpro6000` profile sets the speed defaults (Flash-Next: MTP with 3
draft tokens, 8 lanes, every expert in VRAM; 27B: DFlash2 with 7 draft tokens).

**Status:** under construction (Plan 01, `context/plans/`). Nothing here has been pulled,
downloaded or booted yet; every number this project cites will live in the plan tracker's
Evidence table. Upstream's published numbers (Flash-Next ~258 tok/s at one lane on code, measured
on a Server Edition PRO 6000) are theirs, not ours.

## Prerequisites

- Docker reachable **as yourself** — no `sudo` anywhere. Every script refuses a `DOCKER` that
  names `sudo` and refuses to run as root. Containers run as uid/gid `1000:1000`.
- The RTX PRO 6000 as host GPU 0 (`nvidia-smi -L`); GPU 1, the RTX 4060, is never used. A driver
  that runs CUDA 13.1 (615.78.08 on 2026-10-27).
- Host RAM: Flash-Next keeps every expert in VRAM on this card but holds its 28.8 GB n-gram table
  in RAM when ~37 GB is available (~9 GB is the minimum). The 27B pins its 1.35 GB token
  embedding, 3.72 GB of retained slots and the 2 GiB KV-RAM arena, and keeps a 4 GiB margin:
  ~12 GB available (arithmetic over upstream's `docs/user/configuration/host-ram.md`).
- `models` → `/mnt/noir/scratch/ai/llm/models` (the siblings' model tree); the Ignis artifacts
  live flat in `models/ignis/`.
- **One server on `:8700` at a time**: stop vLLM first (`docker stop vllm`). Both want the whole
  card.

## Download the weights

**Run these yourself** — agents never start a download (they are 18–68 GiB). Pick a form; both
fetch exactly the revisions Ignis 0.8.0's own catalog pins (`versions.env`). Nothing is gated: no
Hugging Face token is needed.

| Model | Repo @ revision | Files | Size | License |
|---|---|---|---|---|
| Flash-Next (125B MoE, trellis 2.5 bpw) | `gpillon/Qwen3.8-Flash-Next-trellis-a25-ignis` @ `8d94db83` | `qwen3_8_flash_next_trellis_a25-v2.ninfer`, `qwen3_8_flash_next_mtp_3p0-v2.ninfer` (the MTP companion — required, the profile turns MTP on), each with its `.conversion.json` | 67.80 GiB | Qwen Community License 1.0 |
| 27B (NVFP4 + DFlash2) | `gpillon/Qwen3.8-27B-nvfp4full-dflash2-NInfer` @ `e961b419` | `qwen3_8_27b_nvfp4full-v2.ninfer` + `.graft.json` (required: the load is refused without it) | 18.07 GiB | apache-2.0 |
| 27B, abliterated (optional) | `gpillon/Qwen3.8-27B-nvfp4full-dflash2-abliterated-NInfer` @ `ef321694` | `qwen3_8_27b_nvfp4full-v2-huihui-abliterated.ninfer` + `.graft.json` | 18.07 GiB | apache-2.0 — **it does not refuse**; never expose it to untrusted users |

Flash-Next's license asks for a separate Qwen license only for "Model-as-a-Service" / "AI Work
Assistant" businesses; internal use is exempt.

Create the directory as yourself first — `docker` would create a missing bind source as root:

```bash
mkdir -p models/ignis
```

### Form 1 — through the Ignis image (recommended: verifies every file)

`ignis-server model download` fetches the catalog's pinned files, resumes a broken transfer
(`.part` files), and checks each file's size and SHA-256 before renaming it into place. It needs
no GPU.

```bash
IMG=$(. ./versions.env && echo "$IGNIS_IMAGE")
docker run --rm --user 1000:1000 -e HOME=/tmp -v "$PWD/models/ignis:/models" "$IMG" \
  model download qwen3.8-flash-next --out /models
docker run --rm --user 1000:1000 -e HOME=/tmp -v "$PWD/models/ignis:/models" "$IMG" \
  model download qwen3.8-27b --out /models
# optional:
docker run --rm --user 1000:1000 -e HOME=/tmp -v "$PWD/models/ignis:/models" "$IMG" \
  model download qwen3.8-27b-abliterated --out /models
```

### Form 2 — the Hugging Face CLI

Needs the CLI (`pip install -U huggingface_hub` gives `hf`; older installs call it
`huggingface-cli download` with the same arguments). Files are named one by one, so the command
does not depend on either tool's `--include` syntax; each repo ships a `.sha256` per artifact for
`sha256sum -c`.

```bash
# Flash-Next (67.80 GiB)
hf download gpillon/Qwen3.8-Flash-Next-trellis-a25-ignis \
  --revision 8d94db83cd8e58d6be31e796723e5e35b960638c --local-dir models/ignis \
  qwen3_8_flash_next_trellis_a25-v2.ninfer qwen3_8_flash_next_trellis_a25-v2.ninfer.conversion.json \
  qwen3_8_flash_next_trellis_a25-v2.ninfer.sha256 \
  qwen3_8_flash_next_mtp_3p0-v2.ninfer qwen3_8_flash_next_mtp_3p0-v2.ninfer.conversion.json \
  qwen3_8_flash_next_mtp_3p0-v2.ninfer.sha256

# 27B (18.07 GiB)
hf download gpillon/Qwen3.8-27B-nvfp4full-dflash2-NInfer \
  --revision e961b419b672e183aa55df8c4b975abc82006e8a --local-dir models/ignis \
  qwen3_8_27b_nvfp4full-v2.ninfer qwen3_8_27b_nvfp4full-v2.ninfer.graft.json \
  qwen3_8_27b_nvfp4full-v2.ninfer.sha256

# 27B abliterated (optional, 18.07 GiB)
hf download gpillon/Qwen3.8-27B-nvfp4full-dflash2-abliterated-NInfer \
  --revision ef3216949e42c42fc713feffb2e05849d57f0f3b --local-dir models/ignis \
  qwen3_8_27b_nvfp4full-v2-huihui-abliterated.ninfer \
  qwen3_8_27b_nvfp4full-v2-huihui-abliterated.ninfer.graft.json \
  qwen3_8_27b_nvfp4full-v2-huihui-abliterated.ninfer.sha256

# verify (reads every byte — minutes on NFS)
(cd models/ignis && sha256sum -c ./*.sha256)
```

`hf download --local-dir` also leaves a `.cache/huggingface/` directory inside `models/ignis/`;
it is harmless.

## Serve

```bash
docker stop vllm                       # one server on :8700 at a time
PRINT_CONFIG=1 ./qwen38fn/run          # optional first: every flag checked by the pinned binary,
                                       #   effective config printed — no GPU, no model, vLLM may stay up
./qwen38fn/run                         # Flash-Next on :8700 as cosmo-6000 (foreground; Ctrl-C drains)
DETACH=1 ./qwen38fn/run                # the same, in the background: docker logs -f ignis
```

The first run pulls the pinned image if it is not local. The launcher refuses — before any
`docker run` — while anything publishes `:8700` (or a `vllm`/`ignis` container runs), when
`GPU` is not the RTX PRO 6000, and when an artifact or sidecar is missing (it prints the
`hf download` command). It echoes the full `docker run` line first.

| Knob | Default | What |
|---|---|---|
| `PORT` | `8700` | host port (the container listens on 8000) |
| `SERVED` | `cosmo-6000` | the one model id served |
| `GPU` | `0` | host index of the RTX PRO 6000 (`nvidia-smi -L`) |
| `MODELS_DIR` | `models/ignis` | the flat artifact directory, mounted read-only |
| `CACHE_DIR` | `storage/cache` | n-gram rows + KV-disk (local disk, not NFS) |
| `CONTEXT` | `262144` | the native context: no YaRN. `1048576` is the profile's (YaRN x4 on every request) |
| `IDLE_UNLOAD` | `0` | seconds idle before unloading; `0` never (Ignis's own default is 300) |
| `SPEC_TOKENS` | profile (Flash-Next MTP 3) | draft tokens |
| `LANES` | profile (8) | decode lanes |
| `DETACH` | — | `1` = run in the background |
| `PRINT_CONFIG` | — | `1` = `ignis-server config print` with the same flags, then exit |
| `EXTRA_ARGS` | — | any `ignis-server` flags, appended last (last wins), e.g. `--model-loop-guard off` |

- `./qwen38-27b/run` — the 27B on `:8700` (`ABLITERATED=1` for the abliterated weights)
  *(Plan 01 T3)*.

Check it is up: `curl -s localhost:8700/ready` (200 once the model is loaded) and
`curl -s localhost:8700/v1/models`.

**One model name.** Ignis 0.8.0 serves exactly one id, here `cosmo-6000`. The other names the
vLLM driver also answers to (`cosmo-proxy`, `claude-heavy`, `claude-light`,
`claude-haiku-4-5-20251001`) get a `404 model_not_found`. `llama-proxy` sends only `cosmo-6000`;
a request with no `model` is served by the loaded model.

## Layout

| Path | What |
|---|---|
| `versions.env` | every pin: the image by tag and digest, its source commit, the HF repos, revisions and artifact names |
| `lib/common.sh` | shared guards (DOCKER/sudo/root/socket) and refusals (busy port, wrong GPU, missing files), sourced by every script |
| `models` | symlink to the shared model tree; the artifacts go in `models/ignis/` |
| `storage/` | the launchers' writable cache (n-gram rows, KV-disk) — gitignored |
| `captures/` | bench output and logs — gitignored |
| `context/` | the project's knowledge base and plans (`context/plans/RULES.md` governs work) |
