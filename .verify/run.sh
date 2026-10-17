#!/usr/bin/env bash
set -euo pipefail

# agent-tui verification master entrypoint
# Executed by automated verification runners or developer testing

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$DIR/.." && pwd)"

echo "==> [agent-tui] Starting verification suite..."
cd "$REPO_ROOT"

echo "==> Checking cargo clippy with -D warnings..."
cargo clippy -- -D warnings

echo "==> Checking cargo check..."
cargo check

echo "==> Running Playbook: Usage Tab..."
"$DIR/playbooks/verify_usage_tab.sh"

echo "==> Running Playbook: Full Suite..."
"$DIR/playbooks/verify_full_suite.sh"

echo "==> [agent-tui] All verification playbooks passed cleanly (exit 0)."
