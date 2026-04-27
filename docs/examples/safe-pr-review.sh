#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_REPO="${TARGET_REPO:-$PWD}"

bash "$ROOT/captain/harnesses/rust-harness/scripts/harness.sh" \
  --repo "$TARGET_REPO" \
  --time "${CAPTAIN_TIME:-30m}" \
  --executor openclaw \
  --runtime-log-file "$ROOT/captain/harnesses/rust-harness/runs/pr-review-runtime.log" \
  --prompt "Review the current diff for correctness, tests, security risk, and commit readiness. Prefer comments or tests over broad rewrites."
