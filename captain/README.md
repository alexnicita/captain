# Captain Product Tree

This directory is the canonical Captain product namespace. It separates product code, tests, and architecture docs from repository-level distribution files.

The shape borrows two ideas:

- Linux-kernel-style subsystem ownership: code is grouped by operational responsibility, not by one giant application bucket.
- Polymarket Agents-style product root: a clear top-level product package with `src`, `tests`, docs, scripts, and examples around it.

## Layout

```text
captain/
  src/                 implementation code and subsystem adapters
  tests/               product test suites
  docs/                architecture and process documentation
  MAINTAINERS          ownership map for subsystems
```

Compatibility entrypoints remain at the historical root paths:

- `harnesses/rust-harness/Cargo.toml`
- `harnesses/rust-harness/scripts/harness.sh`
- `harnesses/hourly-harness/run.sh`
- `tests/run.sh`

That lets users keep existing commands while the codebase gains a cleaner top-level abstraction.
