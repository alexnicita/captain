# Migration Notes: Python helper -> Rust runtime gate

This repository is Rust-first.

If you previously used a Python runtime/checklist helper (e.g., `forced_hour_harness.py` in another workspace location), migrate to native Rust CLI commands:

```bash
cargo run -- gate start --checklist ./fixtures/gate_checklist.done.md --dry-run --dry-runtime-sec 3 --dry-heartbeat-sec 1 --poll-seconds 1
cargo run -- gate status
cargo run -- gate stop
```

## Why

- single-language orchestration surface (Rust)
- easier contributor onboarding and packaging
- fewer runtime dependencies

## Compatibility model

- Shell scripts are thin wrappers around Rust commands.
- New orchestration logic should be added under `src/` modules, not Python sidecars.
