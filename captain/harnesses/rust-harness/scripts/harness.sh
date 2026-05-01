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
THOUGHT_LOG_FILE=""
NOOP_STREAK_LIMIT=""
CONFORMANCE_INTERVAL_UNCHANGED=""
PROGRESS_FILE=""
RUN_LOCK_FILE=""
COMMIT_EACH_CYCLE=0
PUSH_EACH_CYCLE=0

usage() {
  cat <<'EOF'
Usage:
  captain/harnesses/rust-harness/scripts/harness.sh --repo <path> --time <duration> [options]

Required:
  --repo                Target repository path
  --time                Duration (e.g. 3600, 45m, 1h)

Optional:
  --executor            cargo|shell|openclaw|hermes|claude|codex (default: cargo)
  --heartbeat-sec       Coding heartbeat interval (default: 30)
  --cycle-pause-sec     Pause between cycles in seconds (default: 2)
  --prompt              Optional user-session prompt string
  --prompt-file         Optional path to prompt text file (conflicts with --prompt)
  --runtime-log-file    Optional human-readable phase stream output
  --thought-log-file    Optional markdown thought log output per phase
  --noop-streak-limit   Force mutation after N no-op cycles
  --conformance-interval-unchanged  Run full conformance every K unchanged cycles
  --progress-file       Optional progress state file path
  --run-lock-file       Optional lock file path for single-instance guard
  --commit-each-cycle   Attempt commit hook each cycle
  --push-each-cycle     Attempt push after commit

Environment:
  CAPTAIN_CLEANUP_AUTO=1              Run guarded storage cleanup before harness start
  CAPTAIN_CLEANUP_MIN_FREE_GB=N       Free-space threshold for automatic cleanup
  CAPTAIN_CLAUDE_TOOLS=Read,Grep,...  Claude Code tool allowlist (default: Read,Grep,Glob,LS)
  CAPTAIN_CODEX_SANDBOX=read-only     Codex sandbox mode for JSON-edit generation

Examples:
  captain/harnesses/rust-harness/scripts/harness.sh --repo /path/to/repo --time 1h
  captain/harnesses/rust-harness/scripts/harness.sh --repo . --time 45m --prompt "improve test coverage"
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
    --thought-log-file)
      THOUGHT_LOG_FILE="${2:-}"; shift 2 ;;
    --noop-streak-limit)
      NOOP_STREAK_LIMIT="${2:-}"; shift 2 ;;
    --conformance-interval-unchanged)
      CONFORMANCE_INTERVAL_UNCHANGED="${2:-}"; shift 2 ;;
    --progress-file)
      PROGRESS_FILE="${2:-}"; shift 2 ;;
    --run-lock-file)
      RUN_LOCK_FILE="${2:-}"; shift 2 ;;
    --commit-each-cycle)
      COMMIT_EACH_CYCLE=1; shift ;;
    --push-each-cycle)
      PUSH_EACH_CYCLE=1; shift ;;
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

# Provider bootstrap so "run as-is" works with OpenClaw auth profiles.
# Priority:
#   1) explicit env overrides already set by operator
#   2) sane OpenAI defaults
#   3) OPENAI_API_KEY loaded from OpenClaw auth profile store when available
export HARNESS_PROVIDER="${HARNESS_PROVIDER:-http}"
export HARNESS_PROVIDER_ENDPOINT="${HARNESS_PROVIDER_ENDPOINT:-https://api.openai.com/v1/responses}"
export HARNESS_MODEL="${HARNESS_MODEL:-gpt-5.3-codex}"

if [[ -z "${OPENAI_API_KEY:-}" ]]; then
  AUTH_PROFILES_PATH="${OPENCLAW_AUTH_PROFILES:-$HOME/.openclaw/agents/main/agent/auth-profiles.json}"
  if [[ -f "$AUTH_PROFILES_PATH" ]]; then
    PROFILE_CRED="$(python3 - "$AUTH_PROFILES_PATH" <<'PY'
import json
import sys

path = sys.argv[1]

try:
    with open(path, "r", encoding="utf-8") as f:
        data = json.load(f)
except Exception:
    print("", end="")
    raise SystemExit(0)

profiles = (data.get("profiles") or {})

for pid in ("openai:default", "openai:manual"):
    key = ((profiles.get(pid) or {}).get("key") or "").strip()
    if key:
        print(key, end="")
        raise SystemExit(0)

# Fallback: some setups only have openai-codex OAuth access token.
for pid in ("openai-codex:default", "openai-codex:manual"):
    access = ((profiles.get(pid) or {}).get("access") or "").strip()
    if access:
        print(access, end="")
        raise SystemExit(0)

print("", end="")
PY
)"
    if [[ -n "$PROFILE_CRED" ]]; then
      export OPENAI_API_KEY="$PROFILE_CRED"
      echo "[harness] loaded provider credential from OpenClaw auth profiles."
    fi
    unset PROFILE_CRED
  fi
fi

executor_uses_own_auth() {
  case "$EXECUTOR" in
    hermes|claude|claude-code|codex) return 0 ;;
    *) return 1 ;;
  esac
}

if [[ -z "${OPENAI_API_KEY:-}" ]] && ! executor_uses_own_auth; then
  echo "[harness] error: OPENAI_API_KEY is unset and no OpenClaw auth profile credential was found." >&2
  echo "[harness] hint: run 'openclaw models status --json' to verify configured auth profiles." >&2
  exit 4
elif [[ -z "${OPENAI_API_KEY:-}" ]] && executor_uses_own_auth; then
  echo "[harness] OPENAI_API_KEY is unset; $EXECUTOR executor will use its own auth/config."
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
if [[ -n "$THOUGHT_LOG_FILE" ]]; then
  CMD+=(--thought-log-file "$THOUGHT_LOG_FILE")
fi
if [[ -n "$NOOP_STREAK_LIMIT" ]]; then
  CMD+=(--noop-streak-limit "$NOOP_STREAK_LIMIT")
fi
if [[ -n "$CONFORMANCE_INTERVAL_UNCHANGED" ]]; then
  CMD+=(--conformance-interval-unchanged "$CONFORMANCE_INTERVAL_UNCHANGED")
fi
if [[ -n "$PROGRESS_FILE" ]]; then
  CMD+=(--progress-file "$PROGRESS_FILE")
fi
if [[ -n "$RUN_LOCK_FILE" ]]; then
  CMD+=(--run-lock-file "$RUN_LOCK_FILE")
fi
if [[ "$COMMIT_EACH_CYCLE" -eq 1 ]]; then
  CMD+=(--commit-each-cycle)
fi
if [[ "$PUSH_EACH_CYCLE" -eq 1 ]]; then
  CMD+=(--push-each-cycle)
fi

if [[ "${CAPTAIN_CLEANUP_AUTO:-0}" == "1" ]]; then
  echo "[harness] storage cleanup auto-check enabled"
  bash "$ROOT_DIR/../../scripts/storage_guard.sh" --auto --min-free-gb "${CAPTAIN_CLEANUP_MIN_FREE_GB:-8}" || \
    echo "[harness] warning: storage cleanup auto-check failed; continuing" >&2
fi

echo "[harness] repo=$REPO_DIR"
echo "[harness] time=$TIME_INPUT"
echo "[harness] executor=$EXECUTOR"
echo "[harness] provider=$HARNESS_PROVIDER endpoint=$HARNESS_PROVIDER_ENDPOINT model=$HARNESS_MODEL"
echo "[harness] heartbeat_sec=$HEARTBEAT_SEC cycle_pause_sec=$CYCLE_PAUSE_SEC"
if [[ -n "$PROMPT" || -n "$PROMPT_FILE" ]]; then
  echo "[harness] prompt=provided"
else
  echo "[harness] prompt=empty"
fi

"${CMD[@]}"

echo "[harness] completed"
