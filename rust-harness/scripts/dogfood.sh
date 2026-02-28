#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

"$ROOT_DIR/scripts/check_toolchain.sh"

pushd "$ROOT_DIR" >/dev/null
rm -f ./runs/events.jsonl
cargo run -- --config ./config.example.toml run --objective "harness self-check time"
cargo run -- --config ./config.example.toml replay --path ./runs/events.jsonl
cargo run -- --config ./config.example.toml eval --path ./runs/events.jsonl
cargo run -- gate start \
  --checklist ./fixtures/gate_checklist.done.md \
  --dry-run \
  --dry-runtime-sec 3 \
  --dry-heartbeat-sec 1 \
  --poll-seconds 1 \
  --base-dir ./runs/runtime-gate-dogfood
cargo run -- gate status --base-dir ./runs/runtime-gate-dogfood
popd >/dev/null

echo "[dogfood] complete"
