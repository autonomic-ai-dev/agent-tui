# agent-tui — Context for agents

Terminal dashboard for Autonomic organ health and live workflow visibility. Part of [agent-body](https://github.com/autonomic-ai-dev/agent-body).

> Comprehensive operational context: [../CONTEXT.md](../CONTEXT.md)

| Field | Value |
|-------|-------|
| Role | Operator Terminal UI (k9s / htop analog) |
| Binary | `agent-tui` |
| Transport | NATS subscriber (`nats://127.0.0.1:4222`) |
| Version | v1.0.0-beta.3 |
| Docs Hub | [https://autonomic-ai.dev/docs/telemetry](https://autonomic-ai.dev/docs/telemetry) · [Architecture Hub](https://autonomic-ai.dev/docs/architecture) |

## When to use

Local ops visibility: heart metrics, spine DAG progress, muscle sandbox logs, brain routing hints — without leaving the terminal.

```bash
autonomic tui
# or: agent-tui
```

## Verification

TUI is observational. Record product verification with `agent-brain verify --` in the repo under test.

## CI

Path filter on `src/**` + Cargo/Makefile/scripts/`.github` (crate lives at repo root). Docs-only PRs skip compose.
