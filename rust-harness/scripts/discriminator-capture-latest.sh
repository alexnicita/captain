#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASE_DIR="${CAPTURE_DIR:-$ROOT_DIR/.harness/discriminator-captures}"
LINES=20
JSON_MODE=0

for arg in "$@"; do
  case "$arg" in
    --json)
      JSON_MODE=1
      ;;
    *)
      LINES="$arg"
      ;;
  esac
done

if [[ ! -d "$BASE_DIR" ]]; then
  echo "[latest] capture base not found: $BASE_DIR" >&2
  exit 1
fi

LATEST_DIR="$(find "$BASE_DIR" -mindepth 1 -maxdepth 1 -type d | sort | tail -n 1)"
if [[ -z "$LATEST_DIR" ]]; then
  echo "[latest] no capture runs found under $BASE_DIR" >&2
  exit 1
fi

METADATA_PATH="$LATEST_DIR/metadata.json"
ACT_PATH="$LATEST_DIR/act-summaries.jsonl"
METADATA_EXISTS=false
ACT_EXISTS=false
[[ -f "$METADATA_PATH" ]] && METADATA_EXISTS=true
[[ -f "$ACT_PATH" ]] && ACT_EXISTS=true

if [[ "$JSON_MODE" == "1" ]]; then
  jq -nc \
    --arg latest_dir "$LATEST_DIR" \
    --arg metadata_path "$METADATA_PATH" \
    --arg act_summaries_path "$ACT_PATH" \
    --argjson metadata_exists "$METADATA_EXISTS" \
    --argjson act_summaries_exists "$ACT_EXISTS" \
    '{latest_dir:$latest_dir,metadata_path:$metadata_path,act_summaries_path:$act_summaries_path,exists:{metadata:$metadata_exists,act_summaries:$act_summaries_exists}}'
  exit 0
fi

echo "[latest] dir: $LATEST_DIR"

echo "--- metadata.json ---"
if [[ "$METADATA_EXISTS" == "true" ]]; then
  jq . "$METADATA_PATH" 2>/dev/null || cat "$METADATA_PATH"
else
  echo "(missing metadata.json)"
fi

echo "--- act-summaries.jsonl (last $LINES) ---"
if [[ "$ACT_EXISTS" == "true" ]]; then
  tail -n "$LINES" "$ACT_PATH"
else
  echo "(missing act-summaries.jsonl)"
fi
