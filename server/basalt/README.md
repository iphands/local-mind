# basalt — Basalt in a container, on :8700

[Basalt](https://github.com/jesdga95/basalt) (a local inference engine for **Qwen3.8-Flash-Next**)
built from a pinned commit in our own image for the RTX PRO 6000 Blackwell (sm_120), and one
launcher, `./run`, that serves a `.basalt` pack on `:8700` with an OpenAI- and Anthropic-compatible
API.

Basalt loads only Flash-Next `.basalt` packs. `.ninfer` artifacts (for example the QUASAR 27B)
belong in `../ninfer`.

**Status (2026-11-20):** built on Basalt v0.5.0 (`iphands/basalt-blackwell:cu1343-sm120-ad274aa`,
Plan 10) and the model-free gate passes on that image; **its boots are not recorded yet** (Plan 10
T2), and neither were the v0.4.1 image's (`cu1343-sm120-d3b7574`, Plan 08), which is kept as the
rollback: `IMAGE_OVERRIDE=iphands/basalt-blackwell:cu1343-sm120-d3b7574 ./run`. The last measured
boots are the v0.2.1-era image's (`cu1343-sm120-dc047a7`, Plan 01, 2026-11-04, local pack):
`./run` loaded in ~40 s, `./run --single` in ~34 s, a 63.29 GiB expert cache in VRAM; clients reach it
as `cosmo-6000` on `cosmo.lan:8700`. The defaults are the owner's choices, not measured winners —
tuning is a later plan.

## Prerequisites

- Docker 29+ with BuildKit and the NVIDIA runtime, used as yourself (no `sudo`).
- The pack (131.7 GB). `./run` defaults to the NFS copy,
  `/mnt/noir/scratch/ai/llm/models/vllm/jesdga/Qwen3.8-FN-NVFP4-DQ6_K.basalt` (owner's choice,
  2026-11-06; `models` here links to `/mnt/noir/scratch/ai/llm/models`). Basalt loads weights with
  `O_DIRECT`, which over that copy ran at ~5–10 MB/s on 2026-11-04, before it was defragmented.
  `PACK=<path> ./run` or a first argument serves any other copy, for example a local one in the
  gitignored `models.local/`. Prefill reads the PLE n-gram table at random, 16 rows a
  token, so `PLE_IO=direct` from NFS (~2.9k IOPS, 4 KiB random reads, 2026-11-06) stalls a cold
  121k-token prompt past the engine's 60 s watchdog; the default `PLE_IO=ram` reads it once at boot.
- Nothing else on `:8700`: stop `vllm` (`../vllmcustom`), `../ninfer` or `../ignis` first.

## Pins

`versions.env`: Basalt `ad274aa8` (the v0.5.0 tag's commit, pinned 2026-11-20; before that
`d3b75742`, v0.4.1, and `dc047a77`, the v0.2.1 engine), CUDA 13.4.3 devel and runtime images by digest, image repository
`iphands/basalt-blackwell`. Moving the pin is a plan: `context/plans/RULES.md`.

## Build

```bash
PREFLIGHT_ONLY=1 ./build   # checks only: vendor/basalt at the pin, base digests, Dockerfile lint
./build                    # the image iphands/basalt-blackwell:cu1343-sm120-ad274aa (log in captures/)
```

`./build` manages `vendor/basalt` (cloned on first use, checked out at `BASALT_SHA`) and refuses
a dirty checkout. The image keeps Basalt's source-tree layout at `/opt/basalt` (`bin/`, `serve/`,
`tools/`, `data/`, and a `.venv` with upstream's pinned server dependencies) because the binary takes
the directory above its own `bin/` as its install root and runs the Python server from there.

## Publish

```bash
DRY_RUN=1 ./publish        # print the tags, touch nothing (also accepts PUSH_DRY_RUN=1)
./publish                  # owner-only: publish what versions.env pins
TAG=cu1343-sm120 ./publish # take the image from another local tag instead of the pin's own name
```

Three tags go up: the immutable `cu1343-sm120-ad274aa` that `./build` made, the moving
`cu1343-sm120`, and `latest`. `./build` mints no `:latest`, so `./publish` owns it, and it only ever
takes it for the newest local build of the pinned commit. Every name is read from the image's own
`ai.basalt.*` labels, never from the tag it was handed. Needs a terminal on stdin and a
`docker login`.

## Run

```bash
./run                      # 3 slots, each with the full 362144-token context
./run --single             # 1 slot, 362144 tokens
DRY_RUN=1 ./run            # every check, print the docker command, start nothing
./run --help               # the knob table
```

Clients use `http://cosmo.lan:8700/v1` (OpenAI) or `http://cosmo.lan:8700` (Anthropic) and must
name the model `cosmo-6000`; any other name gets a 404. Stop the server with Ctrl+C or
`docker stop basalt`.

| Knob (env) | Default | Basalt setting |
|---|---|---|
| `PACK` / first argument | `/mnt/noir/scratch/ai/llm/models/vllm/jesdga/Qwen3.8-FN-NVFP4-DQ6_K.basalt` | `serve <pack>`; its directory is mounted read-only at `/models` |
| `PARALLEL` | `3`; `1` with `--single` | `--parallel` (1..8) |
| `SLOT_FULL_CONTEXT` | `1` (only when `PARALLEL` > 1) | `--slot-full-context`: every slot gets `MAX_CONTEXT`, at N× the KV cache |
| `MAX_CONTEXT` | `362144` | `--max-context` |
| `KV` | `int8` | `--kv` (`fp16`, `int8`, `q4_0`, `k8v4`) |
| `PLE_IO` | `ram` | `--ple-io`: `ram` locks the 28.8 GB PLE n-gram table in RAM at boot (~27 GiB, held for the server's life); `direct` reads 16 random 4 KiB rows per prompt token from the pack |
| `VISION` | `dynamic` | `--vision` (`off`, `dynamic`, `cpu`, a GPU number) |
| `PORT` | `8700` | `--port`, published on `0.0.0.0` |
| `ALIAS` | `cosmo-6000` | `--alias`: the one model name requests must use |
| `ALLOWED_HOSTS` | `cosmo.lan` and `$(hostname)` | `--allowed-host` per name: Basalt refuses unknown `Host` names (IPs and `localhost` always pass) |
| `API_KEY` | none | `--api-key`; without one Basalt warns that it is open to the network |
| `NAME` | `basalt` | the container name |
| `CACHE_DIR` | `storage/cache` | mounted at `~/.cache` (Basalt's engine log and disk-cached conversations) |
| `EXTRA_ARGS` | none | appended last to the serve flags (last wins) |
| `IMAGE_OVERRIDE` | the newest image built for `BASALT_SHA` | the image to run |

`./run` refuses to start while anything publishes `:8700` or a `vllm`, `ninfer`, `ignis` or
`basalt` container runs, and refuses a pack that is not a `.basalt` file (a `.ninfer` path is named
as NInfer's). Requests from the host arrive through the docker bridge, so Basalt treats them as
remote for images: `data:` URLs work; image URLs and paths need `--image-url-host` /
`--image-dir` in `EXTRA_ARGS`.
