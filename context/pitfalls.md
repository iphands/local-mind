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
