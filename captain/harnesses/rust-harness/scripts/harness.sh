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
  CAPTAIN_OPENROUTER_MODEL=<id>       Route provider + supported agent CLIs through OpenRouter
  OPENROUTER_API_KEY=<key>            OpenRouter API key for OpenRouter-routed runs
  CAPTAIN_OPENROUTER_ENV=<path>       OpenRouter env file (default: repo .env.openrouter)
  CAPTAIN_CLAUDE_TOOLS=Read,Grep,...  Claude Code tool allowlist (default: Read,Grep,Glob,LS)
  CAPTAIN_CODEX_MODE=goal             Enable Codex /goal long-running mode
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

source_env_file_if_present() {
  local env_file="$1"
  if [[ -f "$env_file" ]]; then
    set -a
    # shellcheck source=/dev/null
    source "$env_file"
    set +a
  fi
}

CAPTAIN_REPO_ROOT="$(cd "$ROOT_DIR/../../.." && pwd)"
CAPTAIN_OPENROUTER_ENV_PATH="${CAPTAIN_OPENROUTER_ENV:-$CAPTAIN_REPO_ROOT/.env.openrouter}"
source_env_file_if_present "$CAPTAIN_OPENROUTER_ENV_PATH"

# Provider bootstrap so "run as-is" works with OpenClaw auth profiles.
# Priority:
#   1) explicit env overrides already set by operator
#   2) installer-written OpenRouter defaults or explicit OpenRouter env
#   3) sane OpenAI defaults
#   4) provider credential loaded from OpenClaw auth profile store when available
OPENROUTER_REQUESTED=0
DEFAULT_OPENAI_PROVIDER="http"
DEFAULT_OPENAI_ENDPOINT="https://api.openai.com/v1/responses"
DEFAULT_OPENAI_MODEL="gpt-5.3-codex"
case "${CAPTAIN_PROVIDER:-}" in
  openrouter|OpenRouter) OPENROUTER_REQUESTED=1 ;;
esac
if [[ -n "${CAPTAIN_OPENROUTER_MODEL:-}" || "${HARNESS_PROVIDER_ENDPOINT:-}" == *openrouter.ai* ]]; then
  OPENROUTER_REQUESTED=1
fi

if [[ "$OPENROUTER_REQUESTED" == "1" ]]; then
  if [[ -z "${CAPTAIN_OPENROUTER_MODEL:-}" ]]; then
    if [[ -n "${HARNESS_MODEL:-}" && "$HARNESS_MODEL" != "$DEFAULT_OPENAI_MODEL" ]]; then
      export CAPTAIN_OPENROUTER_MODEL="$HARNESS_MODEL"
    else
      export CAPTAIN_OPENROUTER_MODEL="openrouter/auto"
    fi
  fi
  if [[ -z "${HARNESS_PROVIDER:-}" || "$HARNESS_PROVIDER" == "$DEFAULT_OPENAI_PROVIDER" ]]; then
    export HARNESS_PROVIDER="openai-compatible"
  fi
  if [[ -z "${HARNESS_PROVIDER_ENDPOINT:-}" || "$HARNESS_PROVIDER_ENDPOINT" == "$DEFAULT_OPENAI_ENDPOINT" ]]; then
    export HARNESS_PROVIDER_ENDPOINT="https://openrouter.ai/api/v1/chat/completions"
  fi
  if [[ -z "${HARNESS_PROVIDER_API_KEY_ENV:-}" || "$HARNESS_PROVIDER_API_KEY_ENV" == "OPENAI_API_KEY" ]]; then
    export HARNESS_PROVIDER_API_KEY_ENV="OPENROUTER_API_KEY"
  fi
  if [[ -z "${HARNESS_MODEL:-}" || "$HARNESS_MODEL" == "$DEFAULT_OPENAI_MODEL" ]]; then
    export HARNESS_MODEL="$CAPTAIN_OPENROUTER_MODEL"
  fi
else
  export HARNESS_PROVIDER="${HARNESS_PROVIDER:-$DEFAULT_OPENAI_PROVIDER}"
  export HARNESS_PROVIDER_ENDPOINT="${HARNESS_PROVIDER_ENDPOINT:-$DEFAULT_OPENAI_ENDPOINT}"
  export HARNESS_PROVIDER_API_KEY_ENV="${HARNESS_PROVIDER_API_KEY_ENV:-OPENAI_API_KEY}"
  export HARNESS_MODEL="${HARNESS_MODEL:-$DEFAULT_OPENAI_MODEL}"
fi

PROVIDER_KEY_ENV="${HARNESS_PROVIDER_API_KEY_ENV:-OPENAI_API_KEY}"

provider_key_value() {
  local env_name="$1"
  printf '%s' "${!env_name:-}"
}

if [[ -z "$(provider_key_value "$PROVIDER_KEY_ENV")" ]]; then
  AUTH_PROFILES_PATH="${OPENCLAW_AUTH_PROFILES:-$HOME/.openclaw/agents/main/agent/auth-profiles.json}"
  if [[ -f "$AUTH_PROFILES_PATH" ]]; then
    PROFILE_CRED="$(python3 - "$AUTH_PROFILES_PATH" "$PROVIDER_KEY_ENV" <<'PY'
import json
import sys

path = sys.argv[1]
key_env = sys.argv[2]

try:
    with open(path, "r", encoding="utf-8") as f:
        data = json.load(f)
except Exception:
    print("", end="")
    raise SystemExit(0)

profiles = (data.get("profiles") or {})

def profile_credential(profile):
    for field in ("key", "apiKey", "api_key", "access", "token"):
        value = ((profile or {}).get(field) or "").strip()
        if value:
            return value
    return ""

if key_env == "OPENAI_API_KEY":
    candidates = (
        "openai:default",
        "openai:manual",
        # Fallback: some setups only have openai-codex OAuth access token.
        "openai-codex:default",
        "openai-codex:manual",
    )
elif key_env == "OPENROUTER_API_KEY":
    candidates = ("openrouter:default", "openrouter:manual")
else:
    provider = key_env.lower()
    if provider.endswith("_api_key"):
        provider = provider[:-8]
    provider = provider.replace("_", "-")
    candidates = (f"{provider}:default", f"{provider}:manual")

for pid in candidates:
    credential = profile_credential(profiles.get(pid) or {})
    if credential:
        print(credential, end="")
        raise SystemExit(0)

print("", end="")
PY
)"
    if [[ -n "$PROFILE_CRED" ]]; then
      export "$PROVIDER_KEY_ENV=$PROFILE_CRED"
      echo "[harness] loaded $PROVIDER_KEY_ENV from OpenClaw auth profiles."
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

if [[ -z "$(provider_key_value "$PROVIDER_KEY_ENV")" ]] && ! executor_uses_own_auth; then
  echo "[harness] error: $PROVIDER_KEY_ENV is unset and no OpenClaw auth profile credential was found." >&2
  if [[ "$PROVIDER_KEY_ENV" == "OPENROUTER_API_KEY" ]]; then
    echo "[harness] hint: set OPENROUTER_API_KEY or run 'openclaw models auth login --provider openrouter'." >&2
  else
    echo "[harness] hint: run 'openclaw models status --json' to verify configured auth profiles." >&2
  fi
  exit 4
elif [[ -z "$(provider_key_value "$PROVIDER_KEY_ENV")" ]] && executor_uses_own_auth; then
  echo "[harness] $PROVIDER_KEY_ENV is unset; $EXECUTOR executor will use its own auth/config."
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
