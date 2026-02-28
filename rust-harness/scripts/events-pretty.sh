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
  def t($n): if . == null then "" else (.|tostring|gsub("\n";" ")|if length>$n then .[0:$n]+"…" else . end) end;
  . as $e |
  ($e.kind // "event") as $k |
  ($e.epoch // 0) as $ts |
  ($e.run_id // "-") as $rid |
  ($e.data // {}) as $d |
  if $k=="coding.cycle.start" then
    "🧠 [\($rid)] cycle #\(($d.cycle // "?")) START | remaining=\(($d.remaining_sec // "?"))s | executor=\(($d.executor // "?"))"
  elif $k=="coding.cycle.finish" then
    "✅ [\($rid)] cycle #\(($d.cycle // "?")) DONE | ok=\(($d.success // "?")) | commits=\(($d.commits // 0)) pushes=\(($d.pushes // 0))"
  elif $k=="coding.phase" then
    "🧩 [\($rid)] phase=\(($d.phase // "?")) | note=\(($d.note|t(120)))"
  elif $k=="tool.call" then
    "🔧 [\($rid)] tool=\(($d.tool // "?")) args=\(($d.args|t(120)))"
  elif $k=="tool.result" then
    "📥 [\($rid)] tool=\(($d.tool // "?")) ok=\(($d.ok // "?")) out=\(($d.output|t(120)))"
  elif $k=="git.commit" then
    "📦 [\($rid)] commit \(($d.sha // "?")) \(($d.message|t(120)))"
  elif $k=="git.push" then
    "🚀 [\($rid)] push branch=\(($d.branch // "?")) remote=\(($d.remote // "origin"))"
  elif $k=="run.heartbeat" then
    "💓 [\($rid)] heartbeat elapsed=\(($d.elapsed_sec // "?"))s remaining=\(($d.remaining_sec // "?"))s"
  elif $k=="run.finished" then
    "🏁 [\($rid)] finished status=\(($d.status // "done")) total_cycles=\(($d.cycles_total // "?"))"
  else
    "• [\($rid)] \($k) \(($d|t(140)))"
  end
'