#!/usr/bin/env bash
set -euo pipefail

# Nice human-readable monitor for OpenClaw agent/subagent work
# Shows: searches, fetches, downloads/writes, prompts, commits, pushes, and file activity.
#
# Usage:
#   scripts/observe-agent.sh --latest
#   scripts/observe-agent.sh --file /path/to/transcript.jsonl
#   scripts/observe-agent.sh --all
#   scripts/observe-agent.sh --latest --kb-root "$HOME/.openclaw/workspace/captain/knowledge"

TRANSCRIPT_FILE=""
USE_LATEST=0
USE_ALL=0
OPENCLAW_HOME="${OPENCLAW_HOME:-$HOME/.openclaw}"
WORKSPACE="${OPENCLAW_WORKSPACE:-$OPENCLAW_HOME/workspace}"
KB_ROOT="$WORKSPACE/captain/knowledge"
SESSIONS_DIR="$OPENCLAW_HOME/agents/main/sessions"
REFRESH_SECS=20

while [[ $# -gt 0 ]]; do
  case "$1" in
    --file)
      TRANSCRIPT_FILE="${2:-}"; shift 2 ;;
    --latest)
      USE_LATEST=1; shift ;;
    --kb-root)
      KB_ROOT="${2:-}"; shift 2 ;;
    --workspace)
      WORKSPACE="${2:-}"; shift 2 ;;
    --all)
      USE_ALL=1; shift ;;
    --refresh)
      REFRESH_SECS="${2:-20}"; shift 2 ;;
    -h|--help)
      sed -n '1,70p' "$0"; exit 0 ;;
    *)
      echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

if [[ "$USE_LATEST" -eq 1 ]]; then
  # Prefer gateway session transcript store, fall back to workspace root.
  TRANSCRIPT_FILE=$(ls -1t "$SESSIONS_DIR"/*.jsonl 2>/dev/null | head -n1 || true)
  if [[ -z "$TRANSCRIPT_FILE" ]]; then
    TRANSCRIPT_FILE=$(ls -1t "$WORKSPACE"/*.jsonl 2>/dev/null | head -n1 || true)
  fi
fi

if [[ "$USE_ALL" -ne 1 ]]; then
  if [[ -z "$TRANSCRIPT_FILE" || ! -f "$TRANSCRIPT_FILE" ]]; then
    echo "Transcript file not found. Use --latest or --file <path>, or use --all." >&2
    exit 1
  fi
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required. Install jq: sudo yum install -y jq" >&2
  exit 2
fi

if ! command -v stdbuf >/dev/null 2>&1; then
  echo "stdbuf is required (coreutils)." >&2
  exit 3
fi

hr() { printf '%*s\n' "${COLUMNS:-100}" '' | tr ' ' '-'; }

echo
hr
if [[ "$USE_ALL" -eq 1 ]]; then
  printf "🛰️  OBSERVING: ALL session transcripts (main + subagents)\n"
else
  printf "🛰️  OBSERVING: %s\n" "$TRANSCRIPT_FILE"
fi
printf "📁 WORKSPACE: %s\n" "$WORKSPACE"
printf "📚 KB ROOT  : %s\n" "$KB_ROOT"
printf "⏱️  REFRESH  : %ss\n" "$REFRESH_SECS"
hr
echo

echo "Legend:"
echo "  🎯 PROMPT      task/objective sent to subagent"
echo "  🔎 SEARCH      web/parallel searches"
echo "  🌐 FETCH       URL fetch/extraction"
echo "  💾 WRITE       files written/edited/downloaded"
echo "  🧪 EXEC        shell commands"
echo "  📦 GIT         commit/push signals"
echo "  📥 RESULT      tool output summary"
echo

# Background panel: filesystem + git heartbeat
(
  while true; do
    echo
    hr
    echo "📊 ACTIVITY SNAPSHOT ($(date -u +"%Y-%m-%d %H:%M:%S UTC"))"
    hr

    echo "[Recent commits]"
    git -C "$WORKSPACE" log --oneline -5 2>/dev/null || true

    echo
    echo "[Working tree]"
    git -C "$WORKSPACE" status --short 2>/dev/null || true

    echo
    echo "[All workspace files changed in last 10 min]"
    find "$WORKSPACE" \
      -path "$WORKSPACE/.git" -prune -o \
      -path "$WORKSPACE/node_modules" -prune -o \
      -type f -mmin -10 -print 2>/dev/null | sort | sed 's#^#  • #' || true

    echo
    echo "[KB files changed in last 10 min]"
    find "$KB_ROOT" -type f -mmin -10 2>/dev/null | sort | sed 's#^#  📚 #' || true

    echo
    echo "[PDFs touched in last 10 min]"
    find "$WORKSPACE/kb/assets/pdfs" -type f -mmin -10 2>/dev/null | sort | sed 's#^#  ⬇ #' || true

    sleep "$REFRESH_SECS"
  done
) &
SNAP_PID=$!

cleanup() {
  kill "$SNAP_PID" >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

# Event stream from transcript(s)
# We parse JSONL into readable categories with trimmed payloads.

if [[ "$USE_ALL" -eq 1 ]]; then
  mapfile -t _files < <(ls -1 "$SESSIONS_DIR"/*.jsonl 2>/dev/null || true)
  if [[ ${#_files[@]} -eq 0 ]]; then
    echo "No session transcript files found under $SESSIONS_DIR" >&2
    exit 1
  fi
  TAIL_CMD=(tail -qF "${_files[@]}")
else
  TAIL_CMD=(tail -qF "$TRANSCRIPT_FILE")
fi

"${TAIL_CMD[@]}" | stdbuf -oL jq -r '
  def t($n): if . == null then "" elif (.|type)=="string" then (if (.|length)>$n then .[0:$n]+"…" else . end) else ((.|tostring) | if (.|length)>$n then .[0:$n]+"…" else . end) end;
  def clean: gsub("\\n";" ") | gsub("\\s+";" ");

  if .role=="user" and (.content|type)=="array" then
    .content[]? | select(.type=="text") | "🎯 PROMPT  " + ((.text|clean)|t(260))

  elif .role=="assistant" and (.content|type)=="array" then
    .content[]? |
    if .type=="toolCall" then
      if .name=="web_search" then
        "🔎 SEARCH  web_search q=" + (((.arguments.query // "")|clean)|t(180))
      elif .name=="exec" then
        ( .arguments.command // "" ) as $cmd |
        if ($cmd|test("parallel-search\\.js\\s+--query")) then
          "🔎 SEARCH  parallel " + (($cmd|capture("--query\\s+\"(?<q>[^\"]+)\"").q // "(query parse failed)")|t(180))
        elif ($cmd|test("curl|wget")) then
          "💾 WRITE   download cmd=" + (($cmd|clean)|t(220))
        elif ($cmd|test("git\\s+commit|git\\s+push")) then
          "📦 GIT     " + (($cmd|clean)|t(220))
        else
          "🧪 EXEC    " + (($cmd|clean)|t(220))
        end
      elif .name=="web_fetch" then
        "🌐 FETCH   " + (((.arguments.url // "")|t(220)))
      elif .name=="write" then
        "💾 WRITE   path=" + (((.arguments.path // .arguments.file_path // "")|t(220)))
      elif .name=="edit" then
        "💾 WRITE   edit file=" + (((.arguments.path // .arguments.file_path // "")|t(220)))
      else
        "🔧 TOOL    " + (.name // "?") + " args=" + (((.arguments|tostring)|clean)|t(180))
      end

    elif .type=="text" then
      "📝 NOTE    " + (((.text // "")|clean)|t(220))
    elif .type=="thinking" then
      "🧭 STEP    planning/update"
    else empty end

  elif .role=="toolResult" then
    (.toolName // "?") as $tool |
    (.content[0].text // "") as $out |
    if ($tool=="web_search" or $tool=="web_fetch") then
      "📥 RESULT  " + $tool + " " + (($out|clean)|t(220))
    elif ($tool=="write" or $tool=="edit") then
      "💾 WRITE   result=" + (($out|clean)|t(220))
    elif ($tool=="exec" and ($out|test("\\[master [0-9a-f]+\\]|git push|-> master"))) then
      "📦 GIT     " + (($out|clean)|t(220))
    elif ($tool=="exec") then
      "📥 RESULT  exec " + (($out|clean)|t(220))
    else
      "📥 RESULT  " + $tool + " " + (($out|clean)|t(220))
    end

  else empty end
'
