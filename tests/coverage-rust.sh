#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  echo "cargo-llvm-cov not found. Install with:" >&2
  echo "  cargo install cargo-llvm-cov" >&2
  exit 2
fi

cargo llvm-cov \
  --manifest-path harnesses/rust-harness/Cargo.toml \
  --all-features \
  --workspace \
  --summary-only \
  --fail-under-lines "${RUST_COVERAGE_MIN:-70}"
