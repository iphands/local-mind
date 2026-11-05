# context/ — pitfalls for the whole local-mind tree

Traps that generalize across the sub-projects (`server/basalt`, `server/vllmcustom`, `server/ninfer`,
`server/ignis`, `opencode2`). Project-specific detail stays in each sub-project's own `context/`.
Shape per `server/basalt/context/AGENTS.md`: Problem / How to avoid / Sources, tagged
`[OBSERVED YYYY-MM-DD]` when it actually bit.

---

# A shared worktree has no such thing as "my uncommitted edits are private"

*Correction (host agent session, 2026-11-04 PST = 2026-11-05 UTC): the second writer was **another
agent session** (on the host, in the `docker` group), not the human — `b308244` is that session's
commit (`server/basalt` commits `0b1efb6`, `7c6d622`). Read "the host session" for "he"/"the human"
below; the lesson is unchanged, and it holds for any second writer.*

## Problem
[OBSERVED 2026-11-05] In `server/basalt`, an agent and the human worked in the **same worktree** — he to
run the GPU and docker steps that only his shell can run, the agent to write plans and knowledge. He
committed; the commit collected the agent's in-flight files. The commit was subject-lined
*"record memlock unlimited inside the container (plan Q2 resolved)"* and also carried 30 lines of the
agent's gate-status block plus two tracker edits, under his subject and authorship. What that costs:
1. Work attributed to a message that does not describe it and an author who did not write it; `git log -S`
   and blame now point at the wrong task.
2. **Both writers edited the same bullet within the same minute and it merged into a self-contradicting
   paragraph** — a `RESOLVED …` verdict with the other party's `— still open: …` glued onto the end of the
   same sentence. Git reports nothing wrong; the next session just reads nonsense in an authoritative file.
3. The agent's model — "that edit is uncommitted, it is still on the bench" — became false silently. It
   was in HEAD, and the append-only rule then removes the escape hatch: `--amend`, `rebase`,
   `reset --hard`, `revert`, and `stash` / `checkout -- <path>` / `restore` / `clean` on work you did not
   make are all banned. **There is no safe undo after a collision — only forward commits.**

## How to avoid
- The writer with more bandwidth keeps the tree clean. **Never end a turn with knowledge files dirty**,
  especially not while the human is live; stage **explicit paths** as each logical unit finishes
  (`git add <paths>`, never `-A` / `-a`) to shrink the in-flight window to minutes.
- **Name the dirty files in every handoff**: `dirty: context/pitfalls.md (mine, in progress)`. One line,
  and the only signal the other writer can act on without reading a diff.
- Re-read before writing a hot shared file: `git log -1 --format='%h %ad %s'` + `git status`, then read
  the *current* region rather than trusting what you last wrote. A self-contradicting sentence is the tell.
- On discovery, **HEAD is ground truth; fix forward with dated notes.** Never undo the other writer's
  commit; keep their verdict and measurements verbatim, delete only your own mechanical half, and put your
  disagreement in as a separately attributed note instead of editing their sentence.
- Skip the machinery: lock files, per-agent worktrees, and hooks that refuse the human's commits. He has
  to be in that worktree for GPU work, and a hook that blocks him gets routed around within a week.

## Sources
- local-mind: `server/basalt/context/pitfalls.md` § *A shared worktree has no such thing as "my
  uncommitted edits are private"* (the full incident, with the project's git forensics)
- local-mind: `server/basalt/context/plans/RULES.md` § Rule B2 (append-only, banned-command table),
  § Rule B.4 (`git add <paths>`, never `-A`)
- local-mind: `server/basalt/context/plans/completed/01_flashnext_container_tracker.md` § Notes / Deviations

---

# Models on noir's `/main` land fragmented, and `O_DIRECT` loaders crawl on them

## Problem
[OBSERVED 2026-11-04] Basalt's first boot of a 131.7 GB pack from `/mnt/noir/scratch` (NFS 4.2/RDMA)
loaded at 5–10 MB/s; vLLM loads from the same share at GB/s. Basalt reads weights with `O_DIRECT`, so
noir's disks are hit directly. noir's `/main` is btrfs on a PERC H710P array of **spinning** drives
(`rotational=1`, `read_ahead_kb=128`), and the downloaded pack was in **131,723 extents** with almost
none physically adjacent (a parallel, out-of-order download). A cold read became ~125k seeks. With the
file in noir's page cache, `O_DIRECT` reads from cosmo ran at 749 MB/s, so the network was not the cause.
A defrag "yesterday" did not help: the file was written after it.

## How to avoid
- After downloading a large model to `/main`, check and fix it on noir:
  `filefrag <file>` (thousands of extents = fragmented), then
  `btrfs filesystem defragment -v -f -t 128M <file>` (128 MiB is btrfs's largest extent; the 131.7 GB
  pack went 131,723 → 927 extents). `chattr +m` (no compression) only affects data written after it.
- For a loader that uses `O_DIRECT` (Basalt), keep a copy on local NVMe: 40 s boots instead of hours.
- Diagnose from the client: engine threads in state `D` with `wchan` `nfs_file_direct_read`.

## Sources
- local-mind: `server/basalt/context/distilled.md` § *Basalt loads weights with `O_DIRECT`, and there is no switch*
- local-mind: `server/basalt/context/pitfalls.md` § *A boot from the NFS copy crawls at 5–10 MB/s*
- local-mind: `server/basalt/context/plans/completed/01_flashnext_container_tracker.md` (boot 2 rows)

---

# Cross-repo git forensics: five standard commands that run and lie when two histories were rewritten

## Problem
[OBSERVED 2026-11-05] Attributing a vendored pin to its upstream. `server/basalt`'s pin declares itself a
fork of another project, and **every conventional way of relating two repositories produced a confident,
plausible, wrong answer.** They share **0** commit SHAs and have **no merge-base**, because one side
rewrote its published mainline (the only measured difference between thousands of content-identical commit
pairs was the removal of AI co-authorship trailers). The failures, each of which looked like evidence:

1. **`git log <upstream>/<branch>..<pin>`** — with no merge-base, `A..B` is *just `B`*. It printed the
   pin's entire history as "new work not in upstream", which reads as a clean independent fork.
2. **`git cherry` / cross-repo `bisect` / blame continuity** — same cause, same silence.
3. **`git merge-base`** — it does **not** tell you "histories were rewritten", and it does not say so: it
   exits **1 and prints nothing**, which is the dangerous form of silence, because
   `mb=$(git merge-base A B)` is simply empty in a script that never checked `$?`. The probe that means
   what you want is the **intersection of two `git rev-list` outputs**, printed with its marginals
   (`|A|`, `|B|`, `|A∩B|`) — an empty intersection *on its own* is also what a failed `rev-list` gives.
   **`merge-base --independent` is not an unrelatedness test**: it answers "does either input reach the
   other", so a branch that shares ancestry returns the same `2` as two unrelated repositories.
4. **Counting shared objects inside a partial clone you have already queried** — lazy-fetch pulls the
   peer's objects into *your* store, after which `cat-file -e` says they are present and a shared-object
   count "found" hundreds of "shared" commits that a pristine clone of the same URL does not contain —
   they were objects the investigator's own earlier `git log` and `cat-file` calls had pulled on demand.
5. **Asking the forge's per-repo commit API for our SHAs** — it resolves against the repository's
   **object store**, not its default branch, so an object reachable only from a stale ref returns `200`.
   That is what manufactured a false fork boundary.

Two more, off-git: **abbreviated SHAs break promisor probes** (`upload-pack` needs the full 40 hex to
answer "not our ref"), and an **API route that returns a uniform value for every input** — nine SHAs all
`404` on `/api/1/revision/<sha>/get/` — is a broken probe, not a finding; the real route is
`/api/1/revision/<sha>/`, and it holds the ancestor repo and the fork's own root commit.

## How to avoid
- **Never compare commit SHAs between two repositories whose histories may have been rewritten.** Join
  them by **content key** — `(tree, author-date, committer-date, subject)` — checking for duplicate keys
  on *both* sides first so the join is provably bijective. Files join by **blob SHA** (a content address,
  the one thing that does compare). Individual changes join by **`git patch-id --stable`**.
- `git diff A B` **across** two repos is sound: it is tree-to-tree and never consults ancestry. It is the
  exception; keep it.
- Get a **pristine** history before counting anything: `git fetch --filter=tree:0 --no-tags origin <branch>`
  into a scratch clone, then prove pristine-ness — `count-objects` goes `in-pack 0 → N` and there is no
  `objects/info/alternates`. Run object-presence probes **only** there, with **full 40-hex** SHAs; the
  promisor probe is `git cat-file -e <sha>^{commit}` reading `upload-pack: not our ref`.
- **Better, when the question is about two repositories: fetch both into ONE scratch store** with the same
  command and compare inside it. Then `comm -12 <(git rev-list A | sort) <(git rev-list B | sort)` answers
  "do they share any commit object" **directly**, with no cross-repo boundary to distrust and no query
  history of yours to contaminate it. Prove the store's hygiene by **composition, not by a sum**:
  `git cat-file --batch-all-objects --batch-check='%(objecttype)' | sort | uniq -c` should show exactly the
  two histories' commit counts and nothing unexpected (with a `--filter=tree:0` fetch, zero trees and
  blobs; any tag objects must be explained by a named fetch, since `--no-tags` cannot produce them), plus
  `count-objects -v` showing no loose objects. A total that "adds up" can hide an object of the wrong kind.
  Re-run every count under `GIT_NO_LAZY_FETCH=1` — that guards the *next* run, the composition listing is
  what audits the run you already did. (`FETCH_HEAD` is overwritten by the next fetch: capture it into a
  named ref after **each** fetch, or you will compare one repo against itself and get a confident answer
  about nothing.)
- Partition a history by date only on **`%at` / `%ct` epochs and an instant with its offset** — comparing
  `--date=iso-strict` output as a string buckets by the *author's local* calendar day and silently shifts
  the count (here: 882/1,312 vs the true 861/1,333).
- State which **ref** a cross-repo figure came from and when it was fetched. Version names collide across
  forks: `v0.1.0` and `v0.2.1` existed on both sides of this one, at **different objects**.
- Treat a forge's `fork=false` as "no fork-network link", nothing stronger. Treat commit messages, author
  fields, trailers and tag targets as **editable narrative**, not evidence; anchor external claims to a
  named snapshot hash plus a date, and prefer third-party archives over the forge attesting itself —
  while stating what those archives can and cannot enumerate (ref listings can be silently capped).

## Sources
- local-mind: `server/basalt/context/audit/wave0/t2-lineage.txt` §§ 0–7 (the four refuted hypotheses R1–R4,
  the content-key corpus, the 2×2 remote-presence matrix, the Software Heritage record with controls) and
  **§ 11b**, both histories in one object store — the `|A∩B|=0` measurement, the object-composition proof,
  and the silent-`merge-base` / `--independent`-is-not-a-test controls, all with raw output
- local-mind: `server/basalt/context/pitfalls.md` § *Cross-repo git forensics* (the project's fuller
  version, with the basalt-specific numbers this entry deliberately omits) and items 10–17
- local-mind: `server/basalt/context/high_level.md` § *Attesting the Basalt pin `dc047a77`* (the options
  table with each method's verdict); `server/basalt/context/audit/index.md` § *If you are about to run
  something against upstream* (the commands)
