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
    "\n["+ts+"] 🧩 PHASE: "+(d.phase|trunc(40))+"\n"
    +"  run: "+rid+"\n"
    +"  reason: "+(d.reason|trunc(140))+"\n"
    +"  task: "+(d.selected_task|trunc(160))+"\n"
    +"  result: "+(d.result|trunc(140))+"\n"
    +"  next: "+(d.next|trunc(140))

  elif .kind=="coding.cycle.started" then
    "\n["+ts+"] 🧠 CYCLE #"+((d.cycle // "?")|tostring)+" START\n"
    +"  run: "+rid+"\n"
    +"  executor: "+((d.executor // "?")|tostring)+"\n"
    +"  remaining: "+((d.remaining_sec // "?")|tostring)+"s\n"
    +"  prompt: "+(if (d.prompt_provided // false) then "provided" else "none" end)

  elif .kind=="coding.cycle.plan" or .kind=="coding.cycle.act" or .kind=="coding.cycle.verify" then
    "["+ts+"] "
    + (if .kind=="coding.cycle.plan" then "🗺️ PLAN" elif .kind=="coding.cycle.act" then "⚙️ ACT" else "✅ VERIFY" end)
    +" cycle #"+((d.cycle // "?")|tostring)
    +" | ok="+((d.success // "?")|tostring)

  elif .kind=="git.fetch" then
    "["+ts+"] 🔄 GIT FETCH | ok="+((d.success // "?")|tostring)+" | out="+(d.stdout_tail|trunc(120))

  elif .kind=="git.pull" then
    "["+ts+"] ⬇️ GIT PULL | ok="+((d.success // "?")|tostring)+" | conflict="+((d.conflict // false)|tostring)+" | out="+(d.stdout_tail|trunc(120))

  elif .kind=="git.commit" then
    "\n["+ts+"] 📦 COMMIT\n"
    +"  sha: "+((d.sha // "?")|tostring)+"\n"
    +"  message: "+(d.message|trunc(160))

  elif .kind=="git.push" then
    "["+ts+"] 🚀 PUSH | ok="+((d.success // "?")|tostring)+" | remote="+((d.remote // "origin")|tostring)+" | branch="+((d.branch // "master")|tostring)

  elif .kind=="coding.heartbeat" or .kind=="run.heartbeat" then
    "["+ts+"] 💓 HEARTBEAT | cycles="+((d.cycles_total // "?")|tostring)+" | elapsed="+((d.elapsed_sec // "?")|tostring)+"s | remaining="+((d.remaining_sec // "?")|tostring)+"s"

  elif .kind=="coding.cycle.finished" then
    "["+ts+"] 🏁 CYCLE #"+((d.cycle // "?")|tostring)+" DONE | ok="+((d.success // "?")|tostring)+" | runtime="+((d.runtime_ms // "?")|tostring)+"ms"

  elif .kind=="run.finished" then
    "\n["+ts+"] ✅ RUN FINISHED\n"
    +"  run: "+rid+"\n"
    +"  status: "+((d.status // "done")|tostring)+"\n"
    +"  total_cycles: "+((d.cycles_total // "?")|tostring)

  else
    empty
  end
'