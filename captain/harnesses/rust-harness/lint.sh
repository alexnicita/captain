#!/usr/bin/env bash
set -euo pipefail
if ! rustup component list --installed | grep -q clippy; then
  rustup component add clippy
fi
if ! rustup component list --installed | grep -q rustfmt; then
  rustup component add rustfmt
fi
cargo clippy -- -D warnings