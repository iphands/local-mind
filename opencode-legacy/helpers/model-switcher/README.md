# Model Switcher

A simple TUI application to switch models in your OpenCode agent configuration.

## Usage

```bash
cd helpers/model-switcher
cargo run --release [COMMAND]
```

## Commands

- `lm` or `local-mind` - Interactive agent model selection (default)
- `omo` or `oh-my-opencode` - Replace all models in oh-my-opencode.json with a chosen model
- `all` - Change all agents (system default + agents + omo) to same model
- `restore-omo-defaults` - Restore oh-my-opencode.json from backup
- `help` - Show help message

## Features

- Interactive TUI interface with arrow key navigation
- Real-time filtering with substring matching
- Shows current model for each agent
- Updates `container_data/config/opencode/agent/*.md` files in-place
- Subcommand-style CLI interface

## Controls

- **↑/↓** - Navigate between models
- **Type** - Filter models by substring
- **ESC** - Clear filter / Skip agent
- **Ctrl+C** - Exit without changes
- **ENTER** - Select highlighted model

## Supported Agents

- local-mind
- hoodie
- neckbeard

## Models

Reads available models from `data/models` file in the project root.
