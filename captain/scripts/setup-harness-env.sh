#!/usr/bin/env bash
set -euo pipefail

CAPTAIN_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HARNESS_DIR="${HARNESS_DIR:-$CAPTAIN_ROOT/harnesses/rust-harness}"
CONFIG_TEMPLATE="$HARNESS_DIR/config.example.toml"
CONFIG_LOCAL="$HARNESS_DIR/config.local.toml"
ENV_LOCAL="$HARNESS_DIR/.env.local"
PROMPT_DIR="$HARNESS_DIR/prompts"
PROMPT_FILE="$PROMPT_DIR/session-prompt.txt"

usage() {
  cat <<'EOF'
Usage:
  captain/scripts/setup-harness-env.sh [options]

Options:
  --harness-dir <path>      Override harness path
  -h, --help                Show help

What this initializes:
  - captain/harnesses/rust-harness/config.local.toml (from config.example.toml)
  - captain/harnesses/rust-harness/.env.local (OpenAI/OpenRouter placeholders)
  - captain/harnesses/rust-harness/prompts/session-prompt.txt
  - captain/harnesses/rust-harness/runs directory
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --harness-dir)
      HARNESS_DIR="$2"
      CONFIG_TEMPLATE="$HARNESS_DIR/config.example.toml"
      CONFIG_LOCAL="$HARNESS_DIR/config.local.toml"
      ENV_LOCAL="$HARNESS_DIR/.env.local"
      PROMPT_DIR="$HARNESS_DIR/prompts"
      PROMPT_FILE="$PROMPT_DIR/session-prompt.txt"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || { echo "Missing required command: $1" >&2; exit 2; }
}

if [[ ! -d "$HARNESS_DIR" ]]; then
  echo "Harness dir not found: $HARNESS_DIR" >&2
  exit 3
fi

need_cmd bash
need_cmd git
need_cmd node
need_cmd npm

if ! command -v cargo >/dev/null 2>&1; then
  cat <<'EOF'
Rust toolchain is not installed.
Install (user-scoped):
  curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
  source "$HOME/.cargo/env"
Then rerun this script.
EOF
  exit 4
fi

mkdir -p "$HARNESS_DIR/runs"
mkdir -p "$PROMPT_DIR"

if [[ -f "$CONFIG_TEMPLATE" && ! -f "$CONFIG_LOCAL" ]]; then
  cp "$CONFIG_TEMPLATE" "$CONFIG_LOCAL"
  echo "Created $CONFIG_LOCAL"
elif [[ -f "$CONFIG_LOCAL" ]]; then
  echo "Config already exists: $CONFIG_LOCAL"
else
  echo "Missing config template: $CONFIG_TEMPLATE" >&2
  exit 5
fi

if [[ ! -f "$ENV_LOCAL" ]]; then
  cat > "$ENV_LOCAL" <<'EOF'
# Local-only secrets for captain/harnesses/rust-harness (gitignored)
OPENAI_API_KEY=

# Optional provider overrides
HARNESS_PROVIDER=http
HARNESS_PROVIDER_ENDPOINT=https://api.openai.com/v1/responses
HARNESS_MODEL=gpt-5.3-codex

# OpenRouter option:
# OPENROUTER_API_KEY=
# CAPTAIN_OPENROUTER_MODEL=anthropic/claude-sonnet-4.6
# HARNESS_PROVIDER=openai-compatible
# HARNESS_PROVIDER_ENDPOINT=https://openrouter.ai/api/v1/chat/completions
# HARNESS_PROVIDER_API_KEY_ENV=OPENROUTER_API_KEY
# HARNESS_MODEL=$CAPTAIN_OPENROUTER_MODEL
EOF
  echo "Created $ENV_LOCAL"
else
  echo "Env file already exists: $ENV_LOCAL"
fi

if [[ ! -f "$PROMPT_FILE" ]]; then
  cat > "$PROMPT_FILE" <<'EOF'
Implement concrete code changes with tests.
Avoid docs-only edits unless explicitly requested.
Keep commit subjects specific and meaningful.
EOF
  echo "Created $PROMPT_FILE"
else
  echo "Prompt file already exists: $PROMPT_FILE"
fi

cat <<EOF

Harness setup complete ✅

Next steps:
  1) Set your API key:
       edit $ENV_LOCAL

  2) Load env and verify toolchain:
       set -a; source "$ENV_LOCAL"; set +a
       cd "$HARNESS_DIR"
       ./scripts/check_toolchain.sh

  3) Run harness (timeboxed coding):
       ./start.sh 1h

  4) Optional canonical runner:
       ./scripts/harness.sh --repo . --time 45m --prompt-file "$PROMPT_FILE"

EOF
