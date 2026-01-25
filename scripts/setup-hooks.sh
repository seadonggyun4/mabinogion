#!/bin/bash
#
# Setup script for git hooks
#
# Usage: ./scripts/setup-hooks.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
HOOKS_DIR="$PROJECT_ROOT/.githooks"

echo "Setting up git hooks for otsim-simulator..."

# Check if we're in a git repository
if [ ! -d "$PROJECT_ROOT/.git" ]; then
    echo "Error: Not a git repository"
    exit 1
fi

# Check if hooks directory exists
if [ ! -d "$HOOKS_DIR" ]; then
    echo "Error: .githooks directory not found"
    exit 1
fi

# Make hooks executable
chmod +x "$HOOKS_DIR"/*

# Configure git to use our hooks directory
git config core.hooksPath .githooks

echo ""
echo "✓ Git hooks configured successfully!"
echo ""
echo "The following hooks are now active:"
for hook in "$HOOKS_DIR"/*; do
    echo "  - $(basename "$hook")"
done
echo ""
echo "To disable hooks temporarily, use:"
echo "  git commit --no-verify"
echo ""
echo "To disable hooks permanently, run:"
echo "  git config --unset core.hooksPath"
