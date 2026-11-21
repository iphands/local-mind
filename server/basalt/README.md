# basalt — Basalt in a container, on :8700

[Basalt](https://github.com/jesdga95/basalt) (a local inference engine, first for
**Qwen3.8-Flash-Next**) built from a pinned commit in our own image for the RTX PRO 6000 Blackwell
(sm_120), and two launchers that serve a `.basalt` pack on `:8700` with an OpenAI- and
Anthropic-compatible API: `./run` for Flash-Next, `./run-hy4` for Tencent's **Hy4-flash**.

Engine flags are per model, so each launcher takes only its own model's packs and names the other
when handed the wrong one. `.ninfer` artifacts (for example the QUASAR 27B) belong in `../ninfer`.

**Status (2026-11-21):** built on Basalt v0.5.5 (`iphands/basalt-blackwell:cu1343-sm120-ff0b978`,
Plan 13) with our buffered-io patch (§ *Build*), and the model-free gate passes on that image; its
boots are Plan 13 T2. The v0.5.0 image (`cu1343-sm120-ad274aa`, Plan 10, rebuilt with the patch) is
kept as the rollback: `IMAGE_OVERRIDE=iphands/basalt-blackwell:cu1343-sm120-ad274aa ./run`. The
image holds five models and this project serves two; Inkling 2 Mini, new in v0.5.5, has no launcher
here. The last measured
boots are the v0.2.1-era image's (`cu1343-sm120-dc047a7`, Plan 01, 2026-11-04, local pack):
`./run` loaded in ~40 s, `./run --single` in ~34 s, a 63.29 GiB expert cache in VRAM; clients reach it
as `cosmo-6000` on `cosmo.lan:8700`. The defaults are the owner's choices, not measured winners —
tuning is a later plan. **Hy4-flash** (`./run-hy4`, Plan 11) booted from the NFS pack on 2026-11-20:
86.2 GiB loaded in 105.8 s, every expert in VRAM (77.84 GiB), a 456-token prompt answered — and a
14,150-token prompt stalled on Engram reads from NFS until the engine's watchdog stopped it. It needs
a local copy of the pack (below); no boot from one is recorded yet.

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
- For `./run-hy4`, the Hy4-flash pack (158.7 GB). It defaults to the NFS copy,
  `/mnt/noir/scratch/ai/llm/models/vllm/jesdga/Hy4-flash-NVFP4.basalt`. Hy4-flash reads its 63 GB
  Engram n-gram table from the pack for every prompt token (64 rows, the reader behind Flash-Next's
  `PLE_IO=direct`) and has no RAM mode, so the NFS caveat above applies with nothing to switch:
  `./run-hy4` prints a note when the pack is on NFS, and a local copy is
  `PACK=models.local/Hy4-flash-NVFP4.basalt ./run-hy4`. Measured 2026-11-20 from the NFS copy:
  prompts read at 53 tok/s (a 456-token prompt took 8.6 s, 8.3 s of it Engram reads), and a
  14,150-token prompt made no progress for 60 s, so the engine's watchdog stopped it. The link is
  not the limit (the same reads run at 164k IOPS once noir has the pages in RAM, about 1k when it
  does not): **serve Hy4-flash from local NVMe**, or run `./preload` first (below), which reads the
  pack's `engram` section once so noir caches it.
- Nothing else on `:8700`: stop `vllm` (`../vllmcustom`), `../ninfer` or `../ignis` first.

## Pins

`versions.env`: Basalt `ff0b978d` (the v0.5.5 tag's commit, pinned 2026-11-21; before that
`ad274aa8`, v0.5.0, `d3b75742`, v0.4.1, and `dc047a77`, the v0.2.1 engine), CUDA 13.4.3 devel and runtime images by digest, image repository
`iphands/basalt-blackwell`. Moving the pin is a plan: `context/plans/RULES.md`.

## Build

```bash
PREFLIGHT_ONLY=1 ./build   # checks only: vendor/basalt at the pin, base digests, Dockerfile lint
./build                    # the image iphands/basalt-blackwell:cu1343-sm120-ff0b978 (log in captures/)
```

`./build` manages `vendor/basalt` (cloned on first use, checked out at `BASALT_SHA`) and refuses
a dirty checkout. It also applies our local patches, `patches/*.patch`, inside the image build
(never in `vendor/basalt`): each must fit the pin exactly or the build stops, and the image's
`ai.basalt.patches` label names each with its hash. One so far (Plan 12):
`0001-buffered-io-switch.patch` — with `BASALT_BUFFERED_IO=1` in its environment the engine reads
files through the OS cache instead of with `O_DIRECT`; without it, upstream's behaviour. The image
built for the pin carries it (label `ai.basalt.patches=0001-buffered-io-switch.patch:a3553ec6bb8c`).
A repin must show the patch still fits: `PREFLIGHT_ONLY=1 ./build` prints `== patch fits: …` or
stops. The image keeps Basalt's source-tree layout at `/opt/basalt` (`bin/`, `serve/`,
`tools/`, `data/`, and a `.venv` with upstream's pinned server dependencies) because the binary takes
the directory above its own `bin/` as its install root and runs the Python server from there.

## Publish

```bash
DRY_RUN=1 ./publish        # print the tags, touch nothing (also accepts PUSH_DRY_RUN=1)
./publish                  # owner-only: publish what versions.env pins
TAG=cu1343-sm120 ./publish # take the image from another local tag instead of the pin's own name
```

Three tags go up: the immutable `cu1343-sm120-ff0b978` that `./build` made, the moving
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
| `BUFFERED_IO` | `1` | `-e BASALT_BUFFERED_IO=1`: the engine reads files through the page cache (our patch, § *Build*); `0` is upstream's `O_DIRECT`, which no cache on this machine can serve. An image built before the patch ignores it, and the launcher says so |
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
as NInfer's) or that is a Hy4-flash pack (it names `./run-hy4`). Requests from the host arrive through the docker bridge, so Basalt treats them as
remote for images: `data:` URLs work; image URLs and paths need `--image-url-host` /
`--image-dir` in `EXTRA_ARGS`.

### Hy4-flash: `./run-hy4`

```bash
./run-hy4                  # 1 slot, 362144 tokens
DRY_RUN=1 ./run-hy4        # every check, print the docker command, start nothing
./run-hy4 --help           # the knob table
```

Same container, port, alias, cache directory and refusals as `./run`; the engine flags differ,
because Hy4-flash takes none of Flash-Next's. `PORT`, `ALIAS`, `ALLOWED_HOSTS`, `API_KEY`, `NAME`,
`CACHE_DIR`, `EXTRA_ARGS` and `IMAGE_OVERRIDE` work as above. The rest:

| Knob (env) | Default | Basalt setting |
|---|---|---|
| `PACK` / first argument | `/mnt/noir/scratch/ai/llm/models/vllm/jesdga/Hy4-flash-NVFP4.basalt` | `serve <pack>`; must be a Hy4-flash pack (`general.architecture` `hy4flash`) — a Flash-Next pack is refused with a pointer to `./run` |
| `MAX_CONTEXT` | `362144` | `--max-context`, 1024 to 1048576 (the model's maximum) |
| `KV` | `fp16` | `--kv` (`fp16`, `fp8`); `int8` is refused by the model |
| `PARALLEL` | `1` | `--parallel` (1..8); upstream lists no parallel slots for Hy4-flash, so above 1 is unproven |
| `SLOT_FULL_CONTEXT` | `1` (only when `PARALLEL` > 1) | `--slot-full-context` |
| `VISION` | `dynamic` | `--vision` (`off`, `dynamic` — the only image mode; no video) |
| `BUFFERED_IO` | `1` | as for `./run`; `./preload` takes the same knob and must be given the same value |

```bash
./preload                  # read the pack's engram section (59 GiB) from NFS so the server caches it, then check
./preload --check          # only sample it: still in the server's RAM? (exit 1 when cold)
./preload --list           # the pack's sections: name, offset, bytes
DRY_RUN=1 ./preload        # print the dd command, read nothing
```

`./preload` is for a pack served from NFS. It takes the section's offset and size from the pack's own
table and reads it the way the engine will: through the page cache with `BUFFERED_IO=1` (the default;
this machine and noir both keep it), with `O_DIRECT` under `BUFFERED_IO=0` (only noir keeps it). The
effect lasts until the pages are evicted or noir reboots; `--check` times 200 random 4 KiB reads
(about 0.5 ms each when cached, 14 ms when noir has to seek). A local pack needs none of this.

There is no `PLE_IO`: `--ple-io` is Flash-Next's flag and Hy4-flash refuses it. These defaults are
starting values (`./run`'s context, upstream's KV default), not measured and not yet the owner's
choices.
