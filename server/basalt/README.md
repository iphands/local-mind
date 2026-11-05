# basalt — Basalt in a container, on :8700

[Basalt](https://github.com/jesdga95/basalt) (a local inference engine for **Qwen3.8-Flash-Next**)
built from a pinned commit in our own image for the RTX PRO 6000 Blackwell (sm_120), and one
launcher, `./run`, that serves a `.basalt` pack on `:8700` with an OpenAI- and Anthropic-compatible
API.

Basalt loads only Flash-Next `.basalt` packs. `.ninfer` artifacts (for example the QUASAR 27B)
belong in `../ninfer`.

**Status (2026-11-04):** the image `iphands/basalt-blackwell:cu1343-sm120-dc047a7` is built and
checked without a GPU (Plan 01 T4); the pack has not been booted yet. The defaults below are the
owner's choices, not measured winners.

## Prerequisites

- Docker 29+ with BuildKit and the NVIDIA runtime, used as yourself (no `sudo`).
- The pack, downloaded by the owner:
  `models/vllm/jesdga/Qwen3.8-FN-NVFP4-DQ6_K.basalt` (131.7 GB; `models` →
  `/mnt/noir/scratch/ai/llm/models`).
- Nothing else on `:8700`: stop `vllm` (`../vllmcustom`), `../ninfer` or `../ignis` first.

## Pins

`versions.env`: Basalt `dc047a77` (main on 2026-11-04, the v0.2.1 engine), CUDA 13.4.3 devel and
runtime images by digest, image repository `iphands/basalt-blackwell`.

## Build

```bash
PREFLIGHT_ONLY=1 ./build   # checks only: vendor/basalt at the pin, base digests, Dockerfile lint
./build                    # the image iphands/basalt-blackwell:cu1343-sm120-dc047a7 (log in captures/)
```

`./build` manages `vendor/basalt` (cloned on first use, checked out at `BASALT_SHA`) and refuses
a dirty checkout. The image keeps Basalt's source-tree layout at `/opt/basalt` (`bin/`, `serve/`,
`data/`, and a `.venv` with upstream's pinned server dependencies) because `basalt serve` runs the
Python server from the source directory compiled into the binary.

## Run

```bash
./run                      # 4 slots, each with the full 262144-token context
./run --single             # 1 slot, 262144 tokens
DRY_RUN=1 ./run            # every check, print the docker command, start nothing
./run --help               # the knob table
```

Clients use `http://cosmo.lan:8700/v1` (OpenAI) or `http://cosmo.lan:8700` (Anthropic) and must
name the model `cosmo-6000`; any other name gets a 404. Stop the server with Ctrl+C or
`docker stop basalt`.

| Knob (env) | Default | Basalt setting |
|---|---|---|
| `PACK` / first argument | `models/vllm/jesdga/Qwen3.8-FN-NVFP4-DQ6_K.basalt` | `serve <pack>`; its directory is mounted read-only at `/models` |
| `PARALLEL` | `4`; `1` with `--single` | `--parallel` (1..8) |
| `SLOT_FULL_CONTEXT` | `1` (only when `PARALLEL` > 1) | `--slot-full-context`: every slot gets `MAX_CONTEXT`, at N× the KV cache |
| `MAX_CONTEXT` | `262144` | `--max-context` |
| `KV` | `int8` | `--kv` (`fp16`, `int8`, `q4_0`, `k8v4`) |
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
