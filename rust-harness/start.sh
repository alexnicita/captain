#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

DURATION="${1:-1h}"
PROMPT="${HARNESS_PROMPT:-Implement concrete Rust code changes in src/ with tests; avoid docs-only edits. Keep commits specific and useful.}"

if [[ -n "$(git status --porcelain)" ]]; then
  echo "[start] repo is dirty. Commit/stash first, then rerun ./start.sh" >&2
  git status --short
  exit 2
fi

rm -f .git/.agent-harness-code.lock

export HARNESS_PROVIDER="http"
export HARNESS_PROVIDER_ENDPOINT="https://api.openai.com/v1/responses"
export HARNESS_MODEL="gpt-5.3-codex"

mkdir -p runs

echo "[start] repo=$(pwd)"
echo "[start] duration=$DURATION"
echo "[start] model=$HARNESS_MODEL endpoint=$HARNESS_PROVIDER_ENDPOINT"
echo "[start] prompt=$PROMPT"

time bash ./scripts/harness.sh \
  --repo . \
  --time "$DURATION" \
  --prompt "$PROMPT" \
  --commit-each-cycle \
  --push-each-cycle \
  --noop-streak-limit 3 \
  --conformance-interval-unchanged 3 \
  --runtime-log-file ./runs/runtime.log \
  --thought-log-file ./runs/thoughts.md \
  2>&1 | tee ./runs/console.log

echo "[start] done"
