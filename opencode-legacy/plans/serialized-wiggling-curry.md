# Plan: Multi-Agent Review Loop with Tier Switching

## Summary

Configure oh-my-opencode to support a **propose→review→iterate→implement** workflow between two local agents, with the ability to add paid agents ($sr, $$super) for escalation.

## Current State

Your existing `oh-my-opencode.json` already has:
- `big-pickle-agent` (local model, needs context window config)
- `local-qwen-agent` (32k context configured)
- `dual-agents` category with `parallel` strategy + `consensus` merge

**Problem**: The `parallel`/`consensus` approach doesn't match your desired **sequential review loop**.

---

## Implementation Plan

### Step 1: Reconfigure Agents for Review Loop

**File**: `config/opencode/oh-my-opencode.json`

Update the two local agents with review-specific prompts:

```jsonc
{
  "agents": {
    "proposer": {
      "model": "local-00/model",  // cosmo.lan:8700
      "maxTokens": 48000,
      "temperature": 0.3,
      "description": "Proposes code changes for peer review",
      "prompt_append": "You are the PROPOSER. Your role:\n1. Analyze requests and propose changes\n2. Format proposals with: files affected, description, rationale\n3. NEVER implement without reviewer approval\n4. If reviewer says 'revise', incorporate feedback and re-propose"
    },
    "reviewer": {
      "model": "local-01/model",  // cosmo.lan:8701
      "maxTokens": 28000,
      "temperature": 0.2,
      "description": "Reviews proposals from proposer",
      "prompt_append": "You are the REVIEWER. Your role:\n1. Critically review proposals from proposer\n2. Respond with: APPROVE (proceed), REVISE (with specific feedback), or REJECT (with reason)\n3. Focus on correctness, edge cases, and maintainability\n4. Be constructive but thorough"
    }
  }
}
```

### Step 2: Update Sisyphus to Orchestrate Review Loop

Add review protocol to Sisyphus's behavior:

```jsonc
{
  "agents": {
    "sisyphus": {
      "model": "local-00/model",  // ALL LOCAL in free tier
      "prompt_append": "## PEER REVIEW PROTOCOL (MANDATORY)\n\nFor code changes, follow this loop:\n\n1. PROPOSE: delegate_task(subagent_type='proposer', prompt='Propose: [task]')\n2. REVIEW: delegate_task(subagent_type='reviewer', prompt='Review: [proposal]')\n3. ITERATE: If 'REVISE', return to step 1 with feedback (max 3 iterations)\n4. ON DISAGREEMENT: After 3 iterations without agreement, STOP and ASK THE USER:\n   - Show the current proposal and reviewer feedback\n   - Ask: 'Escalate to sr-reviewer?', 'Override and implement?', or 'Continue iterating?'\n5. IMPLEMENT: Only after 'APPROVE' (or user override), delegate_task(subagent_type='proposer', prompt='Implement: [approved proposal]')\n\nNever skip the review step. Never auto-escalate without user confirmation."
    }
  }
}
```

### Step 3: Create Tier Configuration Files

Create directory structure:
```
~/.config/opencode/tiers/
├── free.json      # proposer + reviewer only
├── sr.json        # + senior agent
└── super.json     # + super architect
```

**File**: `~/.config/opencode/tiers/free.json`
```jsonc
{
  "$schema": "https://raw.githubusercontent.com/code-yeongyu/oh-my-opencode/master/assets/oh-my-opencode.schema.json",
  "agents": {
    "proposer": {
      "model": "local-00/model",  // cosmo.lan:8700
      "maxTokens": 48000
    },
    "reviewer": {
      "model": "local-01/model",  // cosmo.lan:8701
      "maxTokens": 28000
    },
    "sisyphus": {
      "model": "local-00/model"   // ALL LOCAL - truly free
    }
  },
  "disabled_agents": ["oracle", "prometheus", "metis", "momus", "atlas"]
}
```

**File**: `~/.config/opencode/tiers/sr.json`
```jsonc
{
  "$schema": "https://raw.githubusercontent.com/code-yeongyu/oh-my-opencode/master/assets/oh-my-opencode.schema.json",
  "agents": {
    "proposer": { "model": "local-00/model", "maxTokens": 48000 },
    "reviewer": { "model": "local-01/model", "maxTokens": 28000 },
    "sr-reviewer": {
      "model": "anthropic/claude-sonnet-4-5",
      "description": "Senior reviewer for escalation (user must approve calling this)",
      "prompt_append": "You are the SENIOR REVIEWER. The user chose to escalate to you after proposer/reviewer disagreed. Make the final decision with clear rationale."
    },
    "sisyphus": { "model": "local-00/model" }  // Still local, sr-reviewer is the paid escalation
  },
  "disabled_agents": ["prometheus", "metis", "momus"]
}
```

**File**: `~/.config/opencode/tiers/super.json`
```jsonc
{
  "$schema": "https://raw.githubusercontent.com/code-yeongyu/oh-my-opencode/master/assets/oh-my-opencode.schema.json",
  "agents": {
    "proposer": { "model": "local-00/model", "maxTokens": 48000 },
    "reviewer": { "model": "local-01/model", "maxTokens": 28000 },
    "super-architect": {
      "model": "anthropic/claude-opus-4-5",
      "variant": "max",
      "description": "Super architect for complex design (user must approve calling this)",
      "prompt_append": "You are the SUPER ARCHITECT. The user chose to involve you. Provide high-level guidance on architecture, system design, and complex trade-offs."
    },
    "sisyphus": { "model": "local-00/model" },  // Still local
    "oracle": { "model": "anthropic/claude-opus-4-5", "variant": "max" }  // Available for consult
  }
}
```

### Step 4: Create Tier Switching Script

**File**: `~/.local/bin/oc-tier` (or wherever you keep scripts)
```bash
#!/bin/bash
TIERS_DIR="$HOME/.config/opencode/tiers"
TARGET="$HOME/.config/opencode/oh-my-opencode.json"

case "$1" in
  free|sr|super)
    cp "$TIERS_DIR/$1.json" "$TARGET"
    echo "$1" > "$HOME/.config/opencode/.current-tier"
    echo "Switched to $1 tier"
    ;;
  ""|status)
    cat "$HOME/.config/opencode/.current-tier" 2>/dev/null || echo "not set"
    ;;
  *)
    echo "Usage: oc-tier [free|sr|super|status]"
    ;;
esac
```

### Step 5: Shell Aliases (optional convenience)

Add to `~/.bashrc` or `~/.zshrc`:
```bash
alias oc-free='oc-tier free'
alias oc-sr='oc-tier sr'
alias oc-super='oc-tier super'
```

---

## Files to Create/Modify

| File | Action |
|------|--------|
| `config/opencode/oh-my-opencode.json` | Modify - update agent definitions |
| `~/.config/opencode/tiers/free.json` | Create |
| `~/.config/opencode/tiers/sr.json` | Create |
| `~/.config/opencode/tiers/super.json` | Create |
| `~/.local/bin/oc-tier` | Create |
| `~/.bashrc` or `~/.zshrc` | Modify - add aliases |

---

## Key Decisions

1. **Review enforcement**: Via Sisyphus prompt engineering (not a custom hook)
   - Simpler to implement and modify
   - Can add hook later if needed

2. **Tier switching**: Via config file copying (not symlinks or env vars)
   - More explicit and debuggable
   - Easy to see what's active

3. **Agent naming**: `proposer`/`reviewer` instead of `agent-a`/`agent-b`
   - More semantic and self-documenting
   - Clearer in delegate_task calls

---

## Verification Plan

1. **Test basic setup**: Run `opencode` and verify agents load
2. **Test review loop**: Ask for a simple code change, verify propose→review→implement flow
3. **Test tier switching**: Run `oc-tier sr`, verify sr-reviewer becomes available
4. **Test escalation**: Force disagreement, verify escalation to sr-reviewer works

---

## Resolved Questions

- **Free tier orchestrator**: ALL LOCAL - Sisyphus uses local model too
- **Escalation behavior**: ASK USER FIRST before escalating
- **Provider mapping**: 8700=proposer, 8701=reviewer

# Original prompt
Use this for reference as to how this plan was derived...
Also use it for guidance of how to search for docs and info

## Goals

Im trying to configure opencode so that I can have two agents helping me code:
- Each agent comes from another "provider" or connection
- The agents are small I need to set expected context windows to be small on each (48k for one and 32k for the other)
- I want the agents to work "together":
  - AgentA will propse a change
  - AgentB will review the change and add suggestions / fixes
  - AgentA will review suggetions and sent back to Agent B
  - Loop there until both agents are mostly in agreement that things are good
  - AgentA implements changes
  
## Where to find info

I want you to use the web to learn things about omo and opencode
I also have both projects cloned into the vendor/ directory you can use this too for learning
  
## Open Questions

### Vanilla opencode vs omo (oh-my-opencode)

I want to know if I should use base opencode or opencode + oh-my-opencode. Right now I have oh-my-opencode installed / setup.
omo already has a multi agent setup that is somewhat close to this...
BUT im not sure if omo is flexible in letting me define my own new multi agent setups.

### Additions of other agents

I really want to be able to add additional agents soon. Its important to me to be able to add new agents to this mix
and control what they do, are responsible for and how they interact.

After I add more its VERY important for me to be able to control as easy as possible when Im operating with:
- My standard two agents (free cost wise for me)
- My two agents + super agent (super agent costs $$)
- My two agents + sr agent (sr agent also costs $$ but less than super agent)

I need to be able to switch rapidly in opencode

## Final

I want you to research on the omo documentation and tell me a plan of attack
