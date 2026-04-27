#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_REPO="${TARGET_REPO:-$PWD}"

bash "$ROOT/captain/harnesses/rust-harness/scripts/harness.sh" \
  --repo "$TARGET_REPO" \
  --time "${CAPTAIN_TIME:-20m}" \
  --executor openclaw \
  --runtime-log-file "$ROOT/captain/harnesses/rust-harness/runs/risky-change-runtime.log" \
  --noop-streak-limit 1 \
  --conformance-interval-unchanged 1 \
  --prompt "Attempt one small code change, then verify whether Captain blocks no-op, generic, unsafe, or internal-only commit behavior."
