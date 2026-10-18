# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0-beta.3.4] - 2026-10-18

### Fixed
- **Organ Mesh Multi-Layer Detection (All 11 Online)**:
  - Replaced fictitious `784x` ports with standard ecosystem ports (`agent-body: 3100`, `agent-brain: 3201`, `agent-spine: 3000`, `agent-muscle: 3103`, `agent-heart: 3101`, `agent-mouth: 3104`, `agent-immune: 3106`, `agent-nerves: 4222`, `agent-eyes: 3105`, `agent-tui: LOCAL`, `agent-ci-dashboard: 5173`).
  - Implemented multi-layer health detection probing supervisor daemons (`status.json`), active TCP listeners, Unix domain sockets (`serve.sock`), and installed CLI/MCP binaries on PATH, restoring all 11 organs to `OPERATIONAL`.

### Added
- **Real-Time Subscription Quotas & Reset Countdown**:
  - Connected `provider_quotas` table from `heart.db` to display live Claude Pro 5h rolling window utilization % and reset timer, along with OpenAI Tier 4 TPM utilization %.
- **Cache Deduplication & Savings Attribution**:
  - Dynamically computes cache hit savings percentage and dollar savings from captured prompt and cache read tokens.
- **Durable NATS Event Persistence**:
  - Added `record_usage_event_to_heart_db` and `record_quota_event_to_heart_db` in `telemetry_source.rs`, persisting live incoming NATS events directly into SQLite `heart.db`.

### Changed
- Bumped version to `v1.0.0-beta.3.4`.

## [1.0.0-beta.3.3] - 2026-10-18

### Added
- **NATS Credential Auto-Discovery & Telemetry Cache Fallback**: Seamlessly discover NATS credentials and fall back to local SQLite telemetry cache when remote NATS JetStream is offline.
- **TerminalGuard RAII**: Guaranteed clean alternate screen restoration and cursor restoration on panic, termination signals, or normal exits.

### Fixed
- **CLI Flag Terminal Hang**: Handled CLI flags (`--help`, `--version`) cleanly without initializing raw terminal modes or locking stdin.
- **SQLite Concurrency & Busy Timeouts**: Added 5-second busy timeout to prevent database locked errors during concurrent HUD queries.
- **CI Workflow Hardening**: Excluded markdown and documentation from CI workflow triggers with universal runtime path filtering.

### Changed
- Bumped version to `v1.0.0-beta.3.3`.

## [1.0.0-beta.3.2] - 2026-10-18

### Fixed
- **Clean Zero-State Usage Display**: Replaced placeholder token/cost figures in Tab 6 (USAGE & BUDGET) with clean `0` metrics until live NATS telemetry events are ingested.
- **Dynamic Binary Versioning**: Bound HUD version strings directly to `env!("CARGO_PKG_VERSION")` for automated release alignment.

### Added
- **CI Skip Path Optimizations**: Enhanced `.github/workflows/ci.yml` to skip pipeline execution when changes only affect `CHANGELOG.md`, `CONTEXT.md`, and markdown documentation files.

### Changed
- Bumped version to `v1.0.0-beta.3.2`.

## [1.0.0-beta.3.1] - 2026-10-17

### Added
- **Tab 6: USAGE & BUDGET Governance HUD (ABH-8)**: Live NATS telemetry integration for token expenditure, cost tracking across LLM providers, and real-time budget threshold alert banners.
- Verification playbook in `.verify/run.sh` ensuring zero compilation warnings and comprehensive test coverage.

### Changed
- Bumped version to `v1.0.0-beta.3.1`.

## [1.0.0-beta.3] - 2026-10-17

### Added
- **Stitch Mission Control Dashboard** — Multi-page terminal dashboard HUD aligned with cyber aerospace design system.
- Added comprehensive `CONTEXT.md` in repository root.

### Changed
- Bumped version to `v1.0.0-beta.3`.

## [1.0.0-beta.2] - 2026-10-16

### Added
- Official Homebrew tap formula support (`brew install agent-tui`).

### Changed
- Consolidated architecture documentation into centralized ecosystem hub.

## [1.0.0-beta.1] - 2026-10-16

### Changed
- Unified ecosystem versioning to 1.0.0-beta.1
- Dynamic GitHub pre-release flag in release workflows for beta/rc/alpha tags
- CI strictly arm64 macOS native runners with Developer ID code signing
- Aligned documentation and CI path filters with agent-brain

## [0.1.2] - 2026-07-18

### Added

- Developer ID sign macOS release binaries

## [0.1.1] - 2026-06-21

### Added

- Release pipeline aligned with other Autonomic organs (CI, staged build, changelog-driven GitHub releases)
- Multi-platform release artifacts named `agent-tui-{target}` for `autonomic tui` auto-install
- `AUTONOMIC_NATS_URL` env var for broker connection (defaults to `nats://127.0.0.1:4222`)

### Fixed

- Rustfmt and clippy checks in CI

## [0.1.0] - 2026-06-21

### Added

- Terminal observability dashboard for the Autonomic ecosystem
- Real-time NATS subscriptions for heart, spine, brain, and muscle events
- Cyber-themed layout with CPU/MEM gauges, DAG workflows, brain context, and sandbox logs
