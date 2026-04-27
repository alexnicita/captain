#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HARNESS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
EVENTS_FILE="$HARNESS_ROOT/runs/events.jsonl"
FORMAT="plain"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --format)
      FORMAT="${2:-plain}"; shift 2 ;;
    -h|--help)
      cat <<'EOF'
Usage:
  scripts/events-pretty.sh [events_file] [--format plain|emoji]

Examples:
  scripts/events-pretty.sh
  scripts/events-pretty.sh --format emoji
  scripts/events-pretty.sh /path/to/events.jsonl --format plain
EOF
      exit 0 ;;
    *)
      if [[ "$1" == /* || "$1" == ./* || "$1" == ../* || "$1" == *.jsonl ]]; then
        EVENTS_FILE="$1"; shift
      else
        echo "unknown arg: $1" >&2
        exit 1
      fi
      ;;
  esac
done

if [[ ! -f "$EVENTS_FILE" ]]; then
  echo "events file not found: $EVENTS_FILE" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq required (sudo yum install -y jq)" >&2
  exit 2
fi

if [[ "$FORMAT" == "emoji" ]]; then
  tail -F "$EVENTS_FILE" | jq -r '
    def trunc($n): if . == null then "" else (tostring|gsub("\\n";" ")|if length>$n then .[0:$n]+"…" else . end) end;
    def ts: (.ts_unix // .epoch // 0 | tonumber | strftime("%H:%M:%S"));
    def rid: (.run_id // "-");
    def d: (.data // {});
    def task_title: (if (d.selected_task|type)=="object" then (d.selected_task.title // d.selected_task.id // "none") else (d.selected_task // "none") end);
    def act_cmd: ((d.commands // [])[0] // {});
    def act_prompt_path: ((act_cmd.stdout_tail // "") | fromjson? | .prompt_path // "");
    def act_apply_detail: ((act_cmd.stdout_tail // "") | fromjson? | .apply_detail // "");

    if .kind=="coding.phase" then
      "\n["+ts+"] 🧩 PHASE: "+(d.phase|trunc(40))+"\n"
      +"  run: "+rid+"\n"
      +"  reason: "+(d.reason|trunc(120))+"\n"
      +"  task: "+(task_title|trunc(120))+"\n"
      +"  next: "+(d.next|trunc(120))
    elif .kind=="coding.cycle.started" then
      "\n["+ts+"] 🧠 CYCLE #"+((d.cycle // "?")|tostring)+" START\n"
      +"  run: "+rid+"  remaining="+((d.remaining_sec // "?")|tostring)+"s"
    elif .kind=="coding.cycle.plan" then
      "["+ts+"] 🗺️ PLAN cycle #"+((d.cycle // "?")|tostring)+" ok="+((d.success // "?")|tostring)
    elif .kind=="coding.cycle.act" then
      "["+ts+"] ⚙️ ACT cycle #"+((d.cycle // "?")|tostring)+" ok="+((d.success // "?")|tostring)
      + (if (act_prompt_path|length)>0 then "\n  prompt: "+(act_prompt_path|trunc(160)) else "" end)
      + (if (act_apply_detail|length)>0 then "\n  result: "+(act_apply_detail|trunc(160)) else "" end)
      + (if ((d.error // "")|length)>0 then "\n  error: "+((d.error // "")|trunc(160)) else "" end)
    elif .kind=="coding.cycle.verify" then
      "["+ts+"] ✅ VERIFY cycle #"+((d.cycle // "?")|tostring)+" ok="+((d.success // "?")|tostring)
    elif .kind=="git.fetch" then
      "["+ts+"] 🔄 GIT FETCH ok="+((d.success // "?")|tostring)
    elif .kind=="git.pull" then
      "["+ts+"] ⬇️ GIT PULL ok="+((d.success // "?")|tostring)+" conflict="+((d.conflict // false)|tostring)
    elif .kind=="git.commit" then
      "["+ts+"] 📦 COMMIT " + (d.message|trunc(140))
    elif .kind=="git.push" then
      "["+ts+"] 🚀 PUSH ok="+((d.success // "?")|tostring)
    elif .kind=="coding.heartbeat" or .kind=="run.heartbeat" then
      "["+ts+"] 💓 HEARTBEAT cycles="+((d.cycles_total // "?")|tostring)+" remaining="+((d.remaining_sec // "?")|tostring)+"s"
    elif .kind=="coding.cycle.finished" then
      "["+ts+"] 🏁 CYCLE #"+((d.cycle // "?")|tostring)+" DONE ok="+((d.success // "?")|tostring)
    elif .kind=="run.finished" then
      "\n["+ts+"] ✅ RUN FINISHED total_cycles="+((d.cycles_total // "?")|tostring)
    else empty end
  '
else
  tail -F "$EVENTS_FILE" | jq -r '
    def trunc($n): if . == null then "" else (tostring|gsub("\\n";" ")|if length>$n then .[0:$n]+"…" else . end) end;
    def rid: (.run_id // "-");
    def d: (.data // {});

    if .kind=="coding.phase" then
      "harness: coding phase="+(d.phase|trunc(40))+" run="+rid+" ts="+((.ts_unix // .epoch // 0)|tostring)+" reason='"+(d.reason|trunc(120))+"' task='"+(d.selected_task|trunc(120))+"' next='"+(d.next|trunc(120))+"'"
    elif .kind=="coding.cycle.started" then
      "harness: coding cycle start cycle="+((d.cycle // "?")|tostring)+" run="+rid+" ts="+((.ts_unix // .epoch // 0)|tostring)+" remaining_sec="+((d.remaining_sec // "?")|tostring)+" executor="+((d.executor // "?")|tostring)
    elif .kind=="coding.cycle.plan" or .kind=="coding.cycle.act" or .kind=="coding.cycle.verify" then
      "harness: coding cycle stage stage="+(if .kind=="coding.cycle.plan" then "plan" elif .kind=="coding.cycle.act" then "act" else "verify" end)+" cycle="+((d.cycle // "?")|tostring)+" ok="+((d.success // "?")|tostring)+" run="+rid+" ts="+((.ts_unix // .epoch // 0)|tostring)
    elif .kind=="git.fetch" then
      "harness: git fetch ok="+((d.success // "?")|tostring)+" run="+rid+" ts="+((.ts_unix // .epoch // 0)|tostring)
    elif .kind=="git.pull" then
      "harness: git pull ok="+((d.success // "?")|tostring)+" conflict="+((d.conflict // false)|tostring)+" run="+rid+" ts="+((.ts_unix // .epoch // 0)|tostring)
    elif .kind=="git.commit" then
      "harness: git commit sha="+((d.sha // "?")|tostring)+" run="+rid+" ts="+((.ts_unix // .epoch // 0)|tostring)+" message='"+(d.message|trunc(140))+"'"
    elif .kind=="git.push" then
      "harness: git push ok="+((d.success // "?")|tostring)+" remote="+((d.remote // "origin")|tostring)+" branch="+((d.branch // "master")|tostring)+" run="+rid+" ts="+((.ts_unix // .epoch // 0)|tostring)
    elif .kind=="coding.heartbeat" or .kind=="run.heartbeat" then
      "harness: coding heartbeat cycles="+((d.cycles_total // "?")|tostring)+" elapsed_sec="+((d.elapsed_sec // "?")|tostring)+" remaining_sec="+((d.remaining_sec // "?")|tostring)+" run="+rid+" ts="+((.ts_unix // .epoch // 0)|tostring)
    elif .kind=="coding.cycle.finished" then
      "harness: coding cycle finish cycle="+((d.cycle // "?")|tostring)+" ok="+((d.success // "?")|tostring)+" runtime_ms="+((d.runtime_ms // "?")|tostring)+" run="+rid+" ts="+((.ts_unix // .epoch // 0)|tostring)
    elif .kind=="run.finished" then
      "harness: run finished status="+((d.status // "done")|tostring)+" total_cycles="+((d.cycles_total // "?")|tostring)+" run="+rid+" ts="+((.ts_unix // .epoch // 0)|tostring)
    else empty end
  '
fi
