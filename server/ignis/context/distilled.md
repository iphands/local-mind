# Distilled — confirmed facts for ignis

Compact, confirmed learnings: formats, protocols, tool/library behavior, measured limits. Read
before new work; add an entry whenever something is confirmed — **above the `## Open Questions`
section**, which always stays last. Keep it dense.

**Provenance discipline:** every entry carries a tag — `[SOURCE]` (upstream code, cite
`path:line`), `[DOC]` (documentation), `[REPO]` (this repo's own code/config), `[LIVE YYYY-MM-DD]`
(observed on a running system). **Do not upgrade a tag without doing the work.**

<!-- Entry shape:

## <Topic — the rule in one line> [TAG][TAG]

- The fact, stated so it can be checked.
- The evidence: command + verbatim output, or `path:line`.
- What it means for this project.
-->

---

All `path:line` cites below are in the Ignis source at tag `v0.8.0` (`950ee0d1`, the source of
the pinned image), read 2026-10-27.

## Ignis serves exactly one model id, any string the operator picks [SOURCE]

- `--model-id` takes any id except the *other* model family's; that one id is all `/v1/models`
  lists and all a request may name (`crates/server/src/config.rs:1514-1523`; test
  `crates/server/src/config/tests.rs:1951-1965`: `my-id` on Flash-Next is served as `my-id`).
- An unknown `model` → `404 model_not_found`, "unknown model: X (loaded: Y)", and no switch;
  a blank or missing `model` → the loaded model (`crates/server/src/api.rs:577-586,814-816`).
- There is no multi-alias / served-model-name mechanism: catalog `aliases` are a family's default
  id only (`crates/server/src/download/catalog.yaml:14-16`).
- For us: serve `cosmo-6000`; the vLLM driver's other four names 404 under Ignis.

## A non-catalog id without `--model-artifact` boots the CPU mock [SOURCE]

- `--model-id cosmo-6000` alone is not a catalog id, so the artifact source is the
  `Placeholder(UnknownModel)` — a CPU mock that answers — with only a WARN
  (`crates/server/src/download/mod.rs:60-65,120-125`).
- For us: the launchers always pass `--model-artifact` and refuse when the file is missing.

## `--profile auto` reads CUDA device 0 only; name the profile [SOURCE][DOC]

- `auto` picks by NVML total memory of device 0 (16 GB → `rtx50-16gb`, 32 GB → `rtx5090`,
  96 GB → `rtxpro6000`; a card of no class is refused); a named profile or `none` always wins
  (`docs/user/reference/cli.md:36-38`; `crates/server/src/main.rs:52-56`). The runtime opens CUDA
  device 0 unconditionally (`crates/server/src/runtime.rs:967`); there is no `--device` flag.
- For us: the container sees host GPU 0 only (`--gpus device=0`) and the launchers pass
  `--profile rtxpro6000`.

## The `rtxpro6000` profile [SOURCE]

- 27B: `max_context 1048576`, `prefill_chunk 2048`, DFlash2 `draft_tokens 7`. Flash-Next:
  `max_context 1048576`, MTP `draft_tokens 3`, `decode_lanes 8`. Both: `vision.enabled true`,
  `ngram.hot_bytes auto` (`crates/server/src/config/profile.rs:216-227`).
- A flag beats the profile; our launchers override `max_context` to 262,144 (no YaRN: `auto`
  rope applies no scaling up to 262,144 — user decision 2026-10-27).

## Artifacts are flat files with required sidecars [SOURCE][DOC]

- Flash-Next: `qwen3_8_flash_next_trellis_a25-v2.ninfer` (71,760,711,680 B) and
  `qwen3_8_flash_next_mtp_3p0-v2.ninfer` (1,037,905,920 B), each with a `.conversion.json`; the
  MTP companion is required when MTP is on (`crates/server/src/download/catalog.yaml:41-50`;
  `docs/user/configuration/flash-next-mtp.md:13-15`).
- 27B: `qwen3_8_27b_nvfp4full-v2.ninfer` (19,406,942,468 B) + `.graft.json`; abliterated: the
  `…-huihui-abliterated.ninfer` + its `.graft.json` (`catalog.yaml:18-36`).
- `ignis-server model download <id> --out <dir>` streams to `.part`, hashes as it writes, renames
  only on a matching size + SHA-256, resumes a dropped transfer with `Range`
  (`crates/server/src/download/transfer.rs:1-12`), and needs no GPU or driver
  (`docs/user/guides/models.md:72`).
- Each HF repo also ships a `<artifact>.sha256` in `sha256sum` binary format (`<hash> *<file>`)
  [LIVE 2026-10-27: fetched the 27B's].
- For us: each HF repo in its own `models/vllm/<org>/<repo>/` (owner decision 2026-10-27); a
  launcher mounts its model's directory at `/models:ro` and passes `--model-artifact /models/<file>`,
  so the MTP companion / graft sits beside the artifact as the loader expects [REPO].

## The container and what it writes [SOURCE][DOC]

- `ENTRYPOINT ["/usr/local/bin/ignis-server"]`, `USER ignis` (uid 10001),
  `IGNIS_SERVER_BIND=0.0.0.0:8000`, `EXPOSE 8000 9464`, CUDA 13.1.1 base (`Containerfile:85,92,
  108,125-130`). The server is PID 1 on purpose: no `--init` (`docs/user/troubleshooting.md:378-384`).
- The n-gram cache and KV-disk default to `model` (beside the artifact); both take a directory
  (`docs/user/reference/configuration.md:1262-1320`). On a `:ro` models mount, pass both.
- Reads use `O_DIRECT`, falling back to buffered where refused; no io_uring
  (`crates/artifact/src/direct.rs:5-9`) — docker's default seccomp is fine, unlike NInfer.

## `config print` checks flag spellings with nothing loaded [SOURCE]

- `ignis-server config print [--format json|yaml] [field flags]` parses the flags, resolves the
  layers and prints the effective document; it opens no artifact, and with a named profile reads
  no card (`crates/server/src/config/cli.rs:177-196`). An unknown flag is a parse error.
- Precedence: patch > flag > env > file, each layer trying the per-model (`--qwen38flashnext-…`,
  `--qwen38-…`) value before the general one; the profile is consulted only when no explicit layer
  set the field (`crates/server/src/config/source.rs:431-476`). So a general flag such as
  `--model-max-context` overrides the profile's per-model value.
- For us: `PRINT_CONFIG=1 ./qwen38fn/run` runs it in a `--network none` container with the
  launcher's exact flags — the owner's pre-download check [REPO].

---

## Open Questions

<!-- Things we believe but have not confirmed. Check here before trusting a related assumption;
     move an item up (with its tag) once it is confirmed, or into pitfalls.md if it bit. -->

- Does Ignis 0.8.0 run cleanly as `--user 1000:1000` with `HOME=/cache` and every writable path
  explicit, or does it write under `/home/ignis` anyway? (Plan 01 T4)
- Does it honour `stream_options.include_usage` (the vllmcustom bench client reads the usage
  chunk)? (Plan 01 T4)
