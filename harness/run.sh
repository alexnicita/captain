#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PY="python3"

usage() {
  cat <<'EOF'
Usage:
  harness/run.sh start <checklist.md> [--run-id NAME]
  harness/run.sh start-dry <checklist.md> [--run-id NAME]
  harness/run.sh status [--run-dir PATH]
  harness/run.sh stop [--run-dir PATH]
EOF
}

cmd="${1:-}"
if [[ -z "$cmd" ]]; then
  usage
  exit 1
fi
shift

case "$cmd" in
  start)
    checklist="${1:-}"
    [[ -z "$checklist" ]] && { usage; exit 1; }
    shift
    exec "$PY" "$SCRIPT_DIR/forced_hour_harness.py" run --checklist "$checklist" "$@"
    ;;
  start-dry)
    checklist="${1:-}"
    [[ -z "$checklist" ]] && { usage; exit 1; }
    shift
    exec "$PY" "$SCRIPT_DIR/forced_hour_harness.py" run --checklist "$checklist" --dry-run "$@"
    ;;
  status)
    exec "$PY" "$SCRIPT_DIR/forced_hour_harness.py" status "$@"
    ;;
  stop)
    exec "$PY" "$SCRIPT_DIR/forced_hour_harness.py" stop "$@"
    ;;
  *)
    usage
    exit 1
    ;;
esac
