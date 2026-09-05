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
   `subagent_type="hoodie"`, both with `run_in_background=false`
3. It waits for both, then states what each said and what it decided
4. Only then does it use Write/Edit

## Enforcement is prompt-based

There is no hook or interceptor gating the Write tool. The workflow holds because the agent
prompts state it explicitly, the reviewers physically cannot write (tool restrictions), and the
review step is a tracked todo item.

This is worth being honest about: prompt-based enforcement is only as reliable as the model
following it, and it interacts with the rest of the stack. Two failure modes we have actually hit:

- **`run_in_background=true`** returns `Background task launched.` and a task ID — no review
  content. The agent must call reviewers synchronously (`run_in_background=false`), or wait for
  the completion notification and fetch with `background_output`. The agent prompts now mandate
  the synchronous form.
- **The proxy's reprompt engine** used to fire on a reviewer's finished answer (`finish_reason:
  stop`, no tool calls looks identical to a premature stop) and could replace it with a
  follow-up tool call, leaving the caller with a session of tool calls and no text — the
  "subagent returned empty" symptom. Fixed in `llama-proxy/src/proxy/reprompt.rs`:
  `skip_read_only_requests` skips agents that expose no mutating tools, and follow-up turns are
  now merged into the stopped turn instead of replacing it.

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

**Reviews are slow** — send snippets rather than whole files, and cap the reviewers' tool budget
(both prompts currently suggest ~5 calls).

## References

- [OpenCode Agents](https://opencode.ai/docs/agents)
- [oh-my-opencode](https://github.com/code-yeongyu/oh-my-opencode) — vendored read-only at
  `opencode/vendor/oh-my-opencode/`
- OpenCode source, vendored read-only at `opencode/vendor/opencode/`
