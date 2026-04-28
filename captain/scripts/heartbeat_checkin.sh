#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STATE_FILE="$ROOT/memory/heartbeat-state.json"
NOW="$(date -u +%s)"

usage() {
  echo "Usage: captain/scripts/heartbeat_checkin.sh --status | --check <email|calendar|mentions|weather|workspace|memory>"
}

ensure_state() {
  mkdir -p "$(dirname "$STATE_FILE")"
  if [[ ! -f "$STATE_FILE" ]]; then
    cat >"$STATE_FILE" <<'JSON'
{"lastChecks":{"email":null,"calendar":null,"mentions":null,"weather":null,"workspace":null,"memory":null}}
JSON
  fi
}

print_status() {
  python3 - "$STATE_FILE" "$NOW" <<'PY'
import json,sys
p,now=sys.argv[1],int(sys.argv[2])
j=json.load(open(p,'r',encoding='utf-8'))
c=j.get('lastChecks',{})
th={'email':21600,'calendar':21600,'mentions':21600,'weather':43200,'workspace':28800,'memory':86400}
print('heartbeat state:')
d=[]
for k in ['email','calendar','mentions','weather','workspace','memory']:
    t=c.get(k)
    due=(t is None) or (now-int(t)>=th[k])
    age='never' if t is None else f"{(now-int(t))//3600}h{((now-int(t))%3600)//60:02d}m ago"
    print(f"- {k:9s} {'DUE' if due else 'ok '}  last={age}")
    if due:
        d.append(k)
print('\nnext suggested check:')
print(f"- {d[0]}" if d else '- none')
PY
}

mark_check() {
  python3 - "$STATE_FILE" "$1" "$NOW" <<'PY'
import json,sys
p,k,now=sys.argv[1],sys.argv[2],int(sys.argv[3])
j=json.load(open(p,'r',encoding='utf-8'))
j.setdefault('lastChecks',{})[k]=now
with open(p,'w',encoding='utf-8') as f:
    json.dump(j,f,indent=2)
    f.write('\n')
print(f"updated {k}={now}")
PY
}

main() {
  ensure_state
  case "${1:-}" in
    --status)
      print_status
      ;;
    --check)
      [[ $# -eq 2 ]] || { usage; exit 1; }
      case "$2" in
        email|calendar|mentions|weather|workspace|memory) mark_check "$2" ;;
        *) usage; exit 2 ;;
      esac
      ;;
    *)
      usage
      exit 1
      ;;
  esac
}

main "$@"
