#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

scripts=(
  "scripts/setup-openclaw-captain.sh"
  "scripts/captain-doctor.sh"
  "scripts/setup-harness-env.sh"
  "scripts/init-local-profile.sh"
  "scripts/storage_guard.sh"
  "harnesses/hourly-harness/run.sh"
  "harnesses/rust-harness/start.sh"
  "examples/safe-pr-review.sh"
  "examples/one-hour-coding-sprint.sh"
  "examples/risky-change-caught.sh"
)

for s in "${scripts[@]}"; do
  [[ -f "$s" ]] || { echo "missing script: $s" >&2; exit 1; }
  bash -n "$s"
done

bash scripts/setup-openclaw-captain.sh --help >/dev/null
bash scripts/captain-doctor.sh --help >/dev/null
bash scripts/setup-harness-env.sh --help >/dev/null
bash scripts/init-local-profile.sh >/dev/null || true

echo "test_scripts: ok"
