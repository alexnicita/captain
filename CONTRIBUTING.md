# Contributing

Captain should remain reusable, local-first, and governance-focused.

## Development Loop

```bash
bash captain/scripts/captain-doctor.sh
bash tests/run.sh
cargo test --manifest-path captain/harnesses/rust-harness/Cargo.toml
```

For Rust changes:

```bash
cd captain/harnesses/rust-harness
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Pull Request Checklist

- Keep public interfaces stable unless the PR explicitly documents a migration.
- Update README, examples, or runbooks when behavior changes.
- Add fixture/eval coverage when event behavior changes.
- Do not commit local secrets, auth profiles, run logs, private repo contents, or personal operator files.
- Keep changes focused on agent governance, harness reliability, observability, safety, or install friction.

## Event Contract

JSONL event names, `run_id`, `task_id`, and monotonic `seq` behavior are public contracts. If you change them, update fixtures and replay/eval tests in the same PR.

