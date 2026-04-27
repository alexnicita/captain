# Directory Map

Captain now has a top-level product tree:

```text
captain/
  src/
    rust-harness/       active Rust implementation
    hourly-harness/     active Python runtime gate
    kernel/             reserved lifecycle primitives
    drivers/            reserved external adapters
    policy/             reserved policy engines
    telemetry/          reserved event/reporting contracts
    tools/              reserved product helper tools
  tests/
    rust-harness/       Cargo integration tests
    hourly-harness/     pytest suite
    fixtures/           shared future fixtures
  MAINTAINERS           subsystem ownership map
  harnesses/            compatibility launch surfaces
  scripts/              setup, doctor, and operator scripts
  skills/               OpenClaw-compatible governance packs
  private/              gitignored local-only zone
```

## Compatibility Layer

Historical paths stay live:

- `captain/harnesses/rust-harness/Cargo.toml` points Cargo at `captain/src/rust-harness`.
- `captain/harnesses/hourly-harness/forced_hour_harness.py` dispatches to `captain/src/hourly-harness`.
- root `tests/` remains the repository smoke-test layer.

## Migration Rule

New implementation code should land under `captain/src`. The repository root must only contain these directories:

- `captain/` for product code, compatibility entrypoints, skills, templates, and private local state.
- `docs/` for public docs, demos, examples, and architecture notes.
- `tests/` for repo-level smoke and release checks.

Do not add new product logic directly under compatibility directories unless preserving an old command path requires a small wrapper.
