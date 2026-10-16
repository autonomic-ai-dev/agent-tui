# agent-tui — Context for agents

Terminal dashboard for Autonomic organ health and live workflow visibility. Part of [agent-body](https://github.com/autonomic-ai-dev/agent-body).

| Field | Value |
|-------|-------|
| Role | Operator TUI |
| Binary | `agent-tui` |
| Transport | NATS subscriber (no dedicated HTTP port) |
| Version | 0.1.2+ |

## When to use

Local ops visibility: heart metrics, spine DAG progress, muscle sandbox logs, brain routing hints — without leaving the terminal.

```bash
autonomic tui
# or: cargo run --release -p agent-tui
```

## Verification

TUI is observational. Record product verification with `agent-brain verify --` in the repo under test.

## CI

Path filter on `src/**` + Cargo/Makefile/scripts/`.github` (crate lives at repo root). Docs-only PRs skip compose.

## Related

Nerves (event bus) · heart · spine · muscle · brain
