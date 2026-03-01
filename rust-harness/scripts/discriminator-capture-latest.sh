#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASE_DIR="${CAPTURE_DIR:-$ROOT_DIR/.harness/discriminator-captures}"
LINES="${1:-20}"

if [[ ! -d "$BASE_DIR" ]]; then
  echo "[latest] capture base not found: $BASE_DIR" >&2
  exit 1
fi

LATEST_DIR="$(find "$BASE_DIR" -mindepth 1 -maxdepth 1 -type d | sort | tail -n 1)"
if [[ -z "$LATEST_DIR" ]]; then
  echo "[latest] no capture runs found under $BASE_DIR" >&2
  exit 1
fi

echo "[latest] dir: $LATEST_DIR"

echo "--- metadata.json ---"
if [[ -f "$LATEST_DIR/metadata.json" ]]; then
  jq . "$LATEST_DIR/metadata.json" 2>/dev/null || cat "$LATEST_DIR/metadata.json"
else
  echo "(missing metadata.json)"
fi

echo "--- act-summaries.jsonl (last $LINES) ---"
if [[ -f "$LATEST_DIR/act-summaries.jsonl" ]]; then
  tail -n "$LINES" "$LATEST_DIR/act-summaries.jsonl"
else
  echo "(missing act-summaries.jsonl)"
fi
