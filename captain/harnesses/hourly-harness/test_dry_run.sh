#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

CHECKLIST="checklist.test.md"
cat > "$CHECKLIST" <<'EOF'
- [x] harness boots
- [x] checklist parser works
EOF

python3 forced_hour_harness.py run --checklist "$CHECKLIST" --dry-run --dry-runtime-sec 20 --dry-heartbeat-sec 5
python3 forced_hour_harness.py status

RUN_DIR="$(cat runs/latest_run.txt)"
if ! grep -q "COMPLETE criteria satisfied" "$RUN_DIR/progress.log"; then
  echo "Expected COMPLETE entry missing"
  exit 1
fi

echo "Dry-run smoke test passed: $RUN_DIR"
