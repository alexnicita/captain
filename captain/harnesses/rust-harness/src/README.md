# Compatibility Source Stub

The Rust harness implementation now lives under:

```text
captain/src/rust-harness/
```

`captain/harnesses/rust-harness/Cargo.toml` keeps the existing manifest path stable by pointing its lib, binary, and integration test targets at the top-level Captain product tree.
