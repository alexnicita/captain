# Source Tree

Captain source is organized by subsystem responsibility.

## Active Subsystems

- `rust-harness/` - Rust governance harness: provider/tool orchestration, coding loop, replay/eval, event sink, commit gates.
- `hourly-harness/` - Python runtime/checklist gate used for lightweight forced-duration workflows.

## Reserved Subsystem Boundaries

These directories are intentionally documented before they are filled. They are the future homes for code that should not accrete inside the main Rust loop:

- `kernel/` - run lifecycle primitives and core invariants.
- `drivers/` - provider, executor, and external-system adapters.
- `policy/` - command, tool, commit, sandbox, and release-gate policy engines.
- `telemetry/` - event schemas, replay formats, and report renderers.
- `tools/` - local helper implementations that are product code rather than repo scripts.

When moving code, keep compatibility wrappers in `harnesses/` or `scripts/` until a documented migration exists.
