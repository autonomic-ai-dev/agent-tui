# Dashboard panels

## What it does

agent-tui is a six-tab terminal dashboard. It subscribes to the NATS event bus, sends each event to the tab that shows it (usage, quotas, budget alerts, spine workflows, brain context, muscle compute), and exits on `q` or `Esc`.

## How it works

1. **Subscription.** `main.rs` subscribes to `events.>`, `autonomic.>`, and `events.heart.>`. Messages reach the app through a channel.
2. **Overview (key 1).** A top-level status across organs.
3. **Workflows (key 2).** Events whose subject starts with `events.spine` or `autonomic.spine` update it.
4. **Mesh (key 3).** `refresh_organ_mesh` and `refresh_system_metrics` update one row per organ.
5. **Brain (key 4).** Events starting with `events.brain` or `autonomic.brain` show the context agent-brain routed.
6. **Logs (key 5).** Log lines from the organs, in arrival order.
7. **Usage (key 6).** `events.heart.usage` triggers `reload_usage`. `events.heart.quota` updates quota state. `events.heart.budget.alert` raises an alert. Until the first usage event, a placeholder reads "Awaiting live events on events.heart.usage".
8. **Navigation.** Keys `1` to `6` select a tab. `r` calls `refresh_all_telemetry`. `q` or `Esc` stops the app.

## Where the code is

- Entry and subscriptions: `agent-tui/src/main.rs`
- Tab renderers: `render_*_tab` functions
- State updates: `update_from_nats`, tested with the usage and quota subjects

## How to use it

- `autonomic tui`, or run `agent-tui` built from source
- Keys `1` to `6` switch tabs, `r` refreshes, `q` or `Esc` quits

## How it is verified

- `cargo test` for the whole crate (29 tests in the recorded baseline). These cover state updates and tab selection.

## Known limits

- Tab rendering is not tested. Only the state logic is.
- Without a live NATS broker the tabs stay empty, and that looks the same as "no activity".
- The README describes four panels. The code has six tabs. The code is correct; the README is out of date.
- The forward-tab keys were not read in the feature map pass.

## Related

- Feature map: `agent-tui/.verify/features/dashboard-panels.md`
- Event sources: [event-mesh](../../../agent-nerves/.verify/knowledge-base/event-mesh.md)
