<!-- hands-plan:v1 2026-11-04 -->
# basalt — Plan Series

Dependency chain and status of every plan. **Read this before creating a plan. Update it whenever
a plan is added, starts, completes, or is abandoned** — in the same commit as that change.

Plan status: `pending` | `in-progress` | `blocked` | `done` | `abandoned`.
Plans further out are intentionally light: an untouched pending plan is a **hypothesis, not a
contract** — re-read and revise it before starting (RULES.md Rule C).

**Next free plan number: `15`.** <!-- hp:next-free -->

> **North star (user directive, 2026-11-04):** serve Qwen3.8-Flash-Next through
> [Basalt](https://github.com/jesdga95/basalt), source-built from a pinned commit in our own
> container tuned for the RTX PRO 6000 (sm_120), launched by `./run` on `:8700` with a `.basalt`
> pack (`Qwen3.8-FN-NVFP4-DQ6_K` is upstream's default). Hints come from `../vllmcustom`
> (container shape, `:8700`, run-as-yourself). <!-- hp:north-star -->
>
> **Out of scope (owner decision, 2026-11-04):** the
> `jesdga/Qwen3.8-27B-QUASAR-DFlash2-nvfp4-NInfer` artifact first named for `./run`. It is a
> `.ninfer` file of the dense 27B; Basalt loads only Flash-Next `.basalt` packs
> (`context/distilled.md`). It belongs in `../ninfer`.
>
> **Widened (owner request, 2026-11-20):** also serve Tencent's **Hy4-flash**, which Basalt builds from
> v0.5.0 on, through a second launcher, `./run-hy4` (Plan 11). The out-of-scope note above predates
> that release: Basalt now loads four model families, and this project serves two of them. `.ninfer`
> files are still NInfer's.
>
> **Ordering principle:** boot before tune — a pinned image and a `./run` that boots a pack land
> before any setting is called optimized.

## Currently Active <!-- hp:active -->

<!-- One line per in-progress plan, in the order they should be worked:
     - **Plan NN** — <title>: in progress, X/Y tasks done; next TN (<short title>).
     When nothing is in progress: "None in progress — next up: Plan NN (`/hands-plan resume NN`)." -->

- **Plan 08** — Repin Basalt to v0.4.1 and rebuild: in progress, 3/5 tasks done (T1: `./run` honours a
  pack argument again; T2: pin at `d3b7574`, image `cu1343-sm120-d3b7574` built, gate 1–6 pass on it; T4:
  Plans 02 and 03 abandoned, Plan 09 registered); next **T3**, **blocked on the owner**: `./run` and
  `./run --single`, a real reply each. The new image has not been booted. Then T5, harvest and close.
- **Plan 10** — Repin Basalt to v0.5.0 and rebuild: in progress, 1/3 tasks done (T1: pin at `ad274aa`,
  image `cu1343-sm120-ad274aa` built, gate 1–6 pass on it); next **T2**, half recorded (the default
  boot came up on engine 0.5.0 with 3 × 362144) and **blocked on the owner**: a real reply from that
  boot, then `./run --single` with a reply. Then T3, harvest and close. Plan 08 stays
  open beneath it (owner decision 2026-11-20: settle it later).
- **Plan 12** — Buffered-io patch: in progress, 2/5 tasks done (T1: `patches/0001`, the Dockerfile
  step, `./build`'s fit check; T2: `BUFFERED_IO=1` by default in `./run`, `./run-hy4`, `./preload`);
  next **T3**, **blocked on the owner's `./build`** (the image on disk has no patch, so the switch
  does nothing yet), then **T4**, the owner's Hy4-flash boot from NFS.
- **Plan 11** — Hy4-flash launcher `./run-hy4`: in progress, 3/5 tasks done (T1: the launcher,
  `pack_arch`, gate step 5b; T2: `./run` refuses a Hy4-flash pack; T5: `./preload`); **T3 is blocked
  on the owner**: the first boot loaded and answered a short prompt, then a 14,150-token prompt stalled
  on Engram reads from NFS. It needs `./preload` (or a local copy), a boot, and a quoted reply. Then
  T4, harvest and close.
- **Plan 13** — Repin Basalt to v0.5.5 and rebuild: in progress, 1/3 tasks done (T1: pin at `ff0b978`,
  image `cu1343-sm120-ff0b978` built with the buffered-io patch, gate 1–6 with 5b pass on it); **T2 is
  half recorded and blocked on the owner**: `./run` serves on the new image (engine 0.5.5, buffered reads,
  3 × 362144); still needed, `./run-hy4` on it with a reply. Then T3, harvest and close.

## Plans <!-- hp:plans -->

Keep **Notes** to ~25 words — the detail lives in the plan's tracker.

| Plan | Title | Depends on | Status | Notes |
|------|-------|-----------|--------|-------|
| 01 | Basalt Flash-Next container: pinned sm_120 image, `./run` on :8700 | — | done | `./run` boots the local DQ6_K pack in 40 s (4×262144) / 34 s (`--single`); all experts in VRAM; serves `cosmo-6000` on `cosmo.lan:8700`. → `completed/`. |
| 02 | Basalt vendor audit: leak / theft / keylog / phone-home in `vendor/basalt` | — | abandoned | 2026-11-16, at 5/15: the pin moved to `d3b7574`. Manifest, census and `posture.md` stand for `dc047a7`. → `abandoned/`; re-audit is Plan 09. |
| 03 | Basalt audit, runtime half: observed egress, exec and input capture | 01 | abandoned | 2026-11-16, never started: its target was the `dc047a7` image. → `abandoned/`; folded into Plan 09. |
| 04 | Basalt audit, engine bulk line-by-line: `src/kernels` + `src/core` + `include/` | 09 | pending | Was the deferred half of Plan 02 (~92k lines **at `dc047a7`**). Hypothesis only: paths and line counts must be re-derived at `d3b7574`, where model code moved to `src/models/<model>`. |
| 05 | Tune Basalt past the owner's defaults on this card | 01 | pending | Measured A/Bs: `basalt-probe` suggestions, `expert-profile-save`, `prefill-cpu-share`, `spec` depth, `kv` q4_0/k8v4; 4 slots vs `--single` under load. |
| 06 | `sm_120a` build A/B | 01 | pending | Basalt accepts `120a` but tests only `120`; build both, compare decode/prefill and output identity. |
| 07 | Boot from the defragmented NFS copy | 01 | pending | noir's pack now has 927 extents: does a boot from `/mnt/noir` load fast enough to retire `models.local/`? |
| 08 | Repin Basalt to v0.4.1 and rebuild the image | 01 | in-progress | `dc047a7` → `d3b7574` (v0.4.1 tag commit, 698 commits on). Owner builds and boots; T4 abandons Plans 02/03 and registers the re-audit. |
| 09 | Basalt vendor audit of the v0.4.1 pin `d3b7574`: static reading + runtime observation | 08 | pending | Row only, no plan file yet. Re-targets Plans 02 + 03; reuses `context/audit/scripts/` and the canary method; starts with tag signature + tree identity. |
| 10 | Repin Basalt to v0.5.0 and rebuild the image | 01 | in-progress | `d3b7574` → `ad274aa` (v0.5.0 tag commit, 206 commits on). Owner builds and boots. Plan 08's open rows and Plan 09's target left for later (owner, 2026-11-20). |
| 11 | Hy4-flash launcher: `./run-hy4` | 10 (T1's image) | in-progress | `./run` passes Flash-Next's flags, which Hy4-flash refuses (`--ple-io`, `--kv int8`). Second launcher, per-pack flag check in the gate (step 5b), one owner boot. |
| 12 | Local patch: a switch that stops Basalt's `O_DIRECT` reads | 10 (T1), 11 | in-progress | `BASALT_BUFFERED_IO=1`, a 3-file patch applied in the image build; the launchers and `./preload` turn it on. Owner rebuild and one Hy4-flash boot from NFS. |
| 13 | Repin Basalt to v0.5.5 and rebuild the image | 10 (T1), 12 (T1) | in-progress | `ad274aa` → `ff0b978` (v0.5.5 tag commit, 177 commits on; a fifth model, Inkling 2 Mini). The patch fits unchanged. Owner builds and boots. |
| 14 | Inkling 2 Mini launcher: `./run-ink` | 13 (T1's image), 11 | pending | Third launcher, for the model v0.5.5 added: `--kv fp16`, no table flags; refuses a pack still downloading. Gate step 4 on synthetic packs until the real one lands; one owner boot. |

## Dependency Chain <!-- hp:chain -->

```text
completed/01_flashnext_container ──(recipe, ./build, ./run; feeds 05, 06, 07)── 08_basalt_repin_v041 ──(the pin at T2)── 10_basalt_repin_v050
                                                                                      │
                                                                  09 (audit of d3b7574, row only)
                                                                                      │
                                                                             04_audit_engine_bulk
10_basalt_repin_v050 ──(the v0.5.0 image, built at T1)── 11_hy4flash_launcher ──(the NFS stall at T3; ./preload)── 12_buffered_io_patch
10_basalt_repin_v050 ──(the pin at T1)── 13_basalt_repin_v055 ──(patches/0001 must fit the new pin)── 12_buffered_io_patch
13_basalt_repin_v055 ──(the v0.5.5 image, the first with inkling2)── 14_inkling_launcher ──(shaped after)── 11_hy4flash_launcher
abandoned/02_basalt_vendor_audit, abandoned/03_basalt_audit_runtime ──(method, scripts, posture.md)──▶ 09
```
*2026-11-16:* Plans 02 and 03 audited `dc047a7`; Plan 08 moved the pin, so both are abandoned and Plan 09
inherits their method. *(The 2026-11-05 chain, with 03 gating 02's verdict, is in git history.)*

## Backlog Rationale <!-- hp:backlog -->

- **02 — Vendor audit.** *(Abandoned 2026-11-16; kept for the reasoning Plan 09 inherits.)* The owner asked whether the vendored engine does anything beyond being a
  nice LLM server. Recon (2026-11-05) says clean and security-conscious; the plan's job is to make
  that *provable* rather than plausible — a manifest so coverage is countable and a planted-canary
  census so "the grep found nothing" cannot be laundered into "audited".
- **03 — Runtime half.** *(Abandoned 2026-11-16, never started; kept for the same reason.)* Every static method has the same blind spot (`dlopen`+`dlsym`, raw
  `syscall()`, venv C extensions opening their own sockets); only observed syscalls and network
  state close it. Split from 02 deliberately: Standing Constraints make it owner-run and Plan 01 is
  already stalled on owner boots, so the code verdict must land without inheriting that stall.
  *2026-11-05 re-scope:* pulled forward. The stall it was split off to avoid is gone — the image for the pin exists and docker is reachable from an agent shell — and it is the most direct answer to "phones home" and "keylogs".
- **04 — Engine bulk.** 125k lines of C++/CUDA cannot be read line-by-line in one plan at honest
  adversarial speed. Owner chose boundary-first now, this later (Plan 02 § Scope, dated 2026-11-05),
  so the deferral is a numbered plan rather than an invisible gap.
- **08 — Repin to v0.4.1** (owner request, 2026-11-16). The pin was `main` on 2026-11-04; upstream has
  since released 0.3.0–0.4.1 (multi-model engine, new RAM plan, a dense 27B). A pin change is a plan by
  `RULES.md`'s first table: flags can vanish without shims, so the bump is proven by gate steps 5–6 on the
  new image and by an owner boot, not by the build finishing.
- **10 — Repin to v0.5.0** (owner request, 2026-11-20). Upstream released 0.5.0 on 2026-11-19: a fourth
  model (Hy4-flash) and shared-engine cache work; its notes say no flag or config key was removed or
  renamed. Still a plan by `RULES.md`'s first table, proven the same way as Plan 08: gate steps 5–6 on
  the new image and an owner boot. Plan 08 is still open beneath it and Plan 09's row still names
  `d3b7574` — the owner's call to settle both later (Plan 10 tracker, Follow-ups).
- **11 — Hy4-flash launcher** (owner request, 2026-11-20). The owner ran the new `Hy4-flash-NVFP4` pack
  through `./run` and the server refused `--ple-io`: engine flags are per model, and gate step 5 checks a
  name against the whole binary. A new launcher profile is a plan by `RULES.md`'s first table. Depends
  on Plan 10 only for its image (T1, done); it does not wait for Plan 10's boots.
- **12 — Buffered-io patch** (owner request, 2026-11-20). Plan 11's first boot stalled because Basalt
  reads with `O_DIRECT`, so cosmo's page cache never holds the Engram table and every lookup went to
  noir's disks. The owner asked for a switch, off in the engine by default and on in our launchers. It
  is the project's first carried patch: a Dockerfile change and a launcher default, both plans by
  `RULES.md`'s first table. Its boot (T4) is also the boot Plan 11 T3 is blocked on.
- **14 — Inkling 2 Mini launcher** (owner request, 2026-11-21, while the pack was still downloading). v0.5.5
  added the model and Plan 13 built it; neither launcher's flags are its own. A new launcher profile is a plan
  by `RULES.md`'s first table. Depends on Plan 13 only for its image (T1, done). The launcher and its gate
  checks land before the pack does, on packs the gate writes itself; the real pack joins at T3.
- **13 — Repin to v0.5.5** (owner request, 2026-11-21: "repin to this version so I can build and publish").
  Upstream released 0.5.5 on 2026-11-21: a fifth model (Inkling 2 Mini) and shared-engine work; its notes
  say no flag or config key was removed or renamed. The first repin with a carried patch: the three files
  `patches/0001` touches are the same blobs at both pins. Proven as Plans 08 and 10 were: gate steps 5, 5b
  and 6 on the new image and an owner boot per launcher. Plans 08, 10, 11 and 12 stay open beneath it.
- **09 — Audit of the v0.4.1 pin** (registered 2026-11-16 at Plan 08 T4, owner decision). The built
  engine is now unaudited: Plan 02 read none of `d3b7574`. What carries over is method, not results — the
  manifest-then-census shape, the planted canaries, `context/audit/scripts/` (which hard-code the old pin
  and Plan 02's path and must be re-targeted first), and Plan 03's design for watching a boot on a
  no-egress network. First tasks: fetch the `v0.4.1` tag and check its signature, compare the pin's tree
  with GitHub's tree object (`high_level.md` § *Attesting the Basalt pin `dc047a77`*), rebuild the
  manifest. `audit/posture.md`'s action list (the LAN-open port with no API key) needs no re-audit to act on.
- **05–07 — Plan 01's follow-ups** (registered at its close, 2026-11-04). 05: the defaults are the
  owner's choices, not measured winners (ordering principle: boot before tune). 06: `120a` is accepted
  but untested upstream. 07: the NFS crawl was fragmentation + `O_DIRECT`; noir's copy is now
  defragmented, so the local copy may be unnecessary.

<!-- Add these sections when they earn their place:

## Dependency Chain
```text
01_foundation
    ├── 02_feature_a
    │     └── 04_feature_c
    └── 03_feature_b
```

## Milestones
- After **01**: <an observable capability, phrased so it can be checked>.

## Execution Order
1. <which plans, in what order, and what may run in parallel>

## Backlog Rationale
- **NN — Title.** Why it exists and what it is blocked on, so whoever picks it up starts with context.
-->

## Abandoned / Superseded <!-- hp:abandoned -->

| Plan | Reason | Superseded by | Date |
|------|--------|---------------|------|
| 02 — Basalt vendor audit | Owner decision at the repin (Plan 08). The pin moved from `dc047a7` to `d3b7574` (v0.4.1): 698 commits between them, most of `src/` moved into per-model folders, two models added. The 862-file manifest, the census and the 5 finished tasks (T0–T3, T13) describe a tree we no longer build; finishing the read would audit code the server does not run. Not a failure of method. `audit/posture.md` still applies. | Plan 09 | 2026-11-16 |
| 03 — Basalt audit, runtime half | Same decision. Never started; it was to observe the `dc047a7` image, which is now only the rollback image. | Plan 09 | 2026-11-16 |

*(Record the reason. A plan dropped without one gets re-attempted six months later by someone who
doesn't know it failed.)*

## Standing Constraints <!-- hp:constraints -->

- **The owner runs every long or GPU step** (the convention in `../ninfer` and `../vllmcustom`,
  carried here at init 2026-11-04): the image build, the pack download, every boot and every
  bench. Agents write the scripts, verify them with the gate and dry runs, and record what the
  owner pastes. A task whose behavior is runtime-only is not `done` until the owner has run it.
- **No `sudo` anywhere; everything runs as yourself over plain `docker`.** Containers run as
  uid/gid `1000:1000`. Never wrap a script in anything that scrubs the environment (`env -i`, a
  login shell). Same contract as `../vllmcustom` (its `context/distilled.md`).
- **One server on `:8700` at a time.** `../vllmcustom` (container `vllm`), `../ninfer` and
  `../ignis` all serve there; stop the running one before booting Basalt.
- **One Blackwell card.** GPU 0 is the RTX PRO 6000 (sm_120, 96 GiB); GPU 1 is an RTX 4060
  (sm_89) that Basalt cannot use — the container sees GPU 0 only. Host RAM is 125 GiB, the tier
  for experts that do not fit in VRAM (`nvidia-smi`, `free -g`, 2026-11-04).

---

Completed plans move to `completed/`, abandoned ones to `abandoned/` (RULES.md Rule C). When this
file passes ~30 KB, move the narrative of fully closed series to `completed/SERIES_ARCHIVE.md` and
leave one summary row per series here.
