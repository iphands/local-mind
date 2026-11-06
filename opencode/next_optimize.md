# jev / ask_jev: next optimizations (on hold)

Notes from 2026-11-06. Nothing here is applied yet.

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

## Proposed changes

1. **`agents/local-mind.md`: grep first.** If the question can be written as a pattern (a call,
   an import, a name, a string), use grep. Use `ask_jev` only when no pattern can express the
   question (semantic questions), or for a long saved output ("full output saved to <path>").
   Today the prompt says to reach for `ask_jev` *first* when scouting; that sent this run down the
   slow path. Mirror the rule in the tool description (`ASK_DESCRIPTION` in `plugins/jev.ts`) and
   in the reviewers' `## ask_jev` sections.
2. **Run the semantic pair before judging `ask_jev` overall.** Same method (fresh sessions, one
   with and one with "Do not use the ask_jev tool"), with a question grep can't express, e.g.
   "Which files under llama-proxy/src decide which backend a request goes to, or retry/fail over
   when one is down?" Then pull the numbers from opencode.db the same way. If `ask_jev` doesn't win
   there either, narrow it to the saved-output case.
3. **Combining chunks for `options`.** Today: a length-weighted mean, which turns multi-purpose
   files (`augment.rs`, `handler.rs`, `main.rs`) into "unsure". Option: count a label that any
   part of the file matches strongly (max per label, as yes/no already does), and show the line
   range. Re-run the calibration (`llama-proxy/src`, the 4 questions) before and after.
4. **Big files.** `handler.rs` (260k chars) is always partial at `ASK_FILE_MAX_CHARS` = 200k and
   eats most of a call's 400k budget. Consider a smaller per-file cap plus a "partial" nudge to grep,
   or skip files over the cap with a hint to use grep there.

## Already done (for reference)

- Server: 8 slots with unified KV (`server/llamacpp/run-jev-4060`, benchmarked with
  `run-jev-4060-benchmark`, results in `bench-results/jev-20261106-134340`).
- `ask_jev`: global queue (one request in flight across calls). Live test: 3 parallel calls, the
  server never had more than 3 slots busy, shell checks p50 583 ms / max 985 ms, no cooldown.
