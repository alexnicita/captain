#!/usr/bin/env bash
set -euo pipefail
# Install clippy if missing
if ! rustup component list --installed | grep -q clippy; then
  rustup component add clippy
fi
pushd captain/harnesses/rust-harness > /dev/null
cargo clippy -- -D warnings
popd > /dev/null