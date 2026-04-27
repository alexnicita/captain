#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo test --manifest-path captain/harnesses/rust-harness/Cargo.toml --all-targets --all-features
