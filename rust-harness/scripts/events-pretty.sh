#!/usr/bin/env bash
set -euo pipefail

EVENTS_FILE="${1:-/home/ec2-user/.openclaw/workspace/rust-harness/runs/events.jsonl}"

if [[ ! -f "$EVENTS_FILE" ]]; then
  echo "events file not found: $EVENTS_FILE" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq required (sudo yum install -y jq)" >&2
  exit 2
fi

tail -F "$EVENTS_FILE" | jq -r '
  def trunc($n): if . == null then "" else (tostring|gsub("\\n";" ")|if length>$n then .[0:$n]+"…" else . end) end;
  def ts: (.ts_unix // .epoch // 0 | tonumber | strftime("%H:%M:%S"));
  def rid: (.run_id // "-");
  def d: (.data // {});

  if .kind=="coding.phase" then
    "harness: coding phase="+(d.phase|trunc(40))
    +" run="+rid
    +" ts="+((.ts_unix // .epoch // 0)|tostring)
    +" reason='"+(d.reason|trunc(120))+"'"
    +" task='"+(d.selected_task|trunc(120))+"'"
    +" next='"+(d.next|trunc(120))+"'"

  elif .kind=="coding.cycle.started" then
    "harness: coding cycle start"
    +" cycle="+((d.cycle // "?")|tostring)
    +" run="+rid
    +" ts="+((.ts_unix // .epoch // 0)|tostring)
    +" remaining_sec="+((d.remaining_sec // "?")|tostring)
    +" executor="+((d.executor // "?")|tostring)

  elif .kind=="coding.cycle.plan" or .kind=="coding.cycle.act" or .kind=="coding.cycle.verify" then
    "harness: coding cycle stage"
    +" stage="+(if .kind=="coding.cycle.plan" then "plan" elif .kind=="coding.cycle.act" then "act" else "verify" end)
    +" cycle="+((d.cycle // "?")|tostring)
    +" ok="+((d.success // "?")|tostring)
    +" run="+rid
    +" ts="+((.ts_unix // .epoch // 0)|tostring)

  elif .kind=="git.fetch" then
    "harness: git fetch"
    +" ok="+((d.success // "?")|tostring)
    +" run="+rid
    +" ts="+((.ts_unix // .epoch // 0)|tostring)

  elif .kind=="git.pull" then
    "harness: git pull"
    +" ok="+((d.success // "?")|tostring)
    +" conflict="+((d.conflict // false)|tostring)
    +" run="+rid
    +" ts="+((.ts_unix // .epoch // 0)|tostring)

  elif .kind=="git.commit" then
    "harness: git commit"
    +" sha="+((d.sha // "?")|tostring)
    +" run="+rid
    +" ts="+((.ts_unix // .epoch // 0)|tostring)
    +" message='"+(d.message|trunc(140))+"'"

  elif .kind=="git.push" then
    "harness: git push"
    +" ok="+((d.success // "?")|tostring)
    +" remote="+((d.remote // "origin")|tostring)
    +" branch="+((d.branch // "master")|tostring)
    +" run="+rid
    +" ts="+((.ts_unix // .epoch // 0)|tostring)

  elif .kind=="coding.heartbeat" or .kind=="run.heartbeat" then
    "harness: coding heartbeat"
    +" cycles="+((d.cycles_total // "?")|tostring)
    +" elapsed_sec="+((d.elapsed_sec // "?")|tostring)
    +" remaining_sec="+((d.remaining_sec // "?")|tostring)
    +" run="+rid
    +" ts="+((.ts_unix // .epoch // 0)|tostring)

  elif .kind=="coding.cycle.finished" then
    "harness: coding cycle finish"
    +" cycle="+((d.cycle // "?")|tostring)
    +" ok="+((d.success // "?")|tostring)
    +" runtime_ms="+((d.runtime_ms // "?")|tostring)
    +" run="+rid
    +" ts="+((.ts_unix // .epoch // 0)|tostring)

  elif .kind=="run.finished" then
    "harness: run finished"
    +" status="+((d.status // "done")|tostring)
    +" total_cycles="+((d.cycles_total // "?")|tostring)
    +" run="+rid
    +" ts="+((.ts_unix // .epoch // 0)|tostring)

  else
    empty
  end
'