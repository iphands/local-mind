# Local Mind Agents

Three agents implement a mandatory-peer-review workflow: `local-mind` proposes and writes,
`neckbeard` and `hoodie` review. Reviewers are read-only and cannot modify files.

## Files

```
container_data/config/opencode/
├── opencode.jsonc      # providers, default_agent, permissions
├── oh-my-openagent.json # per-agent model overrides for the plugin's built-in agents
└── agent/
    ├── local-mind.md   # primary — proposes, coordinates review, writes
    ├── neckbeard.md    # senior reviewer — correctness, bugs, security, patterns
    └── hoodie.md       # junior reviewer — performance, usability, UX
```

OpenCode loads `agent/*.md` from its config dir. The container mounts
`container_data/config/` as `~/.config/`, so these land at `~/.config/opencode/agent/`.

## Agents

| Agent | Mode | Model | Tools |
|-------|------|-------|-------|
| `local-mind` | primary | `cosmo-proxy/cosmo-proxy` | `"*": true` |
| `neckbeard` | subagent | `cosmo-proxy/cosmo-proxy` | `read`, `grep`, `glob` only |
| `hoodie` | subagent | `cosmo-proxy/cosmo-proxy` | `read`, `grep`, `glob` only |

All three route through `cosmo-proxy` (`http://cosmo.lan:8799/v1`), the Rust reverse proxy in
`llama-proxy/`, which load-balances to the llama.cpp backends and applies response fixes.

Subagents (`mode: subagent`) do not appear in the agent switcher; they are reachable only via
the `task` tool.

## The workflow

1. `local-mind` drafts the change and shows it — without writing it
2. It calls `task` twice **in one message**, `subagent_type="neckbeard"` and
   `subagent_type="hoodie"`, both with `run_in_background=true`, then joins each review with
   `background_output(task_id, block=true, timeout=600000)`
3. It waits for both joins, then states what each said and what it decided
4. Only then does it use Write/Edit

## Enforcement is prompt-based

There is no hook or interceptor gating the Write tool. The workflow holds because the agent
prompts state it explicitly, the reviewers physically cannot write (tool restrictions), and the
review step is a tracked todo item.

This is worth being honest about: prompt-based enforcement is only as reliable as the model
following it, and it interacts with the rest of the stack. The failure modes we have actually hit:

- **Synchronous reviews serialize.** With `run_in_background=false` each `task` call blocks until
  its child session idles, so the two reviews rarely overlap and the review step costs roughly the
  sum of both. `run_in_background=true` returns a launch receipt (`Background task launched.` and a
  `bg_...` ID) immediately, so both reviewers run concurrently and the caller joins with
  `background_output(task_id, block=true, timeout=600000)`. The receipt is not a review — the agent
  prompts say that explicitly.
- **The proxy's reprompt engine** used to fire on a reviewer's finished answer (`finish_reason:
  stop`, no tool calls looks identical to a premature stop) and could replace it with a
  follow-up tool call, leaving the caller with a session of tool calls and no text — the
  "subagent returned empty" symptom. Fixed in `llama-proxy/src/proxy/reprompt.rs`:
  `skip_read_only_requests` skips agents that expose no mutating tools, and follow-up turns are
  now merged into the stopped turn instead of replacing it.
- **Completion wakes double-fire.** Once every task is `completed`, the plugin's final wake is coded
  to demand a reply, and a late second completion can merge into it and clear the suppression marker
  — so the wake lands again *after* `local-mind` has already joined its reviewers and reported. The
  agent reads the wake's own "call `background_output` for each task ID" instruction as new work and
  re-posts the whole review. Unlike the two above this has no code fix on our side — the wake text is
  generated upstream — so the mitigation is prompt-only: `local-mind.md` "Stale completion wake" makes
  the decision hinge on the Task IDs rather than the instruction, and pins the reply to one sentinel
  line. Observed on this install: the turn after a duplicate wake makes **no** tool call and spawns
  **no** child session — it is inert. It is *not* eliminated: the plugin injects the wake as a
  user-role message, so one turn is the ceiling for any prompt-only fix. Upstream candidates for the
  double-fire, read from the vendored snapshot and **not reproduced** — `isRedundantParentWake()` in
  `opencode/vendor/oh-my-opencode/packages/omo-opencode/src/features/background-agent/parent-wake-dedupe.ts`
  suppresses a wake only when prompt context, reply mode **and** notification coverage all hold, so
  any one failing lets it through. Do not verify with `bin/count-subagent-calls`: it greps a JSON
  export format this install abandoned in Feb 2026.

## Adding a reviewer

1. Add `agent/<name>.md` with `mode: subagent`, `model: cosmo-proxy/cosmo-proxy`, and
   `tools: {"*": false, "read": true, "grep": true, "glob": true}`
2. Add the call to THE REVIEW PROTOCOL section of `local-mind.md`
3. Give it the same "Delivering Your Answer" rules as `neckbeard.md` — final message is plain
   text, never end on a tool call

## Troubleshooting

**Agent doesn't load** — check the file is in `agent/*.md`, that the frontmatter has `name`,
`mode`, and `model`, and that the YAML parses. Model must be `provider/model` and the provider
must exist in `opencode.jsonc`.

**Reviews aren't happening** — confirm `default_agent` is `local-mind` in `opencode.jsonc`, and
that the `task` tool is available to the primary.

**Reviewer returns empty** — see "Enforcement is prompt-based" above. Check the proxy log for
`Reprompt triggered`; a reviewer request should log `Reprompt skipped` instead.

**Agent re-posts a full review after already reporting** — stale completion wake; see the third
failure mode above. Verify against the live SQLite store, not `bin/count-subagent-calls` (dead since
Feb 2026 — it greps a JSON export format this install no longer writes). Needs the `sqlite` CLI, which
`.dnf-deps` does not list (only `sqlite-devel`), so `dnf install -y sqlite` in the container first.

Find the session id:

```bash
sqlite3 -readonly ~/.local/share/opencode/opencode.db \
  "select id, datetime(time_created/1000,'unixepoch','localtime') from session
   order by time_created desc limit 5;"
```

Then the post-wake turns:

```bash
sqlite3 -readonly ~/.local/share/opencode/opencode.db <<'SQL'
select m.id,
  (select group_concat(json_extract(q.data,'$.tool'),',') from part q
     where q.message_id=m.id and json_extract(q.data,'$.type')='tool') as tools,
  (select coalesce(sum(length(json_extract(q.data,'$.text'))),0) from part q
     where q.message_id=m.id and json_extract(q.data,'$.type')='text') as chars
from message m
where m.session_id='<ses_id>' and json_extract(m.data,'$.role')='assistant'
order by m.time_created desc limit 8;
SQL
```

The turn after the wake must show `tools` empty and `chars` under ~70 — the pinned `STALE-WAKE:` line
is 57 characters. `tools` containing `task` or `background_output` means it re-ran or re-joined
reviewers, so it is still firing; `chars` in the thousands means it re-posted the report. If the
database won't open, query it after the session goes idle — a read-only open of a WAL database still
needs a readable `-shm`.

**Reviews are slow** — send snippets rather than whole files, and cap the reviewers' tool budget
(both prompts currently suggest ~5 calls).

## References

- [OpenCode Agents](https://opencode.ai/docs/agents)
- [oh-my-opencode](https://github.com/code-yeongyu/oh-my-opencode) — vendored read-only at
  `opencode/vendor/oh-my-opencode/`
- OpenCode source, vendored read-only at `opencode/vendor/opencode/`
