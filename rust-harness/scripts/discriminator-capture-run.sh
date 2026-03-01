#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DURATION="10m"
PROMPT="${HARNESS_PROMPT:-10-minute discriminator prompt capture run}"
CAPTURE_DIR="${CAPTURE_DIR:-$ROOT_DIR/.harness/discriminator-captures}"
DRY_RUN=0

usage() {
  cat <<'USAGE'
Usage: scripts/discriminator-capture-run.sh [duration] [--dry-run]

Examples:
  scripts/discriminator-capture-run.sh
  scripts/discriminator-capture-run.sh 30m
  scripts/discriminator-capture-run.sh --dry-run
USAGE
}

for arg in "$@"; do
  case "$arg" in
    -h|--help)
      usage
      exit 0
      ;;
    --dry-run)
      DRY_RUN=1
      ;;
    *)
      DURATION="$arg"
      ;;
  esac
done

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "[capture] missing required command: $1" >&2
    exit 2
  }
}

require_cmd cargo
require_cmd jq
require_cmd bash

mkdir -p "$CAPTURE_DIR"
RUN_TS="$(date -u +%Y%m%d-%H%M%S)"
RUN_CAPTURE_DIR="$CAPTURE_DIR/$RUN_TS"
mkdir -p "$RUN_CAPTURE_DIR"

META_FILE="$RUN_CAPTURE_DIR/metadata.json"
SEEN_FILE="$RUN_CAPTURE_DIR/.seen"
ACT_FILE="$RUN_CAPTURE_DIR/act-summaries.jsonl"
RUNTIME_STDOUT="$RUN_CAPTURE_DIR/console.log"

START_EPOCH="$(date -u +%s)"
cat > "$META_FILE" <<EOF
{
  "run_ts": "$RUN_TS",
  "duration": "$DURATION",
  "prompt": "${PROMPT//"/\"}",
  "start_epoch": $START_EPOCH,
  "start_iso": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "dry_run": $DRY_RUN,
  "run_id": null,
  "exit_code": null,
  "end_epoch": null,
  "end_iso": null
}
EOF

echo "[capture] run duration=$DURATION"
echo "[capture] prompt=$PROMPT"
echo "[capture] output=$RUN_CAPTURE_DIR"

if [[ "$DRY_RUN" == "1" ]]; then
  echo "[capture] dry-run OK (prereqs + paths verified)"
  exit 0
fi

# Start from a clean runtime state.
rm -f .git/.agent-harness-code.lock
rm -rf .harness/supercycle runs
mkdir -p runs

touch "$SEEN_FILE"
: > "$ACT_FILE"

export HARNESS_PROVIDER="${HARNESS_PROVIDER:-http}"
export HARNESS_PROVIDER_ENDPOINT="${HARNESS_PROVIDER_ENDPOINT:-https://api.openai.com/v1/responses}"
export HARNESS_MODEL="${HARNESS_MODEL:-gpt-5.3-codex}"

HARNESS_PID=""
CAPTURE_EXIT=0

cleanup() {
  local sig="${1:-normal}"
  if [[ -n "$HARNESS_PID" ]] && kill -0 "$HARNESS_PID" >/dev/null 2>&1; then
    echo "[capture] trap cleanup ($sig): terminating harness pid=$HARNESS_PID"
    kill "$HARNESS_PID" >/dev/null 2>&1 || true
    wait "$HARNESS_PID" >/dev/null 2>&1 || true
  fi
}

trap 'cleanup SIGINT; exit 130' INT
trap 'cleanup SIGTERM; exit 143' TERM

# Run harness in background so we can stream/capture prompts while it executes.
cargo run -- --config ./config.example.toml code \
  --repo . \
  --time "$DURATION" \
  --prompt "$PROMPT" \
  --executor openclaw \
  --supercycle \
  --research-budget-sec 120 \
  --planning-budget-sec 30 \
  --commit-each-cycle \
  --push-each-cycle \
  --noop-streak-limit 3 \
  --conformance-interval-unchanged 3 \
  --runtime-log-file ./runs/runtime-capture.log \
  --thought-log-file ./runs/thoughts-capture.md \
  >"$RUNTIME_STDOUT" 2>&1 &
HARNESS_PID=$!

echo "[capture] harness pid=$HARNESS_PID"

extract_run_id() {
  [[ -f runs/events.jsonl ]] || return 0
  local rid
  rid="$(jq -r 'select(.kind=="run.started") | .run_id' runs/events.jsonl 2>/dev/null | tail -n 1)"
  if [[ -n "$rid" && "$rid" != "null" ]]; then
    tmp="$(mktemp)"
    jq --arg rid "$rid" '.run_id=$rid' "$META_FILE" > "$tmp" && mv "$tmp" "$META_FILE"
  fi
}

capture_prompt_paths() {
  [[ -f runs/events.jsonl ]] || return 0
  jq -r '
    select(.kind=="coding.cycle.act")
    | .data.commands[]?
    | .stdout_tail?
    | fromjson?
    | .prompt_path? // empty
  ' runs/events.jsonl 2>/dev/null | while read -r prompt_path; do
    [[ -z "$prompt_path" ]] && continue
    if ! grep -Fxq "$prompt_path" "$SEEN_FILE"; then
      echo "$prompt_path" >> "$SEEN_FILE"
      if [[ -f "$prompt_path" ]]; then
        cp "$prompt_path" "$RUN_CAPTURE_DIR/"
        echo "[capture] saved $(basename "$prompt_path")"
      fi
    fi
  done
}

capture_act_summaries() {
  [[ -f runs/events.jsonl ]] || return 0
  jq -c '
    select(.kind=="coding.cycle.act")
    | {
        ts_unix,
        run_id,
        cycle: (.data.cycle // null),
        success: (.data.success // null),
        error: (.data.error // null),
        command: ((.data.commands // [])[0].command // null),
        prompt_path: (((.data.commands // [])[0].stdout_tail // "") | fromjson? | .prompt_path // null),
        apply_detail: (((.data.commands // [])[0].stdout_tail // "") | fromjson? | .apply_detail // null)
      }
  ' runs/events.jsonl 2>/dev/null > "$ACT_FILE.tmp" || true
  mv "$ACT_FILE.tmp" "$ACT_FILE" 2>/dev/null || true
}

while kill -0 "$HARNESS_PID" >/dev/null 2>&1; do
  extract_run_id
  capture_prompt_paths
  capture_act_summaries
  sleep 2
done

wait "$HARNESS_PID" || CAPTURE_EXIT=$?

# Save key run artifacts.
cp -f runs/runtime-capture.log "$RUN_CAPTURE_DIR/" 2>/dev/null || true
cp -f runs/thoughts-capture.md "$RUN_CAPTURE_DIR/" 2>/dev/null || true
cp -f runs/events.jsonl "$RUN_CAPTURE_DIR/" 2>/dev/null || true

END_EPOCH="$(date -u +%s)"
tmp="$(mktemp)"
jq \
  --argjson code "$CAPTURE_EXIT" \
  --argjson end_epoch "$END_EPOCH" \
  --arg end_iso "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  '.exit_code=$code | .end_epoch=$end_epoch | .end_iso=$end_iso' \
  "$META_FILE" > "$tmp" && mv "$tmp" "$META_FILE"

echo "[capture] done"
echo "[capture] exit_code=$CAPTURE_EXIT"
echo "[capture] files:"
ls -la "$RUN_CAPTURE_DIR"

exit "$CAPTURE_EXIT"
