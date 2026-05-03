#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

DURATION="${1:-1h}"
PROMPT="${HARNESS_PROMPT:-Implement concrete Rust code changes in src/ with tests; avoid docs-only edits. Keep commits specific and useful.}"
RECOVER_DIRTY="${HARNESS_RECOVER_DIRTY:-0}"
# Ensure RECOVER_DIRTY is either 0 or 1; otherwise default to 0
if [[ "$RECOVER_DIRTY" != "0" && "$RECOVER_DIRTY" != "1" ]]; then
  echo "[start] Warning: HARNESS_RECOVER_DIRTY must be 0 or 1; defaulting to 0" >&2
  RECOVER_DIRTY=0
fi

cleanup_runtime_artifacts() {
  rm -f .git/.agent-harness-code.lock
  rm -rf .harness/supercycle
}

stop_active_runs() {
  pkill -f "scripts/harness\.sh|agent-harness.* code|target/debug/agent-harness.* code" >/dev/null 2>&1 || true
}

recover_dirty_tree_if_enabled() {
  if [[ "$RECOVER_DIRTY" != "1" ]]; then
    return 0
  fi

  echo "[start] HARNESS_RECOVER_DIRTY=1 set; attempting dirty-tree recovery"
  stop_active_runs
  cleanup_runtime_artifacts

  # Reset tracked files to avoid dirty-tree spin from failed cycles.
  git reset --hard HEAD >/dev/null 2>&1 || true

  # Remove only harness-generated untracked dirs; keep user-owned files.
  rm -rf .harness/supercycle runs
}

recover_dirty_tree_if_enabled
cleanup_runtime_artifacts

if [[ -n "$(git status --porcelain)" ]]; then
  echo "[start] repo is dirty. Commit/stash first, then rerun ./start.sh" >&2
  echo "[start] tip: export HARNESS_RECOVER_DIRTY=1 for auto-recovery reset" >&2
  git status --short
  exit 2
fi

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
