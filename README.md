# local-mind

A containerized multi-agent AI development environment that enforces mandatory peer review for all code changes.
Uses local LLMs built on [OpenCode](https://opencode.ai) and [oh-my-opencode](https://github.com/travisennis/oh-my-opencode).

## Overview

local-mind implements a three-agent architecture where no code can be written without parallel approval from two reviewer agents:

| Agent | Role | Focus |
|-------|------|-------|
| **local-mind** | Primary developer | Proposes code, coordinates reviews, writes files |
| **neckbeard** | Senior reviewer | Bugs, security, code quality, pattern consistency |
| **hoodie** | Junior reviewer | Performance, usability, UX, creative suggestions |

Reviewers have read-only access (grep, glob, read) and cannot modify files. The primary agent is forbidden from writing code without sign-off from both reviewers.

## Architecture

```
┌─────────────────────────────────────────────┐
│  Docker Container (Fedora)                  │
│                                             │
│  ┌───────────┐   ┌──────────┐               │
│  │local-mind │──>│neckbeard │ (parallel)    │
│  │ (primary) │──>│  hoodie  │               │
│  └─────┬─────┘   └──────────┘               │
│        │ write only after both approve      │
│        v                                    │
│   [codebase]                                │
│                                             │
│  OpenCode + oh-my-opencode                  │
└──────────────┬──────────────────────────────┘
               │ OpenAI-compatible API
               v
┌──────────────────────────────────────────────┐
│  llama-proxy (cosmo.lan:8799)                │
│  Rust reverse proxy with response fixing,    │
│  metrics, and multi-backend load balancing   │
└──────────────┬───────────────────────────────┘
               │
               v
┌──────────────────────────────────────────────┐
│  Local LLM Servers (cosmo.lan)               │
│  ┌────────────┐  ┌────────────┐              │
│  │ cosmo-6000 │  │ cosmo-4060 │              │
│  │ :8700      │  │ :8701      │              │
│  └────────────┘  └────────────┘              │
│  ikllama / llama.cpp + CUDA                  │
└──────────────────────────────────────────────┘
```

## Project Structure

```
.
├── bin/                        # Entry point scripts
│   ├── opencode                # Launch the dev container
│   ├── build-container-opencode
│   ├── model-holder            # Lock model files in memory
│   ├── model-switcher          # Switch agent models
│   ├── ocm                     # OpenCode model management
│   └── count-subagent-calls    # Count subagent usage
│
├── llama-proxy/                # Rust HTTP reverse proxy
│   ├── src/                    # Proxy source code
│   ├── config.yaml.default     # Config template
│   └── README.md               # Full proxy documentation
│
├── opencode/                   # Main dev environment
│   ├── Dockerfile              # Fedora-based container
│   ├── container_data/
│   │   └── config/opencode/
│   │       ├── opencode.jsonc  # Provider & permission config
│   │       ├── oh-my-opencode.json
│   │       └── agent/          # Agent definitions
│   │           ├── local-mind.md
│   │           ├── neckbeard.md
│   │           └── hoodie.md
│   ├── helpers/
│   │   ├── model-holder/       # Rust: mmap model files to prevent cache thrashing
│   │   └── model-switcher/     # Rust: TUI for switching models per agent
│   ├── vendor/                 # Read-only reference: OpenCode & oh-my-opencode source
│   └── plans/                  # Planning documents
│
└── server/
    ├── proxy -> ../llama-proxy  # Symlink to llama-proxy
    ├── llamacpp/                # llama.cpp LLM server infrastructure
    │   ├── Dockerfile           # CUDA build
    │   ├── run-6000             # Start cosmo-6000 server (:8700, RTX 3070)
    │   ├── run-4060             # Start cosmo-4060 server (:8701, RTX 4060)
    │   ├── run-both             # Start with both GPUs
    │   ├── run-common           # Shared startup logic
    │   └── vendor/              # llama.cpp source
    ├── ikllama/                 # ik_llama.cpp server (alternative backend)
    │   ├── Dockerfile
    │   ├── run-6000
    │   ├── run-4060
    │   └── vendor/
    ├── vllm/                    # vLLM server infrastructure
    ├── sglang/                  # SGLang server infrastructure
    └── trtllm/                  # TensorRT-LLM infrastructure
```

## Prerequisites

- Docker
- NVIDIA GPU(s) with CUDA 12.4+ (for LLM servers)
- Network access to LLM server hosts (or run locally)

## Quick Start

### 1. Build and Start LLM Servers

```bash
cd server/llamacpp       # or server/ikllama
./run-both               # Starts cosmo-6000 on :8700, cosmo-4060 on :8701
```

Or individually:

```bash
./run-6000               # RTX 3070 (GPU 0) - port 8700
./run-4060               # RTX 4060 - port 8701
```

### 2. Start llama-proxy

```bash
cd llama-proxy
cp config.yaml.default config.yaml
# Edit config.yaml to set backend URL(s)
cargo run --release -- run --config config.yaml
```

The proxy listens on `:8799` by default and routes to the backend LLM servers.

### 3. Launch the Dev Container

```bash
bin/opencode
```

This builds the Docker image (if needed) and drops you into a Fedora container with OpenCode, oh-my-opencode, and all development tools pre-installed. Your `~/prog` directory is mounted read-write.

### 4. Start OpenCode Inside the Container

```bash
opencode
```

The `local-mind` agent is configured as the default. It will automatically coordinate with `neckbeard` and `hoodie` for all code changes.

## LLM Providers

All providers use OpenAI-compatible APIs:

| Provider | Endpoint | GPU | Notes |
|----------|----------|-----|-------|
| cosmo-00 | `http://cosmo.lan:8700/v1` | RTX 3070 | Direct backend |
| cosmo-01 | `http://cosmo.lan:8701/v1` | RTX 4060 | Direct backend |
| cosmo-proxy | `http://cosmo.lan:8799/v1` | Load-balanced | Via llama-proxy |
| runpod | RunPod proxy URL | Cloud | Remote fallback |

## llama-proxy

The `llama-proxy/` directory contains a Rust reverse proxy that sits between OpenCode and the LLM backends. Features:

- **Response fixing**: Repairs malformed tool calls and JSON from LLM outputs
- **Multi-backend routing**: Route requests to different backends by model name
- **Load balancing**: `round_robin` or `priority_free` strategies
- **Metrics**: Tokens/sec, context usage, request timing
- **InfluxDB export**: Remote telemetry for monitoring

See `llama-proxy/README.md` for full documentation.

## Helper Tools

### model-switcher

Interactive TUI for switching which LLM model each agent uses:

```bash
cd opencode/helpers/model-switcher
cargo run --release lm        # Switch local-mind's model
cargo run --release omo       # Switch oh-my-opencode models
cargo run --release all       # Switch all agents at once
```

### model-holder

Locks model files into memory via mmap to prevent cache thrashing during inference:

```bash
bin/model-holder --glob "/path/to/models/*.gguf"
```

## Review Workflow

The enforced workflow for every code change:

1. **Draft** - `local-mind` proposes code (does not write to disk)
2. **Review** - Both `neckbeard` and `hoodie` are called in parallel via `task` tool
3. **Summarize** - `local-mind` presents reviewer feedback to the user
4. **Write** - Only after both reviewers respond, code is written to disk

Reviews are also mandatory after running commands to verify changes -- `local-mind` cannot declare success without reviewer confirmation of the output.

## Tech Stack

- **Container**: Fedora with development tools, PHP/Composer, Node.js/Bun, Rust
- **AI Framework**: OpenCode + oh-my-opencode plugin system
- **LLM Inference**: ikllama/llama.cpp, vLLM, SGLang, or TensorRT-LLM with CUDA GPU acceleration
- **Proxy**: llama-proxy (Rust) with response fixing, load balancing, and metrics
- **Helper Tools**: Rust (clap, crossterm, memmap2, serde)
- **Agent Definitions**: Markdown with YAML frontmatter
