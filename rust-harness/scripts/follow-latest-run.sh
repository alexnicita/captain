#!/usr/bin/env bash
set -euo pipefail

EVENTS_FILE="/home/ec2-user/.openclaw/workspace/rust-harness/runs/events.jsonl"

if [[ ! -f "$EVENTS_FILE" ]]; then
  echo "events file not found: $EVENTS_FILE" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq required (sudo yum install -y jq)" >&2
  exit 2
fi

RUN_ID=$(jq -r 'select(.kind=="coding.run.started" or .kind=="coding.cycle.started") | .run_id' "$EVENTS_FILE" | tail -n 1)

if [[ -z "$RUN_ID" || "$RUN_ID" == "null" ]]; then
  echo "No run_id found yet. Start harness first." >&2
  exit 3
fi

echo "Following run_id=$RUN_ID"

tail -F "$EVENTS_FILE" | jq -r --arg rid "$RUN_ID" '
  select(.run_id==$rid and ((.kind|startswith("coding.")) or (.kind|startswith("git."))))
  | (.kind + " " + ((.data // {})|tostring))
'
