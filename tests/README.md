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
