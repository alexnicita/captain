#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/harnesses/hourly-harness"

CHECKLIST="checklist.test.md"
cat > "$CHECKLIST" <<'MD'
- [x] dry-run smoke test item 1
- [x] dry-run smoke test item 2
MD

python3 ./forced_hour_harness.py run \
  --checklist "$CHECKLIST" \
  --dry-run \
  --dry-runtime-sec 3 \
  --dry-heartbeat-sec 1 \
  --poll-seconds 1 >/tmp/hourly-harness-test.log

python3 ./forced_hour_harness.py status >/tmp/hourly-harness-status.log

grep -q "can_finish_now: True" /tmp/hourly-harness-status.log

echo "test_hourly_harness: ok"
