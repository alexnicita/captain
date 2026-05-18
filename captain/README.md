# Captain Product Tree

This directory is the canonical Captain product namespace. It keeps implementation code, compatibility entrypoints, skills, private workspace support, and product tests under one top-level product tree.

The shape borrows two ideas:

- Linux-kernel-style subsystem ownership: code is grouped by operational responsibility, not by one giant application bucket.
- Polymarket Agents-style product root: a clear top-level product package with `src`, `tests`, docs, scripts, and examples around it.

## Layout

```text
captain/
  src/                 implementation code and subsystem adapters
  tests/               product test suites
  harnesses/           stable compatibility entrypoints
  scripts/             setup, doctor, and operator scripts
  skills/              OpenClaw-compatible governance packs
  tools/               product-owned helper tools
  templates/           local operator profile templates
  private/             gitignored local-only workspace zone
  MAINTAINERS          ownership map for subsystems
```

Compatibility entrypoints remain under the product tree:

- `captain/harnesses/rust-harness/Cargo.toml`
- `captain/harnesses/rust-harness/scripts/harness.sh`
- `captain/harnesses/hourly-harness/run.sh`
- `tests/run.sh`

That keeps the repository root strict while preserving stable, documented command surfaces.
