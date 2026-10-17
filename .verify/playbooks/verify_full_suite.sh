#!/usr/bin/env bash
set -euo pipefail

# Playbook: Verify Full Test Suite
# Runs all unit, integration, and doc tests across agent-tui.

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$DIR/../.." && pwd)"

echo "==> [Playbook: Full Suite] Running cargo test across agent-tui..."
cd "$REPO_ROOT"
cargo test

echo "==> [Playbook: Full Suite] Passed cleanly (exit 0)."
