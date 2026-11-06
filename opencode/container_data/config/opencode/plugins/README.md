# jev: decision-model hints for OpenCode

`jev.ts` is an OpenCode plugin that hands small classification jobs to a local 4B decision
model: H2O Lightning 4B on the RTX 4060, started by `server/llamacpp/run-jev-4060`. The plugin
never asks the model to write text. It sends a block of text (the *state*) and a multiple-choice
question, and the model returns a probability for each option. One answer costs one prefill and
no decoding, so it is fast and cheap.

The plugin only ever **adds** information or **tightens** a permission. It never removes a
reviewer, never edits command output, never blocks a tool call, and never loosens a permission.
If the decision server is down, slow or returns nonsense, every hook leaves its call exactly as
it would have been without the plugin. See [Safety when the server is down](#safety-when-the-server-is-down).

## The decision server

| | |
|---|---|
| Launcher | `server/llamacpp/run-jev-4060` (RTX 4060, `device=1`) |
| Endpoint | `POST $JEV_URL/v1/systemone` (default `http://cosmo.lan:8701`) |
| Model file | `h2o-lightning-4b-Q8_0.gguf`; the stock GGUF has no decision API, so every call fails and the plugin stays out of the way |
| llama.cpp | build >= 11773 for `/v1/systemone`; probed against b12169 |

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

**1. Every failure becomes "no answer".** `ask()` wraps the request in `try/catch`. All of these
end with `null`, and `null` means "leave the call alone":

| What went wrong | How it shows up |
|---|---|
| Server not running, port closed | connection refused, fails at once |
| `cosmo.lan` does not resolve | DNS error, fails at once |
| Host up but not answering | `AbortSignal.timeout`, 8 s (F2, F3) or 15 s (F1) |
| Stock GGUF loaded, no `/v1/systemone` | non-2xx status, fails at once |
| Server error or overload | non-2xx status, fails at once |
| Garbage or partial JSON | parse error or "malformed answer", fails at once |

**2. One failure pauses the plugin for 60 seconds.** After any failure, `downUntil` is set to
now + 60 s, and every `ask()` from any feature returns `null` without touching the network until
then. A dead server therefore costs, at most, **one** timeout per minute. With connection
refused or DNS errors it costs nothing noticeable. The log line says so:

```
[jev] unavailable (<error message>); skipping for 60s
```

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

**6. Failures are not cached.** The F1 cache drops a `null` result, so the next reviewer launch
after the cooldown tries again instead of reusing the failure.

**7. Startup does not depend on the server.** `setup()` only registers hooks and logs
`[jev] decision model at <url>`. It never contacts the server, so OpenCode starts normally
whether or not the 4060 is up.

What you do lose while the server is down: F3's extra prompts. Shell permissions fall back to
the plain rules in `opencode.jsonc`, which are the only guard when jev is not running. jev is
an extra layer on top of those rules, not a replacement for them.

The worst cost of an unreachable host (one that swallows packets instead of refusing them) is
latency: one shell command, permission check or reviewer launch waits up to 8 s or 15 s, then
the plugin pauses for a minute. A busy 4060 can also hit the timeout on a large review prefill.
That just means one missed hint and a 60 s pause, nothing more.

## Configuration

| Env var | Effect |
|---|---|
| `JEV_URL` | Decision server base URL (default `http://cosmo.lan:8701`; trailing `/` is stripped) |
| `JEV_DISABLE=1` | Registers no hooks at all; logs `JEV_DISABLE=1, not registering hooks` |
| `JEV_TRACE=1` | Trace view always on (same as the `jev-trace` file existing) |
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

`jev.ts` also exports `__test` (the client, state builders and question sets) for calibration
scripts. The plugin loader reads only `default`.

## Checking it

- On startup, the OpenCode log should show `[jev] decision model at http://cosmo.lan:8701`.
- To probe the server by hand from inside the LAN:

  ```sh
  curl -s http://cosmo.lan:8701/v1/systemone -H 'content-type: application/json' -d '{
    "state": "A coding agent ran a shell command.\n\n$ make test\nexit code: 0\n\n3 passed, 1 FAILED",
    "questions": {"verdict": {"type": "choice", "criteria": {
      "pass": "the command succeeded and its output shows no errors or failures",
      "fail": "the command failed: errors, failing tests, crashes or panics"}}}}'
  ```

- To confirm the plugin fails open, stop the 4060 container and run a shell command that prints
  more than 200 characters. It should come back with no `[jev]` line, and the log should show one
  `unavailable … skipping for 60s` line. With the trace view on, the same call shows a trace with
  the `error` instead.
