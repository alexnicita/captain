# Tests

Basic smoke tests for repo scaffolding and harness entrypoints.

Run all tests:

```bash
bash tests/run.sh
```

Tests included:
- `test_scaffold.sh` — verifies expected folders/files exist
- `test_scripts.sh` — validates key shell scripts and help output
- `test_hourly_harness.sh` — runs a short dry-run of hourly harness logic
- `test-python.sh` — runs pytest suite for Python logic + policy checks
- `test-rust.sh` — runs cargo tests for rust-harness
- `coverage-rust.sh` — optional Rust coverage via cargo-llvm-cov

Optional knobs:
- `RUN_RUST_TESTS=1 bash tests/run.sh`
- `PY_COVERAGE_MIN=90 bash tests/test-python.sh`
- `RUST_COVERAGE_MIN=80 bash tests/coverage-rust.sh`
