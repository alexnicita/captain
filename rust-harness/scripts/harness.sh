#!/usr/bin/env bash
set -euo pipefail

# General-purpose harness entrypoint with the exact interface:
#   --repo <path> --time <duration>
#
# Examples:
#   scripts/harness.sh --repo /path/to/repo --time 1h
#   scripts/harness.sh --repo . --time 3600

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_DIR=""
TIME_INPUT=""
HEARTBEAT_SEC=30
POLL_SEC=5

usage() {
  cat <<'EOF'
Usage:
  scripts/harness.sh --repo <path> --time <duration> [--heartbeat-sec N] [--poll-sec N]

Options:
  --repo            Target repository path to operate on (required)
  --time            Timebox duration: supports plain seconds (e.g. 3600) or suffixes s/m/h (e.g. 90m, 1h)
  --heartbeat-sec   Heartbeat interval seconds (default: 30)
  --poll-sec        Poll interval seconds (default: 5)
EOF
}

to_seconds() {
  local v="$1"
  if [[ "$v" =~ ^[0-9]+$ ]]; then
    echo "$v"; return 0
  elif [[ "$v" =~ ^([0-9]+)s$ ]]; then
    echo "${BASH_REMATCH[1]}"; return 0
  elif [[ "$v" =~ ^([0-9]+)m$ ]]; then
    echo "$(( ${BASH_REMATCH[1]} * 60 ))"; return 0
  elif [[ "$v" =~ ^([0-9]+)h$ ]]; then
    echo "$(( ${BASH_REMATCH[1]} * 3600 ))"; return 0
  fi
  echo "Invalid --time value: $v" >&2
  return 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      REPO_DIR="${2:-}"; shift 2 ;;
    --time)
      TIME_INPUT="${2:-}"; shift 2 ;;
    --heartbeat-sec)
      HEARTBEAT_SEC="${2:-30}"; shift 2 ;;
    --poll-sec)
      POLL_SEC="${2:-5}"; shift 2 ;;
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

RUNTIME_SEC="$(to_seconds "$TIME_INPUT")"
STATE_DIR="$ROOT_DIR/runs/runtime-gate-$(date +%Y%m%d-%H%M%S)"
CHECKLIST="$ROOT_DIR/fixtures/gate_checklist.done.md"

echo "[harness] repo=$REPO_DIR"
echo "[harness] runtime_sec=$RUNTIME_SEC"
echo "[harness] state_dir=$STATE_DIR"

# Execute from target repo context (the gate command itself currently does not accept --repo).
pushd "$REPO_DIR" >/dev/null
cargo run --manifest-path "$ROOT_DIR/Cargo.toml" -- gate start \
  --checklist "$CHECKLIST" \
  --runtime-sec "$RUNTIME_SEC" \
  --heartbeat-sec "$HEARTBEAT_SEC" \
  --poll-seconds "$POLL_SEC" \
  --base-dir "$STATE_DIR"
popd >/dev/null

echo "[harness] completed"
