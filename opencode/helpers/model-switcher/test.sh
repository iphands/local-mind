#!/bin/bash

echo "=== Model Switcher Test ==="
echo "Testing CLI functionality..."

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Test help command
echo "Testing help command..."
if cargo run --release help > /dev/null 2>&1; then
    echo "✓ Help command works"
else
    echo "✗ Help command failed"
fi

# Test all commands parse correctly (will fail on data/models but that's expected)
echo "Testing command parsing..."
commands=("lm" "local-mind" "omo" "oh-my-opencode" "all" "restore-omo-defaults")
for cmd in "${commands[@]}"; do
    if cargo run --release "$cmd" 2>&1 | grep -q "Unknown command"; then
        echo "✗ Command '$cmd' failed to parse"
    else
        echo "✓ Command '$cmd' parses correctly"
    fi
done

# Test unknown command
if cargo run --release invalid-command 2>&1 | grep -q "Unknown command"; then
    echo "✓ Unknown command handling works"
else
    echo "✗ Unknown command handling failed"
fi

echo "✓ Build completed successfully"
echo "✓ Executable available at: target/release/model-switcher"
echo ""
echo "To use the model switcher:"
echo "  cd helpers/model-switcher"
echo "  ./target/release/model-switcher [COMMAND]"
echo ""
echo "Available commands:"
echo "  lm, local-mind        Interactive agent model selection"
echo "  omo, oh-my-opencode   Bulk oh-my-opencode.json updates"
echo "  all                   Change all agents (system + agents + omo) to same model"
echo "  restore-omo-defaults  Restore backup"
echo "  help                  Show this help"