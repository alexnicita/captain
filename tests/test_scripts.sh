#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

scripts=(
  "captain/scripts/heartbeat_checkin.sh"
  "captain/scripts/setup-openclaw-captain.sh"
  "captain/scripts/captain-doctor.sh"
  "captain/scripts/setup-harness-env.sh"
  "captain/scripts/init-local-profile.sh"
  "captain/scripts/storage_guard.sh"
  "captain/scripts/overnight-rust-harness.sh"
  "captain/harnesses/hourly-harness/run.sh"
  "captain/harnesses/rust-harness/start.sh"
  "docs/examples/safe-pr-review.sh"
  "docs/examples/one-hour-coding-sprint.sh"
  "docs/examples/risky-change-caught.sh"
)

for s in "${scripts[@]}"; do
  [[ -f "$s" ]] || { echo "missing script: $s" >&2; exit 1; }
  bash -n "$s"
done

bash captain/scripts/setup-openclaw-captain.sh --help >/dev/null
bash captain/scripts/captain-doctor.sh --help >/dev/null
bash captain/scripts/setup-harness-env.sh --help >/dev/null
bash captain/scripts/init-local-profile.sh >/dev/null || true
bash captain/scripts/heartbeat_checkin.sh --status >/dev/null
bash captain/scripts/heartbeat_checkin.sh --check workspace >/dev/null

echo "test_scripts: ok"
