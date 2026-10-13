# Plan 02 — No sudo anywhere — Tracker

## Overview

- Status: 67% complete (4/6 tasks)
- Start date: 2026-10-13
- Plan: `context/plans/02_no_sudo.md`
- Evidence: the gate's own stdout (`/tmp/gate_all.sh` captured per run), the `grep -ci sudo` counts
  per touched file, and `docker version --format 'server={{.Server.Version}}'` on the host as fixed
  2026-10-13. No bench artifacts: this plan touches no runtime path, so no image tag / `VLLM_REF` /
  model participates in any assertion here.

## Resume Instructions

1. Read the project's root `AGENTS.md`, then `context/plans/RULES.md` **in full**, then
   `02_no_sudo.md`.
2. Environment: no sudo on this host and none wanted — `DOCKER` defaults to plain `docker`. The
   socket is `iphands:iphands 660` (user-owned since 2026-10-13), so plain `docker` works with no
   group membership. `PREFLIGHT_ONLY=1` is the cheap end-to-end check; note `CTX_DIR=` is needed for
   a full build because `rsync` is absent (Plan 01 tracker Deviations). **The agent shell is a
   container that *does* have `/usr/sbin/sudo` — do not infer host facts from it.**
3. Ordering: T1 → T2 → T3 are one contract applied to three copies; do them in that order because
   `_run-lib.sh` is the copy the launchers load and the one that gains two guards the others already
   have. T4 (the gate) must land with T1-T3, never after — a green gate that still demands `sudo -n`
   would be a gate that cannot run. T5 last: docs describe code, not the other way round.
4. Commit per task as `[P2][T<n>][no-sudo] <summary>`, with **this tracker's row in the same commit**.
5. Restore system state at session end (Rule P2.4): no sysctls or power limits move in this plan, but
   `DOCKER` / `ALLOW_ROOT` / `DOCKER_BUILDKIT` must not be left exported in anything you source.

## Open Unknowns

- ~~Is ambient `DOCKER_BUILDKIT=0` actually reachable once the wrapper is plain?~~ RESOLVED (T2): see
  the T2 verify line — `env`-pinning `DOCKER_BUILDKIT=1` is applied regardless, so the plan does not
  depend on the answer; the comment rewrite does, so confirm before writing it.
- Unconfirmed: whether any `*/run` other than `qwen/run` needs its own 7a-style assertion. Current
  gate covers `qwen/run` only; the guard lives in the shared lib, so one assertion covers all four.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: launcher lib stops knowing about privilege | `common/_run-lib.sh` | done | `bash -n` clean; guard matrix refuse/refuse/pass/pass; no `:-sudo`, `sudo -n`, `sudo chown` or `ALLOW_ROOT` left. Root refusal kept, reworded |
| 2 | T2: build contract, remedies, BuildKit pin | `container/build` | done | 9 hunks. `PREFLIGHT_ONLY=1 CTX_DIR=` → rc 0 unprivileged; `DOCKER='sudo -n docker'` → rc 1 before any write; uid-0 refusal proven by `docker run -u 0` on a `:ro` mount; `env`-pinned `DOCKER_BUILDKIT=1` |
| 3 | T3: archs follows | `container/archs` | done | Guard matrix refuse/refuse/pass; `./container/archs nonexistent:nope` → rc 125 (docker's own "no such image"), so it reaches `docker run` unprivileged. Wrapper warning replaced, not deleted |
| 4 | T4: gate drops the sudo requirement | `context/plans/RULES.md` | done | Full gate rc 0: 9 PASS / 3 named SKIP / 0 FAIL. `PASS(7a rejects a sudo in DOCKER)` + `PASS(7a left nothing root-owned)`. 7e still root-free (244,890 entries, 10 s) |
| 5 | T5: docs that assert the opposite | 8 files | done | Instruction-hunt clean: 8 surviving hits, each a gate test input, dated history, or a retraction. Reviewer-found defects all fixed: 4 banned host-fact claims, `README:525` "one script" contradiction, `AGENTS.md` socket mis-pointer, template constraint duplication |
| 6 | T6: full gate, harvest, close | `context/` | pending | Step 6 named-skipped: no runtime change |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid`.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-13 | `docker version --format 'server={{.Server.Version}}'` | `server=29.7.2`, rc 0 | 1 | Plain docker, no privilege. Precondition for the whole plan |
| 2026-10-13 | `stat -c '%U:%G %a' /var/run/docker.sock` | `iphands:iphands 660` | 1 | User-owned socket; kills the "join the docker group" advice |
| 2026-10-13 | `id -nG` | `iphands 27` | 1 | No docker group membership; gid 27 has no name in this env |
| 2026-10-13 | `VLLM_REF=main ./container/build` (pre-plan) | `timeout: failed to run command 'sudo': No such file or directory`, 127 | 1 | The trigger. Probe refused before any write |
| 2026-10-13 | `grep -rn 'su -c' README.md container/` | 8 hits incl. `build:400`, `:404` | 1 | Root reachable without sudo, by the repo's own instruction |
| 2026-10-13 | `grep -n EUID container/slice-setup container/gc-setup` | `slice-setup:49`, `gc-setup:86` both require 0 | 1 | Why the root refusal stays |
| 2026-10-13 | full gate: `awk NR 101-459` → `/tmp/gate_all.sh` → `bash` | **rc 0**, 9 PASS / 3 named SKIP / **0 FAIL** | 1 | `PASS(7a rejects a sudo in DOCKER)`, `PASS(7a left nothing root-owned)`, `PASS(7c) 1000 unlimited readable`, 7e 244,890 entries root-free in 10 s. Skips: shellcheck (permanent), 7b + 7b env (Plan 01 T3b rebuild) |
| 2026-10-13 | step 5 `./container/archs` as run inside that gate | identity line reads `… via docker`; `_vllm_fa2_C 384M sm_120 x76`, no `sm_80` SASS | 1 | Step 5 prints no verdict token, so it was checked for its table — a silently un-executed step is the failure RULES.md names |
| 2026-10-13 | `docker run --rm -u 0:0 -v $PWD:/repo:ro ./container/build` | `## REFUSED: running as root`, in-container rc 1, host repo unchanged | 1 | Proves the kept root guard fires before any write |
| 2026-10-13 | `DOCKER_BUILDKIT=0` vs pinned `=1`, empty context, missing Dockerfile | legacy builder (`DEPRECATED: The legacy builder…`) vs BuildKit (`failed to solve`) | 1 | Confirms ambient `=0` reaches the CLI now the privilege helper is gone; justifies the `env` pin |
| 2026-10-13 | `grep -rn 'none is installed\|this box has no'` over my own T5 text | 4 hits: `AGENTS.md:15`, `SERIES.md:67`, `README:375`, `distilled.md:44` heading | 5 | **Self-inflicted, found by neckbeard.** All four assert a host fact the 01 tracker's dated `sudo -n -l` row refutes, and that `02_no_sudo.md:146-148` explicitly bans. Reworded to "none is expected" / project-scoped |
| 2026-10-13 | `grep -n 'systemctl restart docker' container/gc-setup` | `gc-setup:150` | 5 | hoodie's hazard is real: `AGENTS.md` pointed socket errors at § Caching, whose first imperative is `su -c gc-setup` — a script that restarts docker and kills live containers. `AGENTS.md` now answers the socket error inline and names `gc-setup` as **not** the fix |
| 2026-10-13 | `grep -c 'docker\.sock' AGENTS.md README.md` | `0` and `0` before the fix | 5 | The auto-loaded file and the README never contained the string an agent would grep for, and `AGENTS.md`'s only `EACCES` was the cache-root failure — a false lead for a socket problem |
| 2026-10-13 | `grep -n 'build:313'` vs `grep -n 'checkout --force' container/build` | cited `:313`; actual `git checkout --force` at `:354` (`sync_src`) | 5 | My citation slipped into the plan and this tracker; both corrected. Line numbers were shifted by T2's own added lines |
| 2026-10-13 | instruction-hunt after all T5 fixes | 8 hits: `RULES.md:291` (gate input), `README:576` (retraction), `distilled:53,61` + `pitfalls:46,191,241,260` (dated history) | 5 | T5 acceptance. Every survivor is self-labelled in place; the shipped `→ empty` assertion was unsatisfiable and is replaced |

## Notes / Deviations

- **Instruction vs. finding.** The literal instruction was "delete all root/sudo machinery". The
  root half was NOT safe to delete: `container/build:354` force-checks-out four hand-editable trees,
  RULES.md:493 calls that destroying the owner's work, and `_writable` (`:206`) tests `-w` — always
  true for root — so it reports success on the exact tree it is about to ruin. Owner decided
  (2026-10-13): keep a root refusal, purge all sudo wording. That is what T1-T3 implement.
- Two reviewers independently flagged the remedy strings before any code changed: printing
  `sudo chown` was already unrunnable on this host, and printing `rm -rf` for root-owned staging is
  worse than silence (`rm -rf` into a root-owned 0755 leaf fails partway; a rename needs write on the
  parent only). Both folded into T2's per-class remedies.
- `common/_run-lib.sh` gains a zero-word guard and a 127 branch that `container/build:182` /
  `container/archs:55` already had. `archs:24` claims the three copies are "byte-identical on
  purpose" — that claim was already false before this plan, and T1-T3 make it true again.
- **Two of this plan's own Verify lines were wrong, caught at T1 by running them.** (a) `grep -ci sudo
  → 0` is unsatisfiable and wrong in principle: a guard that *rejects* `sudo` must contain the word, so
  the honest assertion is "no `:-sudo` default, no `sudo -n`, no `sudo` remedy, no instruction to run
  it" — the plan now says that, in T1 and T5. (b) `… | head -3` in a verify line violates this repo's own
  recorded SIGPIPE/exit-141 constraint; replaced with a redirect-to-file.
- **The first cut of the refusal guard did not parse.** `[[ ' '"${DOCKER[*]}"' == *' sudo '* … ]]`
  swallows `== *` into a quoted operand, so bash sees a bare word: `unexpected token 'sudo',
  conditional binary operator expected`. The padding has to be quoted *outside* the operand
  (`' '"${DOCKER[*]}"' ' == *' sudo '*`). `bash -n` found it; the guard had looked correct. Verified
  as a 4-case matrix, not one case — accept-and-reject both have to be exercised or the guard could
  pass by refusing everything.
- **Process slip at T2, no damage.** `PREFLIGHT_ONLY=1 ./container/build` was run to verify the new
  preamble without first running gate step 4's porcelain guard — and that path force-checks-out the
  four hand-editable trees, which `AGENTS.md` says to guard first. Checked after: the only dirty entry
  anywhere is `?? 3rdparty/nccl/` in flashinfer, untracked since 2026-06-19, and `checkout --force`
  does not remove untracked directories. Nothing of the owner's was lost. Run step 4 BEFORE any
  build invocation, including preflight-only ones.
- **The `DOCKER_BUILDKIT=0` claim was reasoned in the plan and is now observed.** An empty context
  with a missing Dockerfile fails either way, but the builder identity is legible in the error:
  `DOCKER_BUILDKIT=0` answers `DEPRECATED: The legacy builder is deprecated` (classic builder), pinned
  `DOCKER_BUILDKIT=1` answers `failed to solve: failed to read dockerfile` (BuildKit). So the Key Fact
  in the plan titled "confirm in T2 before relying on it" is confirmed: with the privilege helper gone,
  an ambient `DOCKER_BUILDKIT=0` really does reach the CLI. The pin is applied at the build line.
- **The kept root guard was proven, not assumed.** `docker run --rm -u 0:0` with the repo mounted `:ro`
  prints the REFUSED block and exits 1, and the host repo stayed clean afterwards (0 root-owned files,
  no new porcelain entries). This is the evidence for overriding the literal "delete all root
  machinery" instruction — the guard is the only thing between `su -c ./container/build` and the four
  trees, and it demonstrably works.
- `ALLOW_ROOT` **announces itself** rather than being silently ignored (reviewer suggestion, adopted):
  `[[ ${ALLOW_ROOT+set} == set ]]` prints a warning and continues. The plan text only said "warn"; the
  wording landed stronger, and the reasoning is in the guard comment — a flag that silently does nothing
  is worse than a refusal, because whoever set it believes it is protecting them.
- **The recorded gate-extraction range is now stale, by this plan's own doing.** It read
  `awk 'NR>=101 && NR<=438'`; Rule A now ends at 459, so that range silently drops the tail of the
  gate — the exact "un-executed step that looks green" failure RULES.md names. Recompute the range
  from the `### Rule A` / `### Rule B` markers after **any** RULES.md edit rather than reusing a
  remembered pair of numbers. This run used 101-459 and extracted 199 lines, step 1 through 7e.
- Two comments in the 7a fence ended **mid-sentence** (`… so the`, `… a sudo that`). Checked against
  `git show HEAD:…`: identical there, `len=98`, so long-standing in the repo rather than damage from
  this session. Both were on lines T4 had to rewrite anyway, so they are now complete sentences.
- The T2 process slip is structurally covered: the gate's own step-4 porcelain guard (extracted line
  47) runs before any preflight/build invocation, so running the gate whole cannot repeat it.
- Step 6 of the gate (boot a model) is named-skipped by design: no runtime behaviour changes. The
  tracker says so rather than pretending the box went green.
- **T5 shipped a draft that broke this plan's own rule, and only review caught it.** Four lines I wrote
  asserted "no `sudo` is installed" / "this box has no `sudo`" — the exact phrasing `02_no_sudo.md:146-148`
  bans, and which the 01 tracker's dated `sudo -n -l` → `(ALL) NOPASSWD: ALL` row refutes. The failure mode
  is instructive: I wrote the ban and then wrote the banned sentence in the highest-leverage file in the
  repo (`AGENTS.md`, auto-loaded every session), because the grep I trusted hunted *command* shapes
  (`sudo -n`, `sudo chown`) and a prose assertion matches none of them. Lesson kept where it bites: the
  assertion in T5's Verify now hunts claims as well as commands, and the two remaining prose risks
  (`distilled.md` heading, `README` host-tuning line) are named there. A doc sweep needs a grep for what
  the docs *assert*, not just for what they *tell you to run*.
- Two files joined T5 that the plan did not list, both found by running the Verify rather than by reading
  the plan: `context/plans/NN_example_tracker.md` (the skeleton every future plan is copied from still
  said "docker calls need `sudo -n`", which would have re-injected the deleted contract into every plan
  written after this one) and `context/AGENTS.md` (claimed a README line count that T5's own edits
  invalidated — now phrased without a number so it cannot go stale again).

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Collapse the triplicated `DOCKER` preamble into `common/_docker-lib.sh` | The `@a` predicate + probe + refusal exist in 3 copies that already drifted once (the `@a` bug had to be fixed 3×, and `archs:24`'s "byte-identical" claim is stale). This plan touches all three again | Plan 03 or a `common/` tidy — not this plan, which must not change structure and semantics at once |
| Route `bench/*` + `scripts/*` through `${DOCKER[@]}` | They write no host files so the uid contract holds, but a failed boot still prints `NOT READY` with an empty `crash-*.log` because the denial goes to stderr and the redirect creates the file anyway | Plan 01 Follow-ups (pre-existing row) — carried forward |
| Add a `container/diag` that prints socket owner:group/mode, `id -nG`, `DOCKER`, `DOCKER_HOST`, `docker version` rc, `EUID`, and the uid of each writable root | One pasteable block replaces the four separate advice blocks this plan had to keep | Backlog |
| Report the true `SocketGroup` once the owner settles how the socket is provisioned | Socket is user-owned today (`660 iphands:iphands`); if it ever becomes group-owned the advice text in `_run-lib.sh` should read the group off the socket rather than assume | Backlog — do not speculatively implement |
| Rootless-daemon audit | If `DOCKER_HOST` ever points at a rootless daemon, `--user 1000:1000` maps through subuid and Plan 01's cache contract changes shape entirely | Before any rootless switch, not before |
| One-line precondition in README § Quick start: you must be able to open the docker socket | A cold reader's first command is `./container/build`, and nothing above it states the access precondition | Next README pass — hoodie (Plan 02 T5), deferred to keep T5 inside the sudo purge |
| Reorder `repin.md:184-189` | It reads as "pass `DOCKER=docker` even though that is the default", which sounds pointless until the next sentence; the real point is the *literal string* is what makes the PATH stub run | Next `repin.md` touch — hoodie (Plan 02 T5) |
| `## Symptom → section` table at the top of `pitfalls.md` | A grep for a quoted error string would land on the fix without reading 400+ lines; the socket entry already carries the exact text | Backlog — hoodie (Plan 02 T5) |
