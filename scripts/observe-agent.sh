#!/usr/bin/env bash
set -euo pipefail

# Human-readable OpenClaw/subagent monitor
# Usage:
#   scripts/observe-agent.sh --file /path/to/transcript.jsonl
#   scripts/observe-agent.sh --latest
#   scripts/observe-agent.sh --latest --kb-root /home/ec2-user/.openclaw/workspace/kb

TRANSCRIPT_FILE=""
USE_LATEST=0
KB_ROOT="/home/ec2-user/.openclaw/workspace/kb"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --file)
      TRANSCRIPT_FILE="${2:-}"; shift 2 ;;
    --latest)
      USE_LATEST=1; shift ;;
    --kb-root)
      KB_ROOT="${2:-}"; shift 2 ;;
    -h|--help)
      sed -n '1,40p' "$0"; exit 0 ;;
    *)
      echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

if [[ "$USE_LATEST" -eq 1 ]]; then
  # newest jsonl in workspace (usually current run transcript)
  TRANSCRIPT_FILE=$(ls -1t /home/ec2-user/.openclaw/workspace/*.jsonl 2>/dev/null | head -n1 || true)
fi

if [[ -z "${TRANSCRIPT_FILE}" || ! -f "${TRANSCRIPT_FILE}" ]]; then
  echo "Transcript file not found. Provide --file or --latest." >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required. Install jq and re-run." >&2
  exit 2
fi

printf "\n🛰️  Monitoring transcript: %s\n" "$TRANSCRIPT_FILE"
printf "📁 KB root watch: %s\n\n" "$KB_ROOT"

# Background watcher: recently changed files
(
  while true; do
    echo "========== FILE ACTIVITY ($(date -u +"%Y-%m-%d %H:%M:%S UTC")) =========="
    find "$KB_ROOT" -type f -mmin -10 2>/dev/null | sort | sed 's#^#  • #' || true
    find /home/ec2-user/.openclaw/workspace/kb/assets/pdfs -type f -mmin -10 2>/dev/null | sort | sed 's#^#  ⬇ #' || true
    echo
    sleep 20
  done
) &
WATCH_PID=$!

cleanup() {
  kill "$WATCH_PID" >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

# Foreground: transcript event stream

tail -F "$TRANSCRIPT_FILE" | jq -r '
  def trunc($n): if (.|length) > $n then .[0:$n] + "…" else . end;

  if .role=="assistant" and (.content|type)=="array" then
    .content[]?
    | if .type=="toolCall" then
        "🔧 TOOL   " + (.name // "?") + "\n   args: " + ((.arguments|tostring) | gsub("\\n";" ") | trunc(280))
      elif .type=="text" then
        "📝 NOTE   " + ((.text // "") | gsub("\\n";" ") | trunc(220))
      elif .type=="thinking" then
        "🧭 STEP   internal planning update"
      else empty end
  elif .role=="toolResult" then
    "📥 RESULT " + (.toolName // "?") +
    "\n   ok: " + ((.isError|not)|tostring) +
    "\n   out: " + (((.content[0].text // "") | gsub("\\n";" ") | trunc(240)))
  else
    empty
  end
'
