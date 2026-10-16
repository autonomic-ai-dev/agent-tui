#!/usr/bin/env bash
# Build agent-tui release and sign on macOS (Developer ID only).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo build --release -p agent-tui
"$ROOT/scripts/sign-macos.sh"
