#!/usr/bin/env bash
set -euo pipefail

# Playbook: Verify Tab 6 Usage & Budget HUD
# Tests Tab 6 cycling, usage event updates, and quota processing.

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$DIR/../.." && pwd)"

echo "==> [Playbook: Usage Tab] Running tests::test_tab_cycling..."
cd "$REPO_ROOT"
cargo test tests::test_tab_cycling

echo "==> [Playbook: Usage Tab] Running tests::test_update_from_nats_usage_event..."
cargo test tests::test_update_from_nats_usage_event

echo "==> [Playbook: Usage Tab] Running tests::test_update_from_nats_quota_event..."
cargo test tests::test_update_from_nats_quota_event

echo "==> [Playbook: Usage Tab] Running usage_tab::tests..."
cargo test usage_tab::tests

echo "==> [Playbook: Usage Tab] Passed cleanly (exit 0)."
