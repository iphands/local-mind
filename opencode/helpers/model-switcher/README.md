# Model Switcher

Switches models and model size info in the OpenCode 2 config. Paths are relative to `opencode/`:

- `container_data/config/opencode/opencode.jsonc`: the top-level `"model"`, plus `limit` and
  `capabilities` on each model under `"providers"`
- `container_data/config/opencode/agents/*.md`: the `model:` line in each agent's frontmatter

## Usage

```bash
bin/model-switcher [COMMAND]     # from the repo root (or anywhere): builds, then runs from opencode/
```

## Commands

- `lm` / `local-mind` (default): interactive. Pick the system default model, then each agent's model, then confirm
- `all`: interactive. Set the system default and every agent to one model
- `status`: show the default and agent models, and each model's context/input/output limits and image input
- `ctx <SIZE> <PRESET>`: set `limit.context` = SIZE and `limit.input` = SIZE × preset on every model
  (`safe` 76%, `balanced` 85%, `aggressive` 92%). You can add `image on|off` after it
- `ctx off`: remove `limit.input` from every model (`limit.context` stays)
- `image [on|off]`: add or remove `"image"` in every model's `capabilities.input`
- `help`

## Where things come from

- **Models**: every `provider/model` declared under `"providers"` in opencode.jsonc (sorted). There is
  no separate model list to keep in sync.
- **Agents**: every `agents/*.md` whose frontmatter has a top-level `model:` line, with `local-mind`
  first. Agents without one inherit the default; `status` lists them as such.

## Safety

- Edits are line-based, so JSONC comments and the one-model-per-line layout are preserved.
- Every opencode.jsonc write is re-parsed first, and the write is refused if the result wouldn't parse.
- `limit` edits stay inside the `"limit": {...}` object. In the opencode 2 schema,
  `capabilities.input` (an array) sits earlier on the same line.
- After `ctx`/`image`, the config is re-read and any model that didn't take the change is named
  (for example, a model entry split across several lines).

## Controls (TUI)

- **↑/↓**: navigate
- **Type**: filter (case-insensitive substring)
- **Backspace**: edit the filter
- **Enter**: select
- **Esc / Ctrl+C**: abort with no changes. Nothing is written until the final confirm.

## Tests

```bash
cargo test --offline
```

The tests cover the file-editing logic in `src/config.rs` (`src/config/tests.rs`). The TUI is not
unit tested. `fixes.md` logs earlier TUI fixes.
