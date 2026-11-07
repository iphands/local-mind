# jev: decision-model hints for OpenCode

`jev.ts` is an OpenCode plugin that hands small classification jobs to a local 4B decision
model: H2O Lightning 4B, served on both GPUs (the RTX PRO 6000 by `server/llamacpp/run-jev-6000`
and the RTX 4060 by `server/llamacpp/run-jev-4060`), which the plugin uses together. The plugin
never asks the model to write text. It sends a block of text (the *state*) and a multiple-choice
question, and the model returns a probability for each option. One answer costs one prefill and
no decoding, so it is fast and cheap.

The plugin only ever **adds** information or **tightens** a permission. It never removes a
reviewer, never edits command output, never blocks a tool call, and never loosens a permission.
If the decision server is down, slow or returns nonsense, every hook leaves its call exactly as
it would have been without the plugin. See [Safety when the server is down](#safety-when-the-server-is-down).

It also adds one tool, `ask_jev` (F4), which lets an agent ask a question about files without
reading them. A tool is allowed to fail: when jev can't answer, `ask_jev` errors with a readable
reason, and the agent reads the files the normal way.

## The decision servers

| | RTX PRO 6000 | RTX 4060 |
|---|---|---|
| Launcher | `server/llamacpp/run-jev-6000` (`device=0`, beside basalt) | `server/llamacpp/run-jev-4060` (`device=1`) |
| Endpoint | `http://cosmo.lan:8710/v1/systemone` | `http://cosmo.lan:8711/v1/systemone` |
| Slots | 16 sharing a 40960-token KV pool, UB 2048 | 8 sharing a 40960-token KV pool, UB 512 |
| Speed | ~20k tok/s prefill; shell check ~33 ms, review ~0.4 s | ~4k tok/s; shell check ~130 ms, review ~1.9 s |

Both run `h2o-lightning-4b-Q8_0.gguf` (the stock GGUF has no decision API, so every call fails and
the plugin stays out of the way) on llama.cpp build >= 11773, probed against b12169. Settings come
from `server/llamacpp/run-jev-benchmark`. `/v1/systemone` runs a request's questions in separate
slots, and clips any state to ~32k tokens silently.

**How the plugin uses both.** `JEV_URLS` lists the servers, preferred first (default
`http://cosmo.lan:8711,http://cosmo.lan:8710`: the 4060 first, because basalt on the 6000 runs faster
when jev's hooks stay off it). `JEV_MODE` picks how they're used:

| Mode | Hooks (F1/F2/F3) | `ask_jev` scans |
|---|---|---|
| `share` (default) | the first server that is up (the 4060: shell check ~130 ms, review ~1.9 s), failing over to the 6000 | every server at once: each takes the next chunk when it's free, so the 6000 still does ~5x more of them whatever the order. Measured live: a 398k-char scan took 5.4 s, 48 requests on the 6000 and 11 on the 4060 |
| `split` | the last server (the 6000), failing over to the other | the first server (the 4060), with the 6000 on standby if it fails: scans stay off basalt's GPU, at ~4k tok/s (a 400k-char scan ~29 s) |

`split` keeps shell checks off the GPU that runs scans. That only matters when several sessions run
at once: in one session the agent never runs a shell command while a scan is going (checked
against the server log and `opencode.db`, see `opencode/next_optimize.md`). `JEV_HOOK_URL` and
`JEV_TOOL_URL` override split mode's choice. `JEV_URL` alone (with `JEV_URLS` unset) still means a
single server.

Each server has its own 60 s pause after a failure, and its own one-at-a-time `ask_jev` queue.

Request and response shape:

```jsonc
// request
{ "state": "<text to judge, question included>",
  "questions": { "verdict": { "type": "choice",
                              "criteria": { "pass": "the command succeeded ...",
                                            "fail": "the command failed ..." } } } }
// response
{ "answers": { "verdict": { "choice": "pass",
                            "probabilities": { "pass": 0.90, "fail": 0.04 } } } }
```

Things learned while probing the API, which shaped the code:

- Unknown keys such as `question` are ignored, so the question has to be written into `state`.
- Every question re-prefills the whole state (about 4k tok/s on the 4060). Pack a checklist
  into one `choice` question rather than asking one question per item.
- The model reads the criteria descriptions. They carry the meaning of each option, so the
  wording matters more than the label.

## What it does

### F1: reviewer pre-screen (`execute.before` on `subagent`)

Fires when the agent launches `neckbeard` or `hoodie`. It does not fire on a follow-up message
to a review that is already running (one that has a `sessionID`).

1. Pulls the reviewed material out of the prompt: the fenced code blocks and diffs if there
   are any, otherwise the whole prompt. Text over about 6k tokens keeps its head and tail.
2. Asks three questions in one request (15 s timeout):
   - `neckbeard_flag`: correctness and safety patterns (`type_suppression`, `empty_catch`,
     `weakened_test`, `hardcoded_secret`, `injection`, `null_unhandled`, `logic_bug`)
   - `hoodie_flag`: performance and usability patterns (`quadratic`, `io_in_loop`,
     `unbounded_load`, `missing_cache`, `blocking_sync`, `poor_error_message`)
   - `risk`: `trivial` / `standard` / `risky`
3. Appends a block to the end of the reviewer's prompt. Each reviewer sees only its own flags:

   ```
   ## Decision-model pre-screen (a 4B model's hints, not findings — confirm or refute each flag first)
   - flags: empty_catch p=0.41, logic_bug p=0.18
   - risk: standard p=0.72
   ```

   A `depth:` line is added when the model is confident:
   - `trivial` >= 0.80 **and** neither reviewer has a flag: "treat this as a quick consultation"
   - `risky` >= 0.60: "risk flagged — do a full review"

The two reviewers are launched in the same message, so they share one request: the plugin caches
the in-flight promise, keyed by a hash of the state. Both reviewers always run. The pre-screen
only adds text to their prompts.

`neckbeard.md` and `hoodie.md` tell the reviewers how to use the block: confirm or refute each
flag, treat an empty block as proving nothing, and ignore "quick consultation" if they find a
serious issue.

### F2: shell verdict (`execute.after` on `shell`)

Catches commands that exit 0 but did not really work: test runners hidden behind `|| true`,
scripts that print errors and still exit 0, builds that pass with warnings worth reading.

It runs only when **all** of these hold:

- the call completed with **exit 0** (a non-zero exit already says it failed)
- the output is at least 200 characters
- the command does not start with a read-only tool (`cat`, `ls`, `grep`, `rg`, `find`,
  `sed -n`, `jq`, `diff`, `git status|log|diff|show|…`, and others)

It sends the command, the exit code and the last ~16k characters of output (8 s timeout), asks
`pass` / `fail` / `warnings` / `inconclusive`, and appends one line to the tool result:

```
[jev] verdict: pass p=0.90 (decision-model hint, not a review; exit=0)
[jev] verdict: fail p=0.71, pass p=0.24 (decision-model hint, not a review; exit=0)
[jev] verdict: unsure (leans fail) p=0.44 (decision-model hint, not a review; exit=0)
```

The second label is shown when it scores at least 0.20. Below 0.50 the line says `unsure`,
because a real failure hidden behind exit 0 scored 0.55 during calibration. The command's own
output is never changed. `local-mind.md` tells the main agent that the verdict is a hint, never
verification, and that output still goes to both reviewers with the verdict line included.

Known gap: the read-only check looks only at the start of the command, so a command written as
`cd dir && git status` still gets a verdict.

### F3: shell risk escalation (`permission` `evaluate`)

Runs only on shell commands that the config would **allow** without asking. It asks whether the
command is `routine`, `destructive`, `network` or `system` (8 s timeout). If a non-routine label
scores at least 0.70, the permission changes from `allow` to `ask` with a message like:

```
jev flagged this command as destructive (p=0.93); confirm to run it
```

The same request asks a second question, `reversibility`: `read_only`, `reversible` or
`irreversible`, with worked examples in each criterion. It can only **add** escalations. If
`shell_risk` finds nothing but `irreversible` scores at least 0.85, the command asks anyway:

```
jev flagged this command as irreversible (p=0.96); confirm to run it
```

When both questions fire, the `shell_risk` label is reported, so its messages stay the same.
The second question adds about nothing: both answers come back in about 125 ms. Probed against
the 4060 on 2026-11-06 with 28 labelled commands:

- It catches what the category question split: `dd if=/dev/zero of=/dev/sda` (destructive 0.38,
  system 0.59, so neither reached 0.70) and `> ~/.bash_history` (0.59 / 0.38). Both score
  irreversible 0.95–0.96.
- Everything that should ask scored irreversible ≥ 0.92. The routine commands the model judged
  (builds, tests, commits, `mv`, `sed -i`, `git stash`, installs, reads) scored ≤ 0.10.
- It does **not** fix the `rm` problem below: `rm -rf node_modules` scores irreversible 0.89 and
  `rm -rf target/ dist/` 0.94. Using `reversible` to cancel a `destructive` hit was considered
  and dropped for that reason, since it would only weaken the gate.

The model scores almost every `rm` as destructive (`rm -rf target/` 0.94, `rm -rf ~/x` 0.96), so
no threshold separates the two. An `rm` is therefore decided locally, without asking the model,
when every target is a plain relative path inside the working directory: no `/`, `~`, `$`, `..`,
bare `.`, globs, pipes, redirects, `;`, `&` or command substitution. That rm stays `allow`. Any
other rm goes to the model as usual.

F3 can only move `allow` to `ask`. It never touches commands that are already `ask` or `deny`
(the `git reset --hard`, `git clean`, `git push --force` and `git branch -D` rules in
`opencode.jsonc` stay as they are), and it never turns anything into `deny`.

### F4: `ask_jev` tool (`ctx.tool.transform`)

A tool the agent calls to ask **one question about many files without reading them**. Each file is
judged separately, and the agent gets one line per file back: the answer, its probability, and the
line range that decided it. None of the file text enters the agent's context. Use it to scout
(which files are relevant, which contain something, which layer a file is), then read only the
hits, and only the line ranges it names.

```jsonc
// the agent's call
{ "paths": ["llama-proxy/src/**/*.rs"],
  "question": "Does this file use a mutex or read-write lock?" }
```

A real result, shortened (`…` marks cut lines):

```
ask_jev: "Does this file use a mutex or read-write lock?" (yes/no) · 17 files · 398k chars · 113,819 tokens judged in 28.8 s
yes 0.95  llama-proxy/src/augment.rs  (lines 1079-1249)
yes 0.91  llama-proxy/src/backends/mod.rs  (lines 1-197)
…
unsure (leans no) 0.69  llama-proxy/src/backends/balancer.rs
no  <N> files, all p≥0.90: …
not checked (budget; ask again with these paths): llama-proxy/src/exporters/influxdb.rs, … (+13)
jev answers are a 4B classifier's guesses: read a file before editing it, quoting it, or relying on the answer.
```

**Arguments**

| Argument | Meaning |
|---|---|
| `paths` | 1–20 entries: files, directories or globs (`src/**/*.ts`), relative to the project. One glob can cover the whole call (up to 50 files are checked), so there's no reason to batch. A saved tool-output path (see below) also works |
| `question` | one question, 3–300 characters, answered per file |
| `options` | optional `{label: description}` with 2–8 labels, e.g. `{"handler": "defines HTTP handlers", "storage": "reads or writes the database"}`, with a question phrased as a choice ("Which of these best describes this file?"). Without it the question is yes/no |
| `pattern` | optional ripgrep regex, used by the router's `rg` mode when the question is a text search |
| `force` | optional `true`: skip the router and scan. Only if grep came up empty or the user explicitly asked for a *scan*; "use ask_jev" alone means let the router decide. Ignored in `strict` mode |

There is one question per call because each question re-prefills the whole state anyway, so two
questions in one call would cost the same as two calls.

**Router: grep or scan?** Before reading any file, one request (~20–55 ms) asks the decision model
whether a plain text search would answer the question:

| Mode | When the answer is "text search" (p(grep) ≥ 0.80) |
|---|---|
| `advise` (default) | no scan: the tool answers at once that this is a text-search question and the agent should grep for the exact name / call / import / string |
| `rg` | runs `rg --count-matches` for the agent's `pattern` over the same expanded, permission-checked files and lists `N matches  path`. Without a `pattern` it falls back to `advise` |
| `off` | scans as before, so the router never answers |

Below 0.80, with `force: true`, or when the router request fails, the tool scans as usual, so the router
fails open. Switch modes while OpenCode runs with `echo rg > ~/.config/opencode/jev-router`
(`off`, `advise` or `rg`; delete the file to go back to `JEV_ROUTER`, then `advise`).

**`strict`** (`echo "rg strict" > …/jev-router`, or `advise strict`) ignores the agent's `force`, so every
`ask_jev` call goes through the router whatever the prompt says. Use it for A/B runs, where wording such
as "use ask_jev" must not decide the arm. Every result records `metadata.router` (`mode`, `strict`,
`p_grep`, `action`, `ms`, and `force_ignored` when `strict` overrode a `force`), which
`opencode/scripts/session-report.py` lists.

Calibrated 2026-11-06 on 40 questions against the live 6000:

| Group | p(grep) | Routed to grep at 0.80 |
|---|---|---|
| Semantic (17), including the agent's own "Which of these best describes this file's relation to the upstream…" with options | 0.05–0.37 | 0 |
| Grep-shaped (17): "which files call / import / mention / reference X" | 0.89–0.96, except "contain unsafe blocks" 0.50 and "derive Serialize" 0.45 | 15 |
| Borderline (6): "uses a mutex or RwLock" 0.75, "talks to the upstream server" 0.33, … | 0.14–0.75 | 0 |

The router catches questions *phrased* as text matches. A behaviour question with an obvious code
signature ("which files talk to the upstream server") still scans, so `local-mind.md` tells the agent
to turn those into a grep itself. On "Which files call reqwest's send()?" over `llama-proxy/src`, live:
`advise` answered in 56 ms; `rg` listed the 9 files with matches out of 40 in 36 ms; `off` scanned only
17 of the 40 files (budget) in 4.8 s and answered "no" for `augment.rs` and `preflight.rs`, which do
call `.send()`.

**Who can use it.** The tool runs under the `read` permission (`options.permission: "read"`), so
every agent that may read sees it: local-mind, chat, and the reviewers neckbeard and hoodie.
Before reading a file, it asks the same permission check the read tool does
(`action: "read"`, the path relative to the project). So the reviewers' `*.env` denies and any `ask`
rules apply per file. A denied file is listed as `denied by permissions`, and the rest of the call goes on.

**Which paths.**
- Paths resolve against the session's directory, with `~` expanded, the same as the read tool.
- Allowed: inside the directory or the project, plus OpenCode's saved tool output
  (`~/.local/share/opencode/tool-output/`). When a tool result is over 2000 lines or 50 KB,
  OpenCode cuts it short and adds `[showing lines 1-N of M; full output saved to <path>]`. Passing
  that path lets the agent ask "did any test fail?" about a 20k-line log without reading it.
- Anything else is skipped as `outside the project`.
- Globs and directories expand through `rg --files`, with the same exclusions OpenCode's glob tool
  uses: `.gitignore` respected, hidden files and `.git` skipped. A file named explicitly is not filtered.
- At most 50 files per call. Extra matches are counted as `+N more files matched`. Binary files
  (a NUL byte in the first 8 KB) are skipped.

**How a file is judged.**
- The file is split at line breaks into chunks of about 8000 characters, with 20 lines of overlap,
  so a match on a boundary is seen whole. Unlike `cap()`, nothing in the middle is dropped.
- Each chunk is one request, with the question, the path and its line range in the state.
- For yes/no, a "yes" in any chunk counts: `p(yes)` is the maximum over chunks, and the line range
  shown is that chunk's.
- For custom options, the probabilities are averaged, weighted by chunk length. If one part of the
  file leans to a different label at ≥ 0.6, that is noted, e.g. `(lines 212-437 lean proc 0.80)`.
- An answer below 0.70 is shown as `unsure (leans …)`. Confident `no` answers (≥ 0.90) fold into one line.

**Budget and load.**
- Requests go **one at a time per server, across all `ask_jev` calls**: each server has its own
  queue in the plugin, so calls the agent runs in parallel wait for each other on each server. More
  in flight on one GPU would buy nothing (the benchmark shows chunk throughput flat from 1 to 8
  clients) and would take slots from the hooks. A second GPU does add speed, so in `share` mode
  both servers work on a scan at once. The hooks never wait in these queues. Measured on the old
  single-slot 4060 during a 400k-character scan: 10 F3 checks answered in 167–630 ms, all of them,
  and no cooldown was triggered.
- An interrupted call that is still waiting in a queue ends at once, and the queue moves on.
- The call first checks, reads and cuts every file into chunks, then hands the chunks to one worker
  per server (`share`) or to one server with the rest on standby (`split`). A chunk that fails is
  retried once on another server; that server then sits out the rest of the call.
- Per call, at most 400k characters are sent (about 114k tokens, about 29 s). Files past that are
  listed as `not checked (budget; ask again with these paths)`. A file over 200k characters is
  checked from the top and marked `partial: lines 1-N of M`.
- Each request has a 15 s timeout. When no server is left to take the remaining chunks, the tool
  returns what it has plus `stopped early: <reason>`, and files with only some parts answered are
  marked `partial: K of N parts checked`.
- If nothing was answered, the tool errors with `jev is unavailable (<reason>); read the files
  instead`. Its timeouts never pause the hooks (see [Safety](#safety-when-the-server-is-down)).
- An interrupted call aborts its request at once.

**Calibration** (2026-11-06, the live 4060). 32 Rust files from `llama-proxy/src` (773k
characters) and four yes/no questions, with ground truth from grep checked by hand: lock use (14
files), disk I/O (4), HTTP routes (1), spawning a process (1). Each cell is files found out of 20,
and false alarms:

| Chunk size | Wording | at 0.5 | at 0.7 | s / 100k chars |
|---|---|---|---|---|
| **8k** | **A** (shipped) | **19/20, 12 false** | **19/20, 6 false** | 7.4 |
| 8k | B ("a mention in a comment or name doesn't count") | 15/20, 6 false | 13/20, 4 false | 7.4 |
| 12k | A | 17/20, 9 false | 15/20, 5 false | 6.7 |
| 24k | A | 17/20, 8 false | 17/20, 6 false | 7.1 |

Wording B and larger chunks cost recall, and for scouting a missed file costs more than an extra
read. Most false alarms are lookalikes: `tokio::spawn` (an async task) answered as "spawns a
process", and `Path` in comments answered as "disk I/O". Treat a yes as "worth reading", not as a fact.

`local-mind.md` tells the main agent to **grep first** whenever the question can be written as a
pattern (including behaviour with an obvious code signature), to use `ask_jev` for questions no
pattern can express and for long saved outputs, to follow the router's "use grep", to call `ask_jev`
without `force` even when the user asks for it (`force` only when grep came up empty or the user
explicitly asked for a scan), and never to count an answer as
verification. `neckbeard.md` and `hoodie.md` say a finding must come from code they read, and to
grep plain text matches.

## Trace view (request/response debugging)

Off by default. When it is on, every request jev sends and the raw response it gets back are shown
in the TUI as formatted JSON, attached to the tool result they belong to.

Turn it on or off while OpenCode is running. The file is checked on every call, so no restart is
needed:

```sh
# inside the ocli container
touch ~/.config/opencode/jev-trace     # on
rm ~/.config/opencode/jev-trace        # off

# from the host (container_data/config is mounted as ~/.config)
touch opencode/container_data/config/opencode/jev-trace
```

`JEV_TRACE=1` in the `opencode serve` environment also turns it on, but needs a restart.

Each trace looks like this:

````
[jev trace] F2 shell verdict
```json
{
  "request": {
    "state": "A coding agent ran a shell command. ...\n$ make test\nexit code: 0\n\n...",
    "questions": { "verdict": { "type": "choice", "criteria": { "pass": "...", "fail": "..." } } }
  },
  "response": {
    "model": "h2oai/h2o-lightning-4b",
    "answers": { "verdict": { "type": "choice", "choice": "fail",
                              "probabilities": { "pass": 0.079, "fail": 0.921 }, "confidence": 0.842 } },
    "usage": { "input_tokens": 119, "output_tokens": 0 }
  },
  "ms": 41
}
```
````

- `request.state` is cut to 800 characters (head and tail). The full state can be 24k
  characters. `questions` is shown in full, criteria included.
- `response` is the server's body exactly as it came back.
- On failure there is no `response`. Instead `error` gives the reason: the fetch error,
  `HTTP 404: …`, `malformed answer for verdict`, or `skipped: cooling down after a failure (42s left)`.
  This is the quickest way to see why a verdict line is missing.

Where each trace appears:

| Feature | Shown on |
|---|---|
| F1 reviewer pre-screen | the reviewer's **result** (after `execute.after` on that tool call id). It is never added to the reviewer's prompt, so the reviewer's input stays the same as with tracing off |
| F2 shell verdict | the shell call's result, after the `[jev] verdict:` line |
| F3 shell risk | the same shell call's result. The permission event has no tool call id, so the trace waits until a shell call whose command contains the judged text arrives |
| F4 `ask_jev` | the tool's own result: one summary block with the first request's state and criteria, the request count, tokens, and the time of each request |

A trace waiting for its tool result is dropped after 10 minutes, or once 16 others are queued.
This happens for example to an F3 trace whose command you rejected at the `ask` prompt.

Caveats:

- A trace is part of the tool result, so **the main agent reads it too**. That costs tokens: an
  F1 trace with all three questions is about 3k characters. Leave tracing off for normal work.
- F2 traces show up only for calls that get a verdict request at all (exit 0, at least 200
  characters, not read-only). F3 traces show up only for commands the config would `allow`.
- Tracing never changes what jev decides. It only adds text, and it goes through the same
  `guard()` as everything else.

## Safety when the server is down

The plugin is designed to **fail open**. If the decision server is unavailable, OpenCode behaves
exactly as it would with the plugin removed. These mechanisms guarantee it:

**1. Every failure becomes "no answer".** `query()` wraps the request in `try/catch` and never
throws. All of these end with `answers: null`, and for a hook that means "leave the call alone":

| What went wrong | How it shows up |
|---|---|
| Server not running, port closed | connection refused, fails at once |
| `cosmo.lan` does not resolve | DNS error, fails at once |
| Host up but not answering | `AbortSignal.timeout`, 8 s (F2, F3) or 15 s (F1) |
| Stock GGUF loaded, no `/v1/systemone` | non-2xx status, fails at once |
| Server error or overload | non-2xx status, fails at once |
| Garbage or partial JSON | parse error or "malformed answer", fails at once |

**2. One failure pauses that server for 60 seconds, and the plugin fails over.** After a hook
failure, that server's `downUntil` is set to now + 60 s, and requests skip it without touching the
network until then. A dead server therefore costs, at most, **one** timeout per minute. With
connection refused or DNS errors it costs nothing noticeable. The log line says so:

```
[jev] http://cosmo.lan:8710 unavailable (<error message>); skipping it for 60s
```

A hook moves on to the next server only after a *cheap* failure: the server is paused, or can't be
reached at all. A hook that times out or gets a bad answer stops there (fail open), so a hook never
waits for two timeouts.

The `ask_jev` tool (F4) runs in its own lane. It respects a running pause, but it only *starts*
one when the server can't be reached at all (connection refused, DNS). A tool request that times
out or gets a bad answer does not pause the hooks, so a slow scan can never switch off F3's gate.
An interrupted tool call is not counted as a failure.

**3. Hooks never throw.** Every hook body is wrapped in `guard()`, which catches any exception,
logs it as `[jev] <hook> hook error, passing through: …` and returns normally. This matters
most for F1: in OpenCode, a throw from `execute.before` blocks the tool call, which would stop a
reviewer from launching. A bug in the plugin's own code (for example an empty `probabilities`
object) is caught the same way.

**4. Mutations happen last and all at once.** Each hook builds its change completely and assigns
it in a single step at the end (`e.input = …`, `e.result = …`, `e.effect = …`). A failure
anywhere before that leaves the event untouched. Nothing is half-applied.

**5. What each feature does when it gets no answer:**

| Feature | With no answer |
|---|---|
| F1 pre-screen | reviewer prompt is sent unchanged; both reviewers run as normal |
| F2 verdict | tool result is unchanged; no `[jev]` line |
| F3 escalation | permission stays as the config decided (`allow` stays `allow`) |
| F4 `ask_jev` | the tool errors with `jev is unavailable (<reason>); read the files instead`, so the agent reads the files the normal way. If some files were already answered, it returns those plus `stopped early: <reason>` |

**6. Failures are not cached.** The F1 cache drops a `null` result, so the next reviewer launch
after the cooldown tries again instead of reusing the failure.

**7. Startup does not depend on the servers.** `setup()` only registers hooks and logs the
servers and mode (`[jev] decision servers, share mode: hooks -> … ; ask_jev -> …`). It never
contacts a server, so OpenCode starts normally whether or not either GPU's server is up.

What you lose while **both** servers are down: F3's extra prompts. Shell permissions fall back to
the plain rules in `opencode.jsonc`, which are the only guard when jev is not running. jev is
an extra layer on top of those rules, not a replacement for them. With one server down, the other
takes everything.

The worst cost of an unreachable host (one that swallows packets instead of refusing them) is
latency: one shell command, permission check or reviewer launch waits up to 8 s or 15 s, then
the plugin pauses for a minute. A busy 4060 can also hit the timeout on a large review prefill.
That just means one missed hint and a 60 s pause, nothing more.

## Configuration

| Env var | Effect |
|---|---|
| `JEV_URLS` | Decision servers, preferred first, comma-separated (default `http://cosmo.lan:8711,http://cosmo.lan:8710`: the 4060, then the 6000; trailing `/` is stripped) |
| `JEV_MODE` | `share` (default: hooks on the first server that is up, scans on all) or `split` (hooks on the last server, scans on the first) |
| `JEV_HOOK_URL` / `JEV_TOOL_URL` | Override which server split mode uses for hooks / for `ask_jev` |
| `JEV_URL` | A single server, used only when `JEV_URLS` is unset |
| `JEV_DISABLE=1` | Registers no hooks and no `ask_jev` tool; logs `JEV_DISABLE=1, not registering hooks` |
| `JEV_TRACE=1` | Trace view always on (same as the `jev-trace` file existing) |
| `JEV_ROUTER` | `ask_jev` router mode when the toggle file is absent: `advise` (default), `rg` or `off` |
| `$XDG_CONFIG_HOME/opencode/jev-router` | Router setting while this file exists: `off`, `advise` or `rg`, optionally followed by `strict` (ignore the agent's `force`). Read on every call |
| `$XDG_CONFIG_HOME/opencode/jev-trace` | Trace view on while this file exists (default `~/.config/opencode/jev-trace`) |

Tunables live at the top of `jev.ts` and were set from a calibration run:

| Constant | Value | Meaning |
|---|---|---|
| `TIMEOUT_MS` | 8000 | F2 and F3 request timeout |
| `REVIEW_TIMEOUT_MS` | 15000 | F1 timeout (3 questions over up to ~6k tokens is ~4.5 s on an idle 4060) |
| `COOLDOWN_MS` | 60000 | pause after any failure |
| `STATE_MAX_CHARS` | 24000 | ~6k tokens of state; head and tail are kept |
| `OUTPUT_TAIL_CHARS` | 16000 | ~4k tokens of command output for F2 |
| `MIN_OUTPUT_CHARS` | 200 | shorter shell output gets no verdict |
| `FLAG_MIN` | 0.15 | lowest probability listed as a reviewer flag |
| `TRIVIAL_MIN` | 0.80 | "quick consultation" threshold |
| `RISKY_MIN` | 0.60 | "full review" threshold |
| `VERDICT_MIN` | 0.50 | below this the verdict says `unsure` |
| `RUNNER_UP_MIN` | 0.20 | show the second verdict label at or above this |
| `ESCALATE_MIN` | 0.70 | shell `allow` -> `ask` threshold |
| `IRREVERSIBLE_MIN` | 0.85 | shell `allow` -> `ask` on an `irreversible` answer alone |
| `TRACE_STATE_CHARS` | 800 | state shown in a trace (head + tail) |
| `TRACE_KEEP` | 16 | F1/F3 traces waiting for their tool result |
| `TRACE_MAX_AGE_MS` | 600000 | a waiting trace older than this is dropped |
| `ASK_CHUNK_CHARS` | 8000 | `ask_jev`: characters per request (~2k tokens, ~0.6 s) |
| `ASK_OVERLAP_LINES` | 20 | lines shared by neighbouring chunks |
| `ASK_MAX_FILES` | 50 | files per call |
| `ASK_FILE_MAX_CHARS` | 200000 | a bigger file is checked from the top and marked partial |
| `ASK_BUDGET_CHARS` | 400000 | characters sent per call (~29 s) |
| `ASK_TIMEOUT_MS` | 15000 | per request |
| `ASK_UNSURE_BELOW` | 0.70 | a winning answer below this is shown as `unsure` |
| `ASK_NO_COLLAPSE` | 0.90 | confident `no` answers fold into one line |
| `ASK_LEAN_MIN` | 0.60 | custom options: note a part that leans to another label |
| `ASK_MAX_LINES` | 60 | lines in the tool result |
| `ASK_ROUTER_GREP_MIN` | 0.80 | p(grep) at or above which the router answers instead of scanning |
| `ASK_ROUTER_TIMEOUT_MS` | 3000 | router request timeout (a timeout means a scan) |

`jev.ts` also exports `__test` (the client, state builders, question sets and the `ask_jev`
pieces, including `askFiles`) for calibration scripts. The plugin loader reads only `default`.

## Checking it

- On startup, the OpenCode log should show
  `[jev] decision servers, share mode: hooks -> http://cosmo.lan:8711 then http://cosmo.lan:8710; ask_jev -> http://cosmo.lan:8711 + http://cosmo.lan:8710`.
- To probe a server by hand from inside the LAN (8710 is the 6000, 8711 the 4060):

  ```sh
  curl -s http://cosmo.lan:8710/v1/systemone -H 'content-type: application/json' -d '{
    "state": "A coding agent ran a shell command.\n\n$ make test\nexit code: 0\n\n3 passed, 1 FAILED",
    "questions": {"verdict": {"type": "choice", "criteria": {
      "pass": "the command succeeded and its output shows no errors or failures",
      "fail": "the command failed: errors, failing tests, crashes or panics"}}}}'
  ```

- To confirm failover, stop one server: hooks and scans carry on against the other, and the log
  shows one `… unavailable … skipping it for 60s` line per minute for the stopped one. The trace
  view's `server` field shows which server answered.
- To confirm the plugin fails open, stop both and run a shell command that prints more than 200
  characters. It should come back with no `[jev]` line. With the trace view on, the same call
  shows a trace with the `error` instead.

## Other plugins: `/stop-all-jobs` (`stop-jobs.ts`)

A separate plugin, not part of jev, that adds one command to the TUI:

```text
/stop-all-jobs         stop every running session and background subagent
/stop-all-jobs list    dry run: show what would be stopped, stop nothing
```

**Why.** A prompt that fans out to many background subagents (9 at once on 2026-11-06, each with
a 70-150k-token context) can swamp basalt's 3 slots. Restarting OpenCode does not help: a running
session keeps `session_v2.time_suspended` set, a background subagent keeps a `job` row with
`status = 'running'`, and `opencode serve` resumes both at startup.

**What it does.**

1. It reads the candidates **read-only** from `opencode.db`: every session with `time_suspended`
   set, plus the parent and child session of every running job. The session you type the
   command in is left out. The command never writes to the database.
2. It stops each one with `ctx.session.interrupt` (OpenCode's own "interrupt active execution
   owned by this process"), subagents first, then parents.
3. An interrupted subagent's job completes and notifies its parent, which can wake the parent, so
   it re-reads the database 1.5 s later and repeats. It stops when a pass stops nothing, or after
   5 passes.
4. It reports in the session you typed it in, as a message that does not start a model turn:

   ```text
   /stop-all-jobs: stopped 4 sessions (3 subagents) in 2 passes: "Scan part 1", "Scan part 2", "Scan part 3", "Rust crates bug scan"
   ```

**Limits.**

- `interrupt` only reaches work owned by the running `opencode serve`. A session that is still
  marked but isn't running there, such as a stale flag left by a crash, comes back as
  `interrupted: false`. It is listed as "Still marked but not running in this OpenCode process"
  and would resume at the next start. Clear it by hand (below).
- Stopping a session does not undo what it did. It only ends the current turn.
- `STOP_JOBS_DB` overrides the database path (default `$XDG_DATA_HOME/opencode/opencode.db`,
  i.e. `~/.local/share/opencode/opencode.db` in the container,
  `container_data/local/share/opencode/opencode.db` on the host).

**Manual cleanup (the fallback).** Do this on the host, **with the opencode container stopped**.
It backs the database up first, then clears the resume flags and the running jobs. Sessions and
their history are kept.

```sh
cd opencode/container_data/local/share/opencode
docker ps --format '{{.Names}}' | grep -qx opencode && echo "stop the opencode container first" || {
  B=~/opencode-db-backup-$(date +%Y%m%d-%H%M%S); mkdir -p "$B" && cp -p opencode.db* "$B"/ && echo "backup: $B"
  python3 -I -c '
import sqlite3
c = sqlite3.connect("opencode.db")
s = c.execute("update session_v2 set time_suspended = null where time_suspended is not null").rowcount
j = c.execute("delete from job where status = '"'"'running'"'"'").rowcount
c.commit(); c.execute("pragma wal_checkpoint(TRUNCATE)")
print(f"cleared {s} suspended sessions, removed {j} running jobs; integrity:", c.execute("pragma integrity_check").fetchone()[0])'
}
```

`stop-jobs.ts` exports `__test` (`readCandidates`, `stopAll`, `dbPath`) for test scripts.
