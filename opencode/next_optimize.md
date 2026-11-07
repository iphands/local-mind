# jev / ask_jev: next optimizations (on hold)

Notes from 2026-11-06. Nothing under "Proposed changes" is applied yet.

## A/B result: `ask_jev` vs plain tools, on a grep-shaped question

Prompt pair, each in a fresh local-mind session (numbers from `container_data/local/share/opencode/opencode.db`):

- **With:** "Without reading them, use ask_jev to find which files under llama-proxy/src talk to the
  upstream server, then read only the hits." (`ses_e4cd1942cffehCQcCIswQpRDxu`, 15:25)
- **Without:** "Find which files under llama-proxy/src talk to the upstream server. Do not use the
  ask_jev tool; use your normal tools (grep, glob, read)." (`ses_e4cca30e1ffejfNUlA2EKICFyH`, 15:33)

| | With `ask_jev` | Without |
|---|---|---|
| Wall time, prompt to final answer | 197 s | 68 s |
| Main-agent steps | 17 | 12 |
| Main-agent prompt tokens processed | 541,676 | 384,890 |
| Peak context | 54,794 | 47,559 |
| Tool output added to context | 85.7k chars | 76.8k chars |
| Tool calls | 4 ask_jev, 12 read, 3 grep, 2 shell, 1 glob | 10 grep, 6 read, 1 glob |
| `ask_jev` work on the 4060 | 377,699 tokens in 90.9 s | — |
| Reviewers | 25 s + 47 s (hoodie also called ask_jev) | 43 s + 39 s |
| Total incl. reviewers | 810k prompt / 21.6k generated | 588k prompt / 21.2k generated |
| Final answer | same map (10 send sites in 7 files) | same map |
| Reviewers caught | missed `proxy_passthrough` (handler.rs:1899) | missed streaming.rs:841, handler.rs:1793 |

One run each, so it's noisy, and the "with" wall time includes a second turn spent waiting for hoodie.
Still, ~3x slower with equal correctness is a clear signal for this kind of question.

Why it lost:
- `ask_jev` was an extra step, not a replacement: the agent still grepped and read to verify
  (answers are hints, as instructed), and read **more** than the control (12 reads, 46.6k chars,
  against 6 reads, 28.3k chars).
- The question is grep-shaped (`.send()`, `http_client`): one `rg` answers it exactly.
- The per-call budget (400k chars) split the crate into 4 calls; ~91 s of 4060 time.
- Options classification over whole files had the predictable false positives: `lib.rs` and
  `proxy/mod.rs` only declare modules, but their module names sound like HTTP code.

## Re-run on the RTX 6000 (same prompt, jev server moved)

jev now runs on the 6000 (`server/llamacpp/run-jev-6000`, `16:1:2048:40960`, started with `PORT=8701`
so the plugin's default URL reaches it). Same "with" prompt, fresh session
(`ses_e4c80e724ffePAdvvxMZpGMyKr`, 16:53), compared with the two runs above:

| | 6000 `ask_jev` | 4060 `ask_jev` | No `ask_jev` |
|---|---|---|---|
| Wall time | 123 s | 197 s | 68 s |
| Time in `ask_jev` | 20.1 s for 408k tokens | 90.9 s for 378k tokens | — |
| Main-agent steps | 17 | 17 | 12 |
| Files read | 17 (61k chars) | 12 (47k) | 6 (28k) |
| Main-agent prompt tokens | 741k (peak context 68k) | 542k (peak 55k) | 385k (peak 48k) |
| Total incl. reviewers | 1.11M prompt / 34k generated | 810k / 22k | 588k / 21k |
| Reviewers | 49 s + 69 s | 25 s + 47 s | 43 s + 39 s |
| Answer | same file map | same map | same map |

- **Faster than without `ask_jev`? No**: 1.8x slower (it was 2.9x on the 4060).
- **Better? No**: the same map once reviewers weigh in, at ~1.9x the total tokens and almost 3x the
  file reads. The agent still verifies `ask_jev`'s answers by grepping and reading, so it remains an
  extra step rather than a replacement.
- **Faster than the 4060 run? Yes**: 74 s faster overall; `ask_jev` itself 4.5x faster (91 s → 20 s),
  matching the benchmark. The remaining ~100 s is the main model plus reviewers, which took longer
  than on the 4060 run (49 + 69 s vs 25 + 47 s). That may be basalt sharing the 6000 with jev's scans,
  or just a different path through the task; one run can't tell.
- Two attempts during the server swap (16:49, 16:51) hit `cooling down` / `ECONNREFUSED` and fell
  back to reading (167 s, ~1M tokens) or stopped early: the fail-open path works.

One run per arm, so the numbers are noisy, but the 68 s vs 123 s gap is large. This strengthens
change 1 below (grep first). `ask_jev` on the 6000 is now fast enough that change 2 (the semantic
pair) is the real test of whether it earns its place.

## Do scans and shell checks overlap in real use? No

Checked 2026-11-06, ~17:00, against everything since jev moved to the 6000 and the last ~6 hours of
sessions:

| Source | Result |
|---|---|
| `llama-jev-6000` server log (234 requests since start) | **0 of 27** short requests (shell checks, small verdicts) ran while an `ask_jev` chunk was in flight. Most requests in flight at once: 6 (multi-question hooks) |
| `opencode.db` tool-call timings (`time.created` → `time.completed`), all sessions | **0 of 143** shell calls ran while one of 19 `ask_jev` calls was running |
| Same, `ask_jev` against `ask_jev` | **3** overlapping pairs (the agent's "parallel batches"; the global queue now serializes them) |

Why: the agent works step by step (`ask_jev`, wait, then the next command), and the reviewers can't run
shell commands (their permissions deny it), so they never trigger F2/F3. The overlap the benchmark's
mixed and sweep phases model only happens with several sessions or agents at once (a swarm, or two
OpenCode sessions), or if the agent sends `ask_jev` and a shell command in one message (not seen yet).
So those phases are a worst-case stress test, not what one session produces: in single-session use a
shell check on the 6000 is answered alone, ~33 ms.

What it means for using both GPUs at once (the plugin talks to one `JEV_URL` today):
- **Split by job** (hooks on one server, `ask_jev` on the other) fixes a problem a single session
  doesn't have. It only pays off for multi-session / swarm use (change 6).
- **Share `ask_jev` chunks across both servers** (each takes the next chunk from one queue, so the
  faster card does more) helps today: scans ~20% faster (6000 ~20k tok/s + 4060 ~4k), since the 4060
  sits idle during a scan.
- **Failover** helps regardless: it would have saved the two runs that failed during the 16:49-16:51
  server swap.
Note (superseded the same day): the servers now run on fixed ports, the 6000 on :8710 and the 4060
on :8711; :8702 stays the benchmark's port.

## Re-run with both servers (share mode)

Same "with" prompt, fresh session (`ses_e4c6bda2affeKZ2sRkEqAqVr3Y`, 17:16), the plugin using both
servers in share mode (6000 :8710 + 4060 :8711), compared with every earlier run:

| | Both servers (17:16) | 6000 only (16:53) | 4060 only (15:25) | No `ask_jev` (15:33) |
|---|---|---|---|---|
| Wall time | 77 s | 123 s | 197 s | 68 s |
| Time in `ask_jev` | 17.8 s for 440k tokens | 20.1 s for 409k tokens | 90.9 s for 378k tokens | — |
| `ask_jev` throughput | ~24.7k tok/s | ~20.3k tok/s | ~4.2k tok/s | — |
| Main-agent steps | 16 | 17 | 17 | 12 |
| Files read / greps | 13 (49k chars) / 5 | 17 (61k) / 9 | 12 (46k) / 3 | 6 (28k) / 10 |
| Main prompt tokens (peak context) | 538k (56k) | 740k (67k) | 541k (54k) | 384k (47k) |
| Total incl. reviewers | 708k / 19k generated | 1.11M / 33k | 809k / 21k | 588k / 21k |
| Reviewers | 25 s + 31 s | 49 s + 69 s | 25 s + 47 s | 43 s + 39 s |
| Key sites in the answer | 7/8 (missed `handler.rs:1793`) | 6/8 | 7/8 | 8/8 |

- **What the second server bought:** `ask_jev` throughput +22% (~24.7k vs ~20.3k tok/s; the 4060
  took 34 of 210 requests, the 6000 176). In time, only ~2-3 s here (17.8 s vs 20.1 s for fewer
  tokens). The agent made 4 `ask_jev` calls in a row (~5 s each, 17:16:35 → 17:16:57), re-asking
  about narrower sets of files after the first call hit its 400k-char budget.
- **Where the 46 s gain over the 6000-only run came from:** mostly not jev. The slower reviewer took
  31 s against 69 s (reviewers run in parallel, so the slower one sets the time: ~38 s), and the
  agent took a shorter path (13 reads vs 17, 202k fewer main-agent prompt tokens). One run per
  setup, so that is run-to-run variance; the jev part got ~10% faster.
- **Against no `ask_jev`:** close on speed now (77 s vs 68 s; it was 1.8x on the 6000 alone and 2.9x
  on the 4060), still more costly (708k vs 588k total tokens, 13 reads vs 6, because the agent still
  checks `ask_jev`'s answers by reading), about the same quality (7/8 vs 8/8 key sites; the miss,
  `handler.rs:1793`, is one the earlier `ask_jev` runs also struggled with).

Bottom line: both servers make `ask_jev` as fast as this hardware allows, and it is now a small share
of the run (~18 of 77 s). On a grep-shaped question what decides the outcome is how much the agent
reads and how fast the reviewers answer, not jev. Changes 1 (grep first) and 2 (the semantic pair)
below are what can still show whether `ask_jev` earns its place.

## Proposed changes

1. **Grep first: implemented, being compared.** Two halves:
   - **`agents/local-mind.md`** now says to grep whenever the question can be written as a pattern,
     including behaviour with an obvious code signature. The reviewers each got one line, and the
     tool description matches.
   - **`ask_jev` router** (`plugins/jev.ts`): before scanning, one decision-model request asks
     whether a plain text search would answer the question. Calibrated on 40 questions: semantic
     ones scored p(grep) 0.05–0.37 (none routed at the 0.80 threshold), grep-shaped ones 0.89–0.96
     (15 of 17 routed). Modes, switchable at runtime via `~/.config/opencode/jev-router`:
     - `advise` (default): answer "use grep"
     - `rg`: run ripgrep for the agent's `pattern`
     - `off`: always scan

     `force: true` skips it. Live on "which files call send()": advise 56 ms; rg 36 ms with all 9
     matching files; the old scan 4.8 s, covering only 17 of 40 files and answering "no" for two
     files that do call it.

   **Comparison protocol.** Each in a fresh local-mind session; numbers via
   `python3 -I scripts/session-report.py --match "<prompt start>" --latest N`:
   - Grep-shaped, once per mode (`echo "off|advise|rg strict" > container_data/config/opencode/jev-router`
     before each; `strict` makes the plugin ignore the agent's `force`, so the prompt's "use ask_jev"
     can't skip the router. A first run at 17:37 had the agent force every call because the old
     rule treated "use ask_jev" as a request to scan; the rule now says to call it without `force`):
     "Without reading them, use ask_jev to find which files under llama-proxy/src call reqwest's
     send(), then read only the hits."
   - The same question with "Do not use the ask_jev tool; use your normal tools" as the control.
   - Change 2's semantic question in `advise` mode, where the router must pass it through to a scan.
2. **Run the semantic pair before judging `ask_jev` overall.** Same method (fresh sessions, one
   with and one with "Do not use the ask_jev tool"), with a question grep can't express, e.g.
   "Which files under llama-proxy/src decide which backend a request goes to, or retry/fail over
   when one is down?" Pull the numbers with `scripts/session-report.py`. If `ask_jev` doesn't win
   there either, narrow it to the saved-output case. (Live check: the router scored this question
   p(grep) 0.12, so it scans.)
3. **Combining chunks for `options`.** Today: a length-weighted mean, which turns multi-purpose
   files (`augment.rs`, `handler.rs`, `main.rs`) into "unsure". Option: count a label that any
   part of the file matches strongly (max per label, as yes/no already does), and show the line
   range. Re-run the calibration (`llama-proxy/src`, the 4 questions) before and after.
4. **Big files.** `handler.rs` (260k chars) is always partial at `ASK_FILE_MAX_CHARS` = 200k and
   eats most of a call's 400k budget. Consider a smaller per-file cap plus a "partial" nudge to grep,
   or skip files over the cap with a hint to use grep there.
5. **Measure what jev costs basalt on the 6000.** Both share its compute, and an `ask_jev` scan keeps
   prefill busy (~762k tokens in 2 minutes were seen live). Time a normal basalt prompt with and
   without a concurrent scan (`jev-bench-load.py` against the 6000).
6. **Keep gates apart from heavy requests under high concurrency.** The 6000 sweep
   (`bench-results/jev-6000-20261106-161444`) tops out at ~9.5 req/s for the jev mix whatever the
   slot count, and llama.cpp has no request priority, so a 250-token shell check waits behind
   6k-token reviews (gate p95 13-35 s at 64-128 clients). Options: hooks on one jev server and
   `ask_jev` / reviews on the other (4060 + 6000 side by side, two `JEV_URL`s), or try a smaller
   `--batch-size`. Only matters for multi-session / swarm use: one session never overlaps them
   (see the section above).

## Already done (for reference)

- Server: 8 slots with unified KV on the 4060 (`server/llamacpp/run-jev-4060`, benchmarked with
  `run-jev-benchmark 4060`, results in `bench-results/jev-20261106-134340`).
- Server on the 6000: `run-jev-6000`, 16 slots, UB 2048, CTX 40960 (`run-jev-benchmark 6000`,
  results in `bench-results/jev-6000-20261106-155122` and `jev-6000-20261106-161444`): 4-7x
  faster than the 4060, same answers; `/v1/systemone` clips states at ~32k tokens.
- `ask_jev`: global queue (one request in flight across calls). Live test: 3 parallel calls, the
  server never had more than 3 slots busy, shell checks p50 583 ms / max 985 ms, no cooldown.
- Both jev servers from the plugin (was change 7): `JEV_URLS` (default `http://cosmo.lan:8710,http://cosmo.lan:8711`,
  the 6000 then the 4060; launchers now default to those ports), `JEV_MODE=share` (default: hooks on
  the first server up with failover, `ask_jev` chunks pulled by one worker per server) or
  `JEV_MODE=split` (hooks on the 4060, scans on the 6000, each failing over to the other), per-server
  cooldowns and queues, one retry per failed chunk on another server. Live: a 398k-char scan in 5.4 s
  (48 requests on the 6000, 11 on the 4060), hook answered by the 6000 in 34 ms.
