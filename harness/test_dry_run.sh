#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

CHECKLIST="harness/checklist.test.md"
cat > "$CHECKLIST" <<'EOF'
- [x] harness boots
- [x] checklist parser works
EOF

python3 harness/forced_hour_harness.py run --checklist "$CHECKLIST" --dry-run --dry-runtime-sec 20 --dry-heartbeat-sec 5
python3 harness/forced_hour_harness.py status

RUN_DIR="$(cat harness/runs/latest_run.txt)"
if ! grep -q "COMPLETE criteria satisfied" "$RUN_DIR/progress.log"; then
  echo "Expected COMPLETE entry missing"
  exit 1
fi

echo "Dry-run smoke test passed: $RUN_DIR"
