# Product Tests

Captain product tests live here.

```text
captain/tests/
  rust-harness/       Cargo integration tests referenced from captain/harnesses/rust-harness/Cargo.toml
  hourly-harness/     pytest suite for the forced-runtime harness
```

Repository-level smoke tests remain under root `tests/` because they validate the full checkout, compatibility wrappers, and launch scripts.
