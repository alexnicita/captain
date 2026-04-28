#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STATE_FILE="$ROOT/memory/heartbeat-state.json"
NOW="$(date -u +%s)"

usage() {
  cat <<'EOF'
Usage:
  captain/scripts/heartbeat_checkin.sh --status
  captain/scripts/heartbeat_checkin.sh --check <email|calendar|mentions|weather|workspace|memory>
EOF
}

ensure_state() {
  mkdir -p "$(dirname "$STATE_FILE")"
  if [[ ! -f "$STATE_FILE" ]]; then
    cat > "$STATE_FILE" <<'JSON'
{
  "lastChecks": {
    "email": null,
    "calendar": null,
    "mentions": null,
    "weather": null,
    "workspace": null,
    "memory": null
  }
}
JSON
  fi
}

status() {
  python3 - "$STATE_FILE" "$NOW" <<'PY'
import json, sys
state_path = sys.argv[1]
now = int(sys.argv[2])
with open(state_path, 'r', encoding='utf-8') as f:
    data = json.load(f)
checks = data.get('lastChecks', {})
thresholds = {
    'email': 6*3600,
    'calendar': 6*3600,
    'mentions': 6*3600,
    'weather': 12*3600,
    'workspace': 8*3600,
    'memory': 24*3600,
}

def age(ts):
    if ts is None:
        return 'never'
    v = now - int(ts)
    return f"{v//3600}h{(v%3600)//60:02d}m ago"

print('heartbeat state:')
due=[]
for k in ['email','calendar','mentions','weather','workspace','memory']:
    ts=checks.get(k)
    is_due = ts is None or (now-int(ts) >= thresholds[k])
    print(f"- {k:9s} {'DUE' if is_due else 'ok '}  last={age(ts)}")
    if is_due:
        due.append(k)
if due:
    print('\nnext suggested check:')
    print(f"- {due[0]}")
else:
    print('\nall checks are fresh')
PY
}

mark() {
  local k="$1"
  case "$k" in
    email|calendar|mentions|weather|workspace|memory) ;;
    *) echo "Invalid check type: $k" >&2; usage; exit 2 ;;
  esac
  python3 - "$STATE_FILE" "$k" "$NOW" <<'PY'
import json, sys
path, key, now = sys.argv[1], sys.argv[2], int(sys.argv[3])
with open(path,'r',encoding='utf-8') as f:
    data=json.load(f)
data.setdefault('lastChecks', {})[key] = now
with open(path,'w',encoding='utf-8') as f:
    json.dump(data, f, indent=2)
    f.write('\n')
print(f"updated {key}={now}")
PY
}

main() {
  ensure_state
  case "${1:-}" in
    --status) status ;;
    --check) [[ $# -eq 2 ]] || { usage; exit 1; }; mark "$2" ;;
    -h|--help) usage ;;
    *) usage; exit 1 ;;
  esac
}

main "$@"
