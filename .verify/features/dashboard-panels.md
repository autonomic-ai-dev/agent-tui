# dashboard-panels

Summary: agent-tui is a six-tab terminal dashboard. It subscribes to the NATS event bus, routes each event to the tab that shows it (usage, quotas, budget alerts, spine workflows, brain context, muscle compute), and exits on `q` or `Esc`.

## Sub-features

- `nats-subscription`: Feeds every tab from the bus.
  - Intent: Keep the dashboard live without polling each organ.
  - Behaviour: `agent-tui/src/main.rs` subscribes to `events.>`, `autonomic.>`, and `events.heart.>`. Messages go to the app through a channel.
  - Outcome: Each message updates the state for its subject. A failed subscription leaves that tab empty. Not verified against a live broker in this pass.
- `tab-overview`: Shows the top-level status across organs.
  - Behaviour: Key `1` selects the Overview tab.
  - Outcome: Rendered from the shared state. Rendering is not checked by a test.
- `tab-workflows`: Shows workflow activity from agent-spine.
  - Behaviour: Key `2` selects the Workflows tab. Events whose subject starts with `events.spine` or `autonomic.spine` update it.
  - Outcome: Workflow activity lines in the tab.
- `tab-mesh`: Shows organ health and resource use.
  - Behaviour: Key `3` selects the Mesh tab. `refresh_organ_mesh` and `refresh_system_metrics` update it.
  - Outcome: One row per organ with its current state.
- `tab-brain`: Shows context that agent-brain routed.
  - Behaviour: Key `4` selects the Brain tab. Events starting with `events.brain` or `autonomic.brain` update it.
  - Outcome: The retrieved context for the latest route.
- `tab-logs`: Shows log lines from the organs.
  - Behaviour: Key `5` selects the Logs tab.
  - Outcome: Log lines in arrival order.
- `tab-usage`: Shows token usage and quota from agent-heart.
  - Behaviour: Key `6` selects the Usage tab. `events.heart.usage` triggers `reload_usage`. `events.heart.quota` updates quota state. `events.heart.budget.alert` raises an alert.
  - Outcome: Usage and quota figures. The `usage_tab.rs` placeholder "Awaiting live events on events.heart.usage" shows until the first event arrives.
- `tab-navigation-and-quit`: Moves between tabs and exits.
  - Behaviour: Keys `1` to `6` select a tab. Key `r` triggers a refresh (handler at `main.rs` line 603). `q` or `Esc` sets `running = false`.
  - Outcome: The active tab changes. The process exits on quit.

## Code

- `src`
- `Cargo.toml`
- `scripts`

## How to get to it (user POV)

- CLI: `autonomic tui`, or `agent-tui` built from source
- Keys: `1`-`6` to switch tabs, `r` to refresh, `q` or `Esc` to quit

## Verification Suites (Targeted Test Commands)

- **Unit Tests:** `cargo test` (whole crate, 29 tests in the baseline)
- **Functional API Tests (Hurl):** not applicable. The TUI has no HTTP surface.
- **Allowlisted Verification:** entries in `.verify/verify.toml`
- **Preconditions:** Rust toolchain. A live check needs NATS running with the other organs publishing events.

### Verification Steps

1. **tui-unit-tests** (sub-features: all)
   - Request: `cargo test`
   - Why: Proves the state update and tab logic. Existing tests include `update_from_nats("events.heart.usage", …)` and `update_from_nats("events.heart.quota", …)`, and a tab-selection assertion.
   - Expected: 29 passed, 0 failed (baseline).
   - Impact: A failure means a tab shows wrong data or the wrong tab is selected. Fix the cause.

## Gotchas

- Tabs are rendered by `render_*_tab` functions. Their output is not checked by any test in this pass. Only state logic is.
- The NATS subscription depends on a live broker. Without it, the tabs stay empty, and the failure looks like "no activity".
- The README describes four panels (top, left, right, bottom). The code has six tabs. The README is out of date, and this contract follows the code.
- The `r` key calls `refresh_all_telemetry` (`main.rs` line 604). Left and BackTab move to the previous tab. The forward-tab keys were not read in this pass.
</content>
