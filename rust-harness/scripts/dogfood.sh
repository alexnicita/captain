#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

"$ROOT_DIR/scripts/check_toolchain.sh"

pushd "$ROOT_DIR" >/dev/null
rm -f ./runs/events.jsonl
cargo run -- --config ./config.example.toml run --objective "harness self-check time"
cargo run -- --config ./config.example.toml replay --path ./runs/events.jsonl
cargo run -- --config ./config.example.toml eval --path ./runs/events.jsonl
popd >/dev/null

echo "[dogfood] complete"
