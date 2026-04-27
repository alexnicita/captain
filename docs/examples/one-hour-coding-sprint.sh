#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_REPO="${TARGET_REPO:-$PWD}"

bash "$ROOT/captain/harnesses/rust-harness/scripts/harness.sh" \
  --repo "$TARGET_REPO" \
  --time "${CAPTAIN_TIME:-1h}" \
  --executor openclaw \
  --runtime-log-file "$ROOT/captain/harnesses/rust-harness/runs/one-hour-runtime.log" \
  --commit-each-cycle \
  --prompt "Make one scoped product-quality improvement with tests. Keep the commit specific and avoid docs-only churn."
