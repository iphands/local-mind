# OpenCode Container Setup

Containerized OpenCode development environment with multi-agent delegation using local LLMs.

## Project Structure

```
.
├── bin/                   # Where we put helper scripts
├── container_data/config/
│   └── opencode/          # OPENCODE_CONFIG_DIR (mounted as ~/.config/opencode)
│       ├── opencode.jsonc # Main config with providers and default_agent
│       └── agent/         # Where custom agent definitions go
├── container_data/cache/  # Mounted as ~/.cache
├── container_data/local/  # Mounted as ~/.local
├── prompts/               # Prompt templates
├── plans/                 # Planning documents
├── vendor/                # READ ONLY codebases you can use to understand opencode / oh-my-opencode (sorce code and docs!!)
└── Dockerfile             # Fedora-based container with dev tools
```

## Understanding

You can dig through the code AND documentation in the folders in vendor/**
Other than learning there thoug these directories are read only DO NOT WRITE or RUN things in vendor/

## Running

Dont run opencode... I will do this. If you need something tested just ask

## Agent Architecture

The `local-mind` agent is the primary agent that MUST delegate to both `neckbeard` and `hoodie` subagents before writing any files:

1. `local-mind` (primary) - Proposes code changes
2. Calls `task` tool with `subagent_type="neckbeard"` for senior review
3. Calls `task` tool with `subagent_type="hoodie"` for junior review
4. Synthesizes feedback, then writes files

Both review calls must use `run_in_background=false`. With `true`, oh-my-opencode's task tool
returns only `Background task launched.` and a task ID — no review content — and the caller must
wait for a completion notification and fetch it with `background_output`.

## Local LLM Providers

All three agents use `cosmo-proxy/cosmo-proxy`. The other providers stay configured in
`opencode.jsonc` for manual switching.

| Provider | Endpoint | Model(s) |
|----------|----------|----------|
| cosmo-proxy | http://cosmo.lan:8799/v1 | cosmo-proxy, cosmo-heavy, cosmo-light |
| cosmo-00 | http://cosmo.lan:8700/v1 | cosmo-6000 |
| cosmo-01 | http://cosmo.lan:8701/v1 | cosmo-4060 |

`cosmo-proxy` is the Rust reverse proxy in `../llama-proxy/`. It load-balances to the llama.cpp
backends, applies tool-call fixes, and runs the reprompt engine. Its behaviour affects agent
behaviour — see `../llama-proxy/README.md`.

## Config Location

The container mounts `container_data/config/` as `~/.config/`, so OpenCode finds config at:
- `~/.config/opencode/opencode.jsonc`
- `~/.config/opencode/agent/*.md`

## Key Files

- `container_data/config/opencode/opencode.jsonc` - Provider config, default_agent setting
- `container_data/config/opencode/agent/local-mind.md` - Primary agent with delegation rules
- `container_data/config/opencode/agent/neckbeard.md` - Senior reviewer (read-only: read/grep/glob)
- `container_data/config/opencode/agent/hoodie.md` - Junior reviewer (read-only: read/grep/glob)
- `container_data/config/opencode/agent/README.md` - Agent architecture and troubleshooting
