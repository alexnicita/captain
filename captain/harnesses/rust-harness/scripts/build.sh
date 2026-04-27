#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

"$ROOT_DIR/scripts/check_toolchain.sh"

pushd "$ROOT_DIR" >/dev/null
cargo build --all-targets
cargo run -- --config ./config.example.toml status
popd >/dev/null

echo "[build] complete"
