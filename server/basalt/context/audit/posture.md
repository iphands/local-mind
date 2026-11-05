Target dc047a77beeddf1e91234d983a91d6bf929e9619 / tree 3f7b69ae4054969ed15b9b6f02d306eb44e0ebe4 — if that is not the tree you are looking at, you are in the wrong checkout.

# Our own posture — what `./run` exposes, and what to change

Plan 02 T13, 2026-11-05. The question here is not what Basalt's code does, but what **our launcher** lets
leave the machine or be read. Every fact below is in `wave0/t13-posture.txt`, emitted by command (`[LIVE
2026-11-05]` unless a `path:line` is given). `/status` and `/api/logs` were deliberately **not** requested
from the live server: they would have copied the owner's conversations into the evidence.

## The short version

**Anyone who can reach port 8700 on this host can use the model and read your recent answers.** `./run` binds
`0.0.0.0` with no API key, and every endpoint except `/health`, the web page and its fonts is gated by the key
alone. One flag fixes most of it: **set an API key.**

## What is true today

| # | Fact | Evidence |
|---|---|---|
| 1 | The server listens on every interface: `-p 0.0.0.0:8700:8700` and `--host 0.0.0.0` | T13 Verify; `run` (the `-p` and `--host` lines) |
| 2 | No API key by default. The live server, asked at its LAN address `192.168.10.99`, reports `"api_key": false` | `/health` |
| 3 | **Without a key, every request is authorized.** `_authorized()` returns true when no key is set (`serve/handler.py:211`). Behind it: `/v1/*` chat, `/status` ("it shows the last answer's tail", `:271`), `/api/logs`, `/metrics`, `/slots`, `/props`, and POST `/load` / `/unload` (`:322`, `:330`) | code, at the pin |
| 4 | Exposure without a key is only a **warning** (`serve/server.py:294`) | code |
| 5 | **`--allowed-host` widens, it does not restrict.** It adds names to a DNS-rebinding check (`serve/security.py:131`), and that check **passes any IP address** (`:142`). A LAN client connecting by IP is never stopped by it; with a key set, the check is off entirely (`handler.py:63`) | code |
| 6 | Basalt accepts the key **only on the command line** (`serve/config.py:104`, no env var read), and the host can see the container's process arguments in `ps` | `ps` |
| 7 | The container is on the default `bridge` network: **it has egress** | `docker inspect basalt` |
| 8 | No hardening: `capdrop=[] secopt=[] ro=false pids=<nil>`. It runs as uid 1000 with no effective capabilities, but Docker's default bounding set remains and the image has **8 setuid binaries** (`su`, `mount`, `passwd`, …) | `docker inspect`; image probe |
| 9 | The image user is in group `sudo`, but **`sudo` is not installed**: inert | image probe |
| 10 | Persisting between boots in `storage/cache` (889 MB): the vision cache (876 MB), tokenizer cache (9.4 MB) and **`engine.log` (3.0 MB), mode `644` under `755` directories — readable by any local user**. What the log contains is T6a's question | `stat`, `du` |
| 11 | The container's writable layer is discarded on stop (`run --rm`) | `run` |

Not measured: whether the host firewall blocks *other* LAN devices. Fact 2 was taken from this host. Docker's
published ports are normally inserted ahead of a host's INPUT rules, so assume they reach the LAN until a
second device shows otherwise.

## Action list, in order

Each is an **owner decision**. Items 1–3 and 5 change a `./run` default, which RULES' plan-gate table says
needs a plan of its own; item 4 is a one-off command.

1. **Set an API key** — `API_KEY=<key> ./run` works today (printed as `REDACTED`). This closes chat,
   `/status`, `/api/logs`, `/metrics`, `/slots`, `/props`, `/load` and `/unload` to everyone without the key.
   *Tradeoff:* every client must send it (`Authorization: Bearer …` or `x-api-key`). Because Basalt takes it
   only on the command line, it is visible in `ps` to local users and in `docker inspect` to the docker group.
   On this single-user host that is acceptable. *Make it the default:* `./run` reads the key from a mode-600
   file under `storage/` and refuses to bind `0.0.0.0` without one.
2. **Bind only where clients are.** If nothing off this host uses the server, publish
   `-p 127.0.0.1:8700:8700` instead of `0.0.0.0`. *Tradeoff:* `cosmo.lan` clients stop working. Since Plan 01
   serves `cosmo-6000` on `cosmo.lan:8700` on purpose, the realistic form is **keep the LAN bind and do item 1**.
   Do not rely on `--allowed-host` for this (fact 5).
3. **Cut egress, after Plan 03.** Put the server on an `--internal` network and publish the port through a
   small forwarding sidecar on both networks. *Tradeoff:* image-by-URL requests stop working (uploaded or
   `data:` images still do), and there is one more container. Plan 03 shows whether anything wants egress
   at all, and how `-p` behaves on an internal network. Decide after.
4. **Lock down the cache now:** `chmod 700 storage/cache`. *Tradeoff:* none. The container runs as uid 1000,
   the same owner.
5. **Cheap container hardening:** add `--security-opt no-new-privileges --cap-drop ALL --pids-limit 4096`.
   `no-new-privileges` is the one that matters (fact 8: setuid binaries plus a default bounding set). The pids
   cap matches the opencode containers (`1c94bd8`). *Tradeoff:* none expected for a uid-1000 process that
   needs no capabilities, but it needs one boot to confirm. `--memlock` is a ulimit, not a capability.

**Considered and not recommended:** `--read-only` root filesystem. The writable layer is already discarded on
stop (fact 11), and it would need a boot test to find every path Basalt writes, for little gain. The `sudo`
group is inert (fact 9). `EXTRA_ARGS`, `--engine-bin`, `--script` and `--config` are operator-only
environment, so no change; T8 confirms that no request reaches them.

**Already tracked elsewhere:** hash-pinned Python dependencies (tracker Follow-ups). At a pin bump, compare
against basalt's own `v0.2.1` (`1b3f6964`), never Strata's (plan § T13 ⚠). No bump is pending.
