#!/usr/bin/env bash
set -euo pipefail

CAPTAIN_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "$CAPTAIN_ROOT/.." && pwd)"
ENV_FILE="${CAPTAIN_OPENROUTER_ENV:-$REPO_ROOT/.env.openrouter}"
MODEL="${CAPTAIN_OPENROUTER_MODEL:-}"
API_KEY="${CAPTAIN_OPENROUTER_API_KEY:-${OPENROUTER_API_KEY:-}}"
UPDATE_OPENCLAW_MODEL="${UPDATE_OPENCLAW_MODEL:-1}"
NON_INTERACTIVE="${CAPTAIN_OPENROUTER_NON_INTERACTIVE:-0}"

usage() {
  cat <<'EOF'
Usage:
  captain openrouter setup [options]
  captain setup openrouter [options]
  captain/scripts/setup-openrouter.sh [options]

Options:
  --model <id>             OpenRouter model id, e.g. anthropic/claude-sonnet-4.6
  --api-key <key>          OpenRouter API key to write to the local env file
  --env-path <path>        Env file path (default: repo .env.openrouter)
  --non-interactive        Use env/default values without prompts
  --no-openclaw-model      Do not run 'openclaw models set' when OpenClaw is installed
  -h, --help               Show help

Environment variables supported:
  CAPTAIN_OPENROUTER_MODEL, OPENROUTER_API_KEY, CAPTAIN_OPENROUTER_API_KEY,
  CAPTAIN_OPENROUTER_ENV, CAPTAIN_OPENROUTER_NON_INTERACTIVE,
  UPDATE_OPENCLAW_MODEL
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --model)
      MODEL="${2:-}"; shift 2 ;;
    --api-key)
      API_KEY="${2:-}"; shift 2 ;;
    --env-path)
      ENV_FILE="${2:-}"; shift 2 ;;
    --non-interactive)
      NON_INTERACTIVE=1; shift ;;
    --no-openclaw-model)
      UPDATE_OPENCLAW_MODEL=0; shift ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 64 ;;
  esac
done

if [[ -f "$ENV_FILE" ]]; then
  set -a
  # shellcheck source=/dev/null
  source "$ENV_FILE"
  set +a
  MODEL="${MODEL:-${CAPTAIN_OPENROUTER_MODEL:-}}"
  API_KEY="${API_KEY:-${OPENROUTER_API_KEY:-}}"
fi

prompt_if_tty() {
  local prompt="$1"
  local default="$2"
  local answer
  if [[ "$NON_INTERACTIVE" == "1" || ! -t 0 ]]; then
    printf '%s\n' "$default"
    return 0
  fi

  if [[ -n "$default" ]]; then
    read -r -p "$prompt [$default]: " answer
    printf '%s\n' "${answer:-$default}"
  else
    read -r -p "$prompt: " answer
    printf '%s\n' "$answer"
  fi
}

prompt_secret_if_tty() {
  local prompt="$1"
  local default_present="$2"
  local answer
  if [[ "$NON_INTERACTIVE" == "1" || ! -t 0 ]]; then
    printf '%s\n' "$API_KEY"
    return 0
  fi

  if [[ "$default_present" == "1" ]]; then
    read -r -s -p "$prompt [keep existing/env value; Enter to keep]: " answer
    printf '\n' >&2
    if [[ -z "$answer" ]]; then
      printf '%s\n' "$API_KEY"
    else
      printf '%s\n' "$answer"
    fi
  else
    read -r -s -p "$prompt [optional; Enter to skip]: " answer
    printf '\n' >&2
    printf '%s\n' "$answer"
  fi
}

write_export() {
  local name="$1"
  local value="$2"
  printf 'export %s=%q\n' "$name" "$value"
}

openclaw_model_ref_from_openrouter_model() {
  local model="$1"
  case "$model" in
    openrouter/*) printf '%s\n' "$model" ;;
    *) printf 'openrouter/%s\n' "$model" ;;
  esac
}

write_openrouter_env() {
  local env_file="$1"
  local model="$2"
  local api_key="$3"
  local old_umask

  mkdir -p "$(dirname "$env_file")"
  old_umask="$(umask)"
  umask 077
  {
    printf '# Local-only Captain OpenRouter configuration.\n'
    printf '# This file is sourced automatically by captain/harnesses/rust-harness/scripts/harness.sh.\n'
    if [[ -n "$api_key" ]]; then
      write_export "OPENROUTER_API_KEY" "$api_key"
    else
      printf '# export OPENROUTER_API_KEY=your-openrouter-key\n'
    fi
    write_export "CAPTAIN_OPENROUTER_MODEL" "$model"
    write_export "HARNESS_PROVIDER" "openai-compatible"
    write_export "HARNESS_PROVIDER_ENDPOINT" "https://openrouter.ai/api/v1/chat/completions"
    write_export "HARNESS_PROVIDER_API_KEY_ENV" "OPENROUTER_API_KEY"
    write_export "HARNESS_MODEL" "$model"
  } > "$env_file"
  umask "$old_umask"
  chmod 600 "$env_file"
}

echo "[1/4] Configuring Captain OpenRouter model..."

MODEL="$(prompt_if_tty "OpenRouter model" "${MODEL:-openrouter/auto}")"
if [[ -z "$MODEL" ]]; then
  echo "OpenRouter model is required." >&2
  exit 2
fi

default_key_present=0
if [[ -n "$API_KEY" ]]; then
  default_key_present=1
fi
API_KEY="$(prompt_secret_if_tty "OpenRouter API key" "$default_key_present")"

echo "[2/4] Writing local env file..."
write_openrouter_env "$ENV_FILE" "$MODEL" "$API_KEY"
echo "Wrote $ENV_FILE"

echo "[3/4] Syncing supported agent defaults..."
if [[ "$UPDATE_OPENCLAW_MODEL" == "1" ]] && command -v openclaw >/dev/null 2>&1; then
  OPENCLAW_MODEL_REF="$(openclaw_model_ref_from_openrouter_model "$MODEL")"
  if openclaw models set "$OPENCLAW_MODEL_REF"; then
    echo "OpenClaw primary model set to $OPENCLAW_MODEL_REF"
  else
    echo "Warning: failed to update OpenClaw primary model." >&2
    echo "Set manually with: openclaw models set \"$OPENCLAW_MODEL_REF\"" >&2
  fi
else
  echo "OpenClaw model sync skipped."
fi

cat <<EOF

[4/4] OpenRouter setup complete

Captain will source:
  $ENV_FILE

Model:
  $MODEL

Agent behavior:
  - OpenClaw receives --model $(openclaw_model_ref_from_openrouter_model "$MODEL")
  - Hermes receives --provider openrouter --model $MODEL
  - Claude Code and Codex keep native config unless CAPTAIN_CLAUDE_MODEL,
    CAPTAIN_CODEX_MODEL, or CAPTAIN_AGENT_MODEL is set.

Try:
  captain hermes "fix the failing tests" --repo . --time 45m --dry-run

EOF
