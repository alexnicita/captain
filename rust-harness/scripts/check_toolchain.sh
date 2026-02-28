#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MIN_RUST="1.76.0"

version_ge() {
  # Returns success if $1 >= $2
  [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]
}

has_cmd() {
  command -v "$1" >/dev/null 2>&1
}

if ! has_cmd rustup; then
  cat >&2 <<'EOF'
[toolchain] rustup not found.
Install user-scoped Rust toolchain (no root required):
  curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
  source "$HOME/.cargo/env"
EOF
  exit 2
fi

if ! has_cmd rustc || ! has_cmd cargo; then
  echo "[toolchain] rustup exists but rustc/cargo missing from PATH." >&2
  echo "Try: source \"$HOME/.cargo/env\"" >&2
  exit 3
fi

RUSTC_VERSION="$(rustc --version | awk '{print $2}')"
if ! version_ge "$RUSTC_VERSION" "$MIN_RUST"; then
  echo "[toolchain] rustc $RUSTC_VERSION is below minimum $MIN_RUST" >&2
  echo "Update with: rustup update stable" >&2
  exit 4
fi

rustup component add clippy rustfmt >/dev/null

pushd "$ROOT_DIR" >/dev/null
cargo --version
rustc --version
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
popd >/dev/null

echo "[toolchain] ready"
