#!/usr/bin/env bash
set -euo pipefail

WORKSPACE="${WORKSPACE:-$HOME/.openclaw/workspace}"
REPO_DIR="$WORKSPACE"
HARNESS_DIR="$WORKSPACE/captain/harnesses/rust-harness"
DURATION="${1:-3h}"
BRANCH="${BRANCH:-main}"

cd "$REPO_DIR"

echo "[overnight-rust-harness] syncing $BRANCH at $(date -u +%FT%TZ)"
git fetch origin --prune
git checkout "$BRANCH"
git pull --rebase origin "$BRANCH"

echo "[overnight-rust-harness] starting harness for $DURATION"
cd "$HARNESS_DIR"
HARNESS_RECOVER_DIRTY=1 ./start.sh "$DURATION"
