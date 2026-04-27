# Captain Launch Backlog

_Last updated: 2026-04-27_

## P0 - Launch Readiness

- [ ] Keep `cargo test --manifest-path captain/harnesses/rust-harness/Cargo.toml` green.
- [ ] Keep `bash tests/run.sh` green on macOS and Linux without global Python package installs.
- [ ] Preserve the canonical runner: `captain/harnesses/rust-harness/scripts/harness.sh --repo <path> --time <duration> --executor openclaw`.
- [ ] Maintain stable JSONL event names with `run_id`, `task_id`, and monotonic `seq`.
- [ ] Keep `captain/scripts/captain-doctor.sh` as the first troubleshooting command.

## P1 - Governance Product Surface

- [ ] Add more event fixtures for failed commits, blocked commands, no-op streaks, and replay/eval regressions.
- [ ] Continue extracting `captain/src/rust-harness/coding.rs` into focused modules.
- [ ] Add anonymized report examples under a public gallery.
- [ ] Add a minimal governance config guide for command policy, runtime budgets, sandbox expectations, and commit gates.

## P2 - Distribution

- [ ] Record the 90-second demo from `docs/demo/90-second-demo.md`.
- [ ] Publish the technical teardown: "Autonomous coding agents need flight recorders, not vibes."
- [ ] Submit standout examples to OpenClaw community channels and showcase surfaces.
- [ ] Recruit 15-25 OpenClaw/Codex-style power users for incident-driven fixtures.
