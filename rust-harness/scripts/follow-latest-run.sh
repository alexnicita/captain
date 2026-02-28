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

echo "🛰️ Following run_id=$RUN_ID"

tail -F "$EVENTS_FILE" | jq -r --arg rid "$RUN_ID" '
  def trunc($n): if . == null then "" else (tostring|gsub("\\n";" ")|if length>$n then .[0:$n]+"…" else . end) end;
  def d: (.data // {});
  def ts: (.ts_unix // .epoch // 0 | tonumber | strftime("%H:%M:%S"));

  select(.run_id==$rid and ((.kind|startswith("coding.")) or (.kind|startswith("git."))))
  | if .kind=="coding.phase" then
      "["+ts+"] 🧩 " + (d.phase|trunc(24)) + " → " + (d.next|trunc(24)) + " | " + (d.reason|trunc(90))
    elif .kind=="coding.cycle.started" then
      "["+ts+"] 🧠 cycle #" + ((d.cycle // "?")|tostring) + " start | remaining=" + ((d.remaining_sec // "?")|tostring) + "s"
    elif .kind=="coding.cycle.finished" then
      "["+ts+"] 🏁 cycle #" + ((d.cycle // "?")|tostring) + " done | ok=" + ((d.success // "?")|tostring) + " | runtime=" + ((d.runtime_ms // "?")|tostring) + "ms"
    elif .kind=="coding.counter" then
      "["+ts+"] 📊 counters | noop_streak=" + ((d.noop_streak // 0)|tostring) + " forced_mutation=" + ((d.forced_mutation // 0)|tostring) + " task_advanced=" + ((d.task_advanced // 0)|tostring)
    elif .kind=="coding.conformance.skipped" then
      "["+ts+"] ⏭️ conformance skipped | interval=" + ((d.interval // "?")|tostring) + " reason=" + (d.reason|trunc(50))
    elif .kind=="coding.cycle.hook" then
      (d.hooks // []) as $h |
      if ($h|length)==0 then "["+ts+"] 🪝 hooks: none"
      else "["+ts+"] 🪝 hooks: " + ($h|map(.name + "=" + (if .success then "ok" else "fail" end) + (if .skipped then "(skipped)" else "" end))|join(", ")) end
    elif .kind=="git.fetch" then
      "["+ts+"] 🔄 git fetch | ok=" + ((d.success // "?")|tostring)
    elif .kind=="git.pull" then
      "["+ts+"] ⬇️ git pull | ok=" + ((d.success // "?")|tostring) + " conflict=" + ((d.conflict // false)|tostring)
    elif .kind=="git.commit" then
      "["+ts+"] 📦 commit | " + (d.message|trunc(120))
    elif .kind=="git.push" then
      "["+ts+"] 🚀 push | ok=" + ((d.success // "?")|tostring)
    elif .kind=="coding.heartbeat" or .kind=="run.heartbeat" then
      "["+ts+"] 💓 heartbeat | cycles=" + ((d.cycles_total // "?")|tostring) + " remaining=" + ((d.remaining_sec // "?")|tostring) + "s"
    else
      "["+ts+"] • " + .kind + " " + (d|trunc(120))
    end
'