#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DURATION="${1:-10m}"
PROMPT="${HARNESS_PROMPT:-10-minute discriminator prompt capture run}"
CAPTURE_DIR="${CAPTURE_DIR:-$ROOT_DIR/.harness/discriminator-captures}"

mkdir -p "$CAPTURE_DIR"
RUN_TS="$(date -u +%Y%m%d-%H%M%S)"
RUN_CAPTURE_DIR="$CAPTURE_DIR/$RUN_TS"
mkdir -p "$RUN_CAPTURE_DIR"

echo "[capture] run duration=$DURATION"
echo "[capture] prompt=$PROMPT"
echo "[capture] output=$RUN_CAPTURE_DIR"

# Start from a clean runtime state.
rm -f .git/.agent-harness-code.lock
rm -rf .harness/supercycle runs
mkdir -p runs

export HARNESS_PROVIDER="${HARNESS_PROVIDER:-http}"
export HARNESS_PROVIDER_ENDPOINT="${HARNESS_PROVIDER_ENDPOINT:-https://api.openai.com/v1/responses}"
export HARNESS_MODEL="${HARNESS_MODEL:-gpt-5.3-codex}"

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
  >"$RUN_CAPTURE_DIR/console.log" 2>&1 &
HARNESS_PID=$!

echo "[capture] harness pid=$HARNESS_PID"

seen_file="$RUN_CAPTURE_DIR/.seen"
touch "$seen_file"

# Poll for newly referenced prompt artifacts from events and snapshot them.
while kill -0 "$HARNESS_PID" >/dev/null 2>&1; do
  if [[ -f runs/events.jsonl ]]; then
    jq -r '
      select(.kind=="coding.cycle.act")
      | .data.commands[]?
      | .stdout_tail?
      | fromjson?
      | .prompt_path? // empty
    ' runs/events.jsonl 2>/dev/null | while read -r prompt_path; do
      [[ -z "$prompt_path" ]] && continue
      if ! grep -Fxq "$prompt_path" "$seen_file"; then
        echo "$prompt_path" >> "$seen_file"
        if [[ -f "$prompt_path" ]]; then
          cp "$prompt_path" "$RUN_CAPTURE_DIR/"
          echo "[capture] saved $(basename "$prompt_path")"
        fi
      fi
    done
  fi
  sleep 2
done

wait "$HARNESS_PID" || true

# Save key run artifacts.
cp -f runs/runtime-capture.log "$RUN_CAPTURE_DIR/" 2>/dev/null || true
cp -f runs/thoughts-capture.md "$RUN_CAPTURE_DIR/" 2>/dev/null || true
cp -f runs/events.jsonl "$RUN_CAPTURE_DIR/" 2>/dev/null || true

echo "[capture] done"
echo "[capture] files:"
ls -la "$RUN_CAPTURE_DIR"
