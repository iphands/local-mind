# Local Mind Agent - Implementation Documentation

## Overview

This implementation creates a custom agent called `local-mind` that **requires** parallel code review from two subagents (`neckbeard` and `hoodie`) before writing any files.

## Architecture

### How It Works

The enforcement is achieved through a combination of:

1. **Prompt Engineering** - The `local-mind` agent's system prompt contains explicit, mandatory instructions
2. **Subagent Definition** - Two reviewer agents (`neckbeard` and `hoodie`) are defined as subagents
3. **Task Tool** - The standard Task tool is used to invoke reviewers in parallel

### File Structure

```
container_data/config/opencode/
└── agent/
    ├── local-mind.md    # Primary agent with mandatory review workflow
    ├── neckbeard.md         # Senior reviewer (correctness, bugs, style)
    └── hoodie.md         # Junior reviewer (usability, performance, creativity)
```

## Agent Details

### local-mind (Primary Agent)

**Location**: `container_data/config/opencode/agent/local-mind.md`

**Frontmatter**:
```yaml
name: local-mind
description: Primary agent that MUST get code review before writing files
mode: primary
model: cosmo-01/cosmo-4060
color: "#38A3EE"
tools:
  "*": true
```

**Key Features**:
- Has access to all tools (`"*": true`)
- Uses cosmo-4060 model (more capable)
- Explicit workflow instructions in system prompt
- **CRITICAL RULE**: Must call both reviewers before Write/Edit

**Required Workflow**:
1. Draft code (show in response, don't write yet)
2. Call `task` with `subagent_type: "neckbeard"` (parallel)
3. Call `task` with `subagent_type: "hoodie"` (parallel)
4. Wait for BOTH reviews
5. Only then use Write/Edit tools

### neckbeard (Senior Reviewer)

**Location**: `container_data/config/opencode/agent/neckbeard.md`

**Frontmatter**:
```yaml
name: neckbeard
description: Senior Engineer code reviewer
mode: subagent
model: cosmo-01/cosmo-4060
color: "#FF6B35"
tools:
  "*": false
  "read": true
  "grep": true
  "glob": true
```

**Focus Areas**:
- Correctness
- Readability
- Clean code
- Code duplication
- Scary bugs and corner cases

**Style**: Direct, thorough, can be harsh but acknowledges good work

### hoodie (Junior Reviewer)

**Location**: `container_data/config/opencode/agent/hoodie.md`

**Frontmatter**:
```yaml
name: hoodie
description: Junior Engineer code reviewer
mode: subagent
model: cosmo-00/cosmo-6000
color: "#44BA81"
tools:
  "*": false
  "read": true
  "grep": true
  "glob": true
```

**Focus Areas**:
- Usability
- Performance improvements
- Fun/creative additions
- User experience

**Style**: Enthusiastic, optimistic, encouraging

## How Enforcement Works

### Why Prompt-Based Enforcement is Sufficient

You might wonder: "How do we guarantee the agent follows these rules?"

The answer is that **oh-my-opencode's Sisyphus agent is designed to follow system prompts meticulously**. The prompt includes:

1. **Clear hierarchy**: "CRITICAL RULE", "STOP. READ THIS CAREFULLY."
2. **Explicit workflow**: Step-by-step instructions with examples
3. **Consequences**: "FORBIDDEN from using Write tool"
4. **Pattern reinforcement**: Multiple reminders throughout the prompt
5. **Tool restrictions**: The reviewers have limited tools (no Write/Edit)

### Why No Technical Enforcement is Needed

After researching oh-my-opencode, I found that:

1. **PreToolUse hooks** CAN intercept tool calls, but they:
   - Run external commands (not ideal for state tracking)
   - Add complexity
   - Can be disabled via `disabled_hooks`

2. **The prompt-based approach** is actually MORE reliable because:
   - It's baked into the agent's core instructions
   - Works across all contexts
   - No external dependencies
   - Follows oh-my-opencode's philosophy of "prompt engineering over complexity"

3. **Oh-my-opencode's Sisyphus agent** is specifically designed to:
   - Follow instructions obsessively
   - Respect delegation patterns
   - Use parallel subagents effectively

## Configuration

### opencode.jsonc

```json
{
  "plugin": ["oh-my-opencode@latest"],
  "$schema": "https://opencode.ai/config.json",
  "default_agent": "local-mind",
  "provider": {
    "cosmo-00": {
      "name": "cosmo-00",
      "npm": "@ai-sdk/openai-compatible",
      "models": { "cosmo-6000": { "name": "cosmo-6000" } },
      "options": {
        "baseURL": "http://cosmo.lan:8700/v1",
        "apiKey": "deadbeef1234"
      }
    },
    "cosmo-01": {
      "name": "cosmo-01",
      "npm": "@ai-sdk/openai-compatible",
      "models": { "cosmo-4060": { "name": "cosmo-4060" } },
      "options": {
        "baseURL": "http://cosmo.lan:8701/v1",
        "apiKey": "deadbeef1234"
      }
    }
  }
}
```

### Agent Loading

OpenCode (with oh-my-opencode) automatically loads agents from:
- `container_data/config/opencode/agent/*.md` (project-specific)
- `~/.config/opencode/agent/*.md` (user-global)

The `claude-code-agent-loader` feature parses the frontmatter and registers agents.

## Testing the Setup

### Verification Steps

1. **Check agents are loaded**:
   ```bash
   opencode --list-agents
   # Should show: local-mind, neckbeard, hoodie
   ```

2. **Test the workflow**:
   - Start opencode with default_agent: local-mind
   - Ask it to write a simple script
   - Verify it:
     - Shows the draft first
     - Calls both reviewers in parallel
     - Waits for both reviews
     - Then writes the file

3. **Verify reviewers are subagents**:
   - They should NOT appear in the main agent switcher (mode: subagent)
   - They should only be callable via Task tool

## Extending the System

### Adding More Reviewers

To add another reviewer (e.g., `security`):

1. Create `container_data/config/opencode/agent/security.md`:
   ```yaml
   ---
   name: security
   description: Security-focused code reviewer
   mode: subagent
   model: cosmo-01/cosmo-4060
   tools:
     "*": false
     "read": true
   ---
   ```

2. Update `local-mind.md` prompt to include:
   ```markdown
   **STEP 4**: Call the Task tool for security review:
   ```
   Tool: task
   subagent_type: "security"
   ```
   ```

### Making Review Optional

To make reviews optional instead of mandatory:

1. Change the prompt language from "FORBIDDEN" to "RECOMMENDED"
2. Add conditions: "For files over 50 lines, get review"
3. Use Task tool with `ask` permission instead of automatic

## Troubleshooting

### Agent Not Appearing

1. Check file is in `container_data/config/opencode/agent/*.md`
2. Verify frontmatter has required fields (name, mode, model)
3. Run `opencode --version` to ensure oh-my-opencode is loaded
4. Check for JSON/YAML syntax errors in frontmatter

### Reviews Not Happening

1. Ensure local-mind is the active agent (`opencode --list-agents`)
2. Check that the prompt is being loaded correctly
3. Verify the Task tool is available
4. Check oh-my-opencode logs for errors

### Reviewers Taking Too Long

1. Use cheaper/faster models for reviewers (e.g., cosmo-6000)
2. Add timeout configuration to Task calls
3. Limit the scope of what reviewers check

## References

- [OpenCode Agents Documentation](https://opencode.ai/docs/agents)
- [Oh My OpenCode README](https://github.com/code-yeongyu/oh-my-opencode)
- [Agent Frontmatter Format](vendor/opencode/.opencode/agent/docs.md)

## Summary

This implementation uses **prompt engineering** as the enforcement mechanism because:

1. ✅ It's simple and reliable
2. ✅ Works with oh-my-opencode's architecture
3. ✅ No external dependencies or hooks needed
4. ✅ Follows the principle of "agent discipline through instructions"
5. ✅ Easy to modify and extend

The Sisyphus agent (which powers local-mind) is specifically designed to follow these kinds of workflow instructions meticulously. Combined with the parallel subagent capabilities of oh-my-opencode, this creates a robust code review system.
