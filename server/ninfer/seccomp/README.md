# seccomp/ — NInfer's syscall profile

`ninfer.json` is **generated** by `scripts/seccomp-profile`; do not edit it by hand.

It is docker's default seccomp profile plus exactly three syscalls, added to the profile's one
unconditional `SCMP_ACT_ALLOW` entry:

- `io_uring_setup`
- `io_uring_enter`
- `io_uring_register`

Why: NInfer reads model files `O_DIRECT` through io_uring (`src/core/io/read_queue.cpp:83`), with
no fallback, and docker's default profile answers `io_uring_setup` with `EPERM`. The alternative,
`seccomp=unconfined`, drops all filtering on a server bound to `0.0.0.0`.

| | |
|---|---|
| Source | <https://raw.githubusercontent.com/moby/moby/docker-v29.8.0/vendor/github.com/moby/profiles/seccomp/default.json> |
| Ref | `docker-v29.8.0` (`MOBY_SECCOMP_REF` in `versions.env`) |
| Source sha256 | `536529b665dd0972c37bfb569f5d4ac8a53592e7b00752bc39ff063ca9864c74` |
| `ninfer.json` sha256 | `0a369b29253de5df97d133e6e037a0365a7c2ea9e9ffe970e50045c83319ba53` |

**Regenerate after a docker upgrade**: the profile is frozen at this ref while dockerd's built-in
default moves with it. Set `MOBY_SECCOMP_REF` to the tag whose commit equals
`docker version --format '{{.Server.GitCommit}}'` and run `./scripts/seccomp-profile`. Project gate
step 3 (`context/plans/RULES.md`) proves the result: io_uring allowed, and a syscall the default
blocks (`keyctl`) still blocked.
