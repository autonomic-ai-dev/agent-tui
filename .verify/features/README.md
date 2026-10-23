# agent-tui features

## Baseline preconditions

- Rust toolchain matching `Cargo.lock`.
- Live panel checks need NATS running with the other organs publishing events.

## Driving conventions

- Unit tests run through `cargo test`. Logs go to `.verify/artifacts/<RUN_ID>/`.

## Proof standards

- Panel state is proven by unit tests. Terminal rendering is not checked by any test in this pass.

## Feature index

| Feature file | Sub-features | Primary surface |
|---|---|---|
| `dashboard-panels.md` | 8 | `autonomic tui`, `agent-tui`, six tabs (keys `1`-`6`), `r` refresh, `q`/`Esc` quit |
