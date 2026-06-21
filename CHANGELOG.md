# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
