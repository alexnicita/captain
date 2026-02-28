#!/usr/bin/env bash
set -euo pipefail

# Canonical interface:
#   --repo <path> --time <duration>
# Optional:
#   --prompt "..." | --prompt-file /path/to/prompt.txt

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_DIR=""
TIME_INPUT=""
HEARTBEAT_SEC=30
CYCLE_PAUSE_SEC=2
EXECUTOR="cargo"
PROMPT=""
PROMPT_FILE=""
RUNTIME_LOG_FILE=""

usage() {
  cat <<'EOF'
Usage:
  scripts/harness.sh --repo <path> --time <duration> [options]

Required:
  --repo                Target repository path
  --time                Duration (e.g. 3600, 45m, 1h)

Optional:
  --executor            cargo|shell (default: cargo)
  --heartbeat-sec       Coding heartbeat interval (default: 30)
  --cycle-pause-sec     Pause between cycles in seconds (default: 2)
  --prompt              Optional user-session prompt string
  --prompt-file         Optional path to prompt text file (conflicts with --prompt)
  --runtime-log-file    Optional human-readable phase stream output

Examples:
  scripts/harness.sh --repo /path/to/repo --time 1h
  scripts/harness.sh --repo . --time 45m --prompt "improve test coverage"
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      REPO_DIR="${2:-}"; shift 2 ;;
    --time)
      TIME_INPUT="${2:-}"; shift 2 ;;
    --executor)
      EXECUTOR="${2:-cargo}"; shift 2 ;;
    --heartbeat-sec)
      HEARTBEAT_SEC="${2:-30}"; shift 2 ;;
    --cycle-pause-sec)
      CYCLE_PAUSE_SEC="${2:-2}"; shift 2 ;;
    --prompt)
      PROMPT="${2:-}"; shift 2 ;;
    --prompt-file)
      PROMPT_FILE="${2:-}"; shift 2 ;;
    --runtime-log-file)
      RUNTIME_LOG_FILE="${2:-}"; shift 2 ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "Unknown arg: $1" >&2
      usage
      exit 1 ;;
  esac
done

if [[ -z "$REPO_DIR" || -z "$TIME_INPUT" ]]; then
  usage
  exit 1
fi

if [[ ! -d "$REPO_DIR" ]]; then
  echo "Repo dir not found: $REPO_DIR" >&2
  exit 2
fi

if [[ -n "$PROMPT" && -n "$PROMPT_FILE" ]]; then
  echo "Use either --prompt or --prompt-file, not both." >&2
  exit 3
fi

"$ROOT_DIR/scripts/check_toolchain.sh"

CMD=(
  cargo run --manifest-path "$ROOT_DIR/Cargo.toml" --
  --config "$ROOT_DIR/config.example.toml"
  code
  --repo "$REPO_DIR"
  --time "$TIME_INPUT"
  --executor "$EXECUTOR"
  --heartbeat-sec "$HEARTBEAT_SEC"
  --cycle-pause-sec "$CYCLE_PAUSE_SEC"
)

if [[ -n "$PROMPT" ]]; then
  CMD+=(--prompt "$PROMPT")
fi
if [[ -n "$PROMPT_FILE" ]]; then
  CMD+=(--prompt-file "$PROMPT_FILE")
fi
if [[ -n "$RUNTIME_LOG_FILE" ]]; then
  CMD+=(--runtime-log-file "$RUNTIME_LOG_FILE")
fi

echo "[harness] repo=$REPO_DIR"
echo "[harness] time=$TIME_INPUT"
echo "[harness] executor=$EXECUTOR"
echo "[harness] heartbeat_sec=$HEARTBEAT_SEC cycle_pause_sec=$CYCLE_PAUSE_SEC"
if [[ -n "$PROMPT" || -n "$PROMPT_FILE" ]]; then
  echo "[harness] prompt=provided"
else
  echo "[harness] prompt=empty"
fi

"${CMD[@]}"

echo "[harness] completed"