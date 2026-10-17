# agent-tui Verification Suite

Deterministic playbook suite testing terminal HUD rendering, tab navigation, NATS telemetry streaming, and **Tab 6: USAGE & BUDGET HUD**.

## Running Verification

```bash
# Master entrypoint:
.verify/run.sh

# Individual playbooks:
.verify/playbooks/verify_usage_tab.sh
.verify/playbooks/verify_full_suite.sh
```

## Playbook Matrix

| Playbook | Target Subsystem | Invariants Verified |
| :--- | :--- | :--- |
| `verify_usage_tab.sh` | Tab 6: Usage & Budget | Tab cycling across all 6 tabs, NATS usage event parsing, and subscription quota updates |
| `verify_full_suite.sh` | Complete agent-tui | 100% unit, integration, and doc tests pass with 0 warnings |
