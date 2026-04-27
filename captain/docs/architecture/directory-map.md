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
  docs/
    architecture/       architecture notes and directory policy
  MAINTAINERS           subsystem ownership map
```

## Compatibility Layer

Historical paths stay live:

- `harnesses/rust-harness/Cargo.toml` points Cargo at `captain/src/rust-harness`.
- `harnesses/hourly-harness/forced_hour_harness.py` dispatches to `captain/src/hourly-harness`.
- root `tests/` remains the repository smoke-test layer.

## Migration Rule

New implementation code should land under `captain/src`. Root-level directories should be either:

- distribution/launch surface (`README.md`, `examples/`, `demo/`, `skills/`)
- compatibility entrypoints (`harnesses/`, `scripts/`, `tests/`)
- private/local-only workspace support (`private/`, templates)

Do not add new product logic directly under compatibility directories unless preserving an old command path requires a small wrapper.
