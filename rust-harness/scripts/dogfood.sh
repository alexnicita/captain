#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found; install rustup toolchain first" >&2
  exit 1
fi

cargo run -- --config ./config.example.toml run --objective "harness self-check time"
cargo run -- --config ./config.example.toml replay --path ./runs/events.jsonl
cargo run -- --config ./config.example.toml eval --path ./runs/events.jsonl
