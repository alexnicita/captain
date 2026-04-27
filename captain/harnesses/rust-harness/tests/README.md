# Compatibility Test Stub

Rust harness integration tests now live under:

```text
captain/tests/rust-harness/
```

The Cargo manifest references those tests explicitly so existing commands such as
`cargo test --manifest-path captain/harnesses/rust-harness/Cargo.toml` keep working.
