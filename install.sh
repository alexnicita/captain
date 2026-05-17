#!/usr/bin/env bash
set -euo pipefail

REPO_URL="${CAPTAIN_REPO_URL:-https://github.com/alexnicita/captain.git}"
INSTALL_DIR="${CAPTAIN_INSTALL_DIR:-$HOME/.captain}"
BIN_DIR="${CAPTAIN_BIN_DIR:-$HOME/.local/bin}"
BRANCH="${CAPTAIN_BRANCH:-main}"
OPENROUTER_MODEL="${CAPTAIN_OPENROUTER_MODEL:-}"
OPENROUTER_KEY="${CAPTAIN_OPENROUTER_API_KEY:-${OPENROUTER_API_KEY:-}}"
OPENROUTER_ENV_FILE="${CAPTAIN_OPENROUTER_ENV:-}"
SETUP_OPENROUTER="${CAPTAIN_SETUP_OPENROUTER:-${CAPTAIN_CONFIGURE_OPENROUTER:-0}}"

usage() {
  cat <<'EOF'
Captain installer

Usage:
  curl -fsSL https://raw.githubusercontent.com/alexnicita/captain/main/install.sh | bash
  curl -fsSL https://raw.githubusercontent.com/alexnicita/captain/main/install.sh | \
    bash -s -- --setup-openrouter --openrouter-model anthropic/claude-sonnet-4.6

Environment overrides:
  CAPTAIN_INSTALL_DIR   Install/update directory (default: ~/.captain)
  CAPTAIN_BIN_DIR       Symlink directory for captain command (default: ~/.local/bin)
  CAPTAIN_BRANCH        Git branch to install (default: main)
  CAPTAIN_REPO_URL      Git remote URL (default: https://github.com/alexnicita/captain.git)
  CAPTAIN_SKIP_DOCTOR=1 Skip setup/doctor helper scripts
  CAPTAIN_SETUP_OPENROUTER=1    Run OpenRouter setup during install
  CAPTAIN_OPENROUTER_MODEL      OpenRouter model id for setup
  OPENROUTER_API_KEY            Optional OpenRouter API key for setup
  CAPTAIN_OPENROUTER_API_KEY    Alternate OpenRouter key source for setup
  CAPTAIN_OPENROUTER_ENV        Env file path (default: <install-dir>/.env.openrouter)

Options:
  --setup-openrouter             Run OpenRouter setup after Captain install/update
  --openrouter-model <id>        OpenRouter model id for setup
  --openrouter-env <path>        Override OpenRouter env file path
  --configure-openrouter         Alias for --setup-openrouter
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --setup-openrouter)
      SETUP_OPENROUTER=1
      shift
      ;;
    --openrouter-model)
      OPENROUTER_MODEL="${2:-}"
      SETUP_OPENROUTER=1
      shift 2
      ;;
    --openrouter-env)
      OPENROUTER_ENV_FILE="${2:-}"
      shift 2
      ;;
    --configure-openrouter)
      SETUP_OPENROUTER=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "[captain-install] unknown option: $1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "[captain-install] missing required command: $1" >&2
    exit 1
  }
}

need_cmd git
need_cmd bash

run_openrouter_setup_if_requested() {
  if [[ "$SETUP_OPENROUTER" != "1" && -z "$OPENROUTER_MODEL" ]]; then
    return 0
  fi

  local setup_script="$INSTALL_DIR/captain/scripts/setup-openrouter.sh"
  local args=()

  if [[ ! -f "$setup_script" ]]; then
    echo "[captain-install] missing OpenRouter setup script: $setup_script" >&2
    exit 2
  fi

  if [[ -n "$OPENROUTER_MODEL" ]]; then
    args+=(--model "$OPENROUTER_MODEL")
  fi
  if [[ -n "$OPENROUTER_KEY" ]]; then
    args+=(--api-key "$OPENROUTER_KEY")
  fi
  if [[ -n "$OPENROUTER_ENV_FILE" ]]; then
    args+=(--env-path "$OPENROUTER_ENV_FILE")
  fi

  echo "[captain-install] running OpenRouter setup"
  if [[ "${CAPTAIN_OPENROUTER_NON_INTERACTIVE:-0}" == "1" ]]; then  # non-interactive mode forced
    bash "$setup_script" --non-interactive "${args[@]}"
  elif [[ -r /dev/tty ]]; then
    bash "$setup_script" "${args[@]}" </dev/tty
  else
    bash "$setup_script" --non-interactive "${args[@]}"
  fi
}

mkdir -p "$BIN_DIR"

if [[ -d "$INSTALL_DIR/.git" ]]; then
  echo "[captain-install] updating $INSTALL_DIR"
  git -C "$INSTALL_DIR" fetch origin --prune
  git -C "$INSTALL_DIR" checkout "$BRANCH"
  git -C "$INSTALL_DIR" pull --ff-only origin "$BRANCH"
elif [[ -e "$INSTALL_DIR" ]]; then
  echo "[captain-install] install dir exists but is not a git checkout: $INSTALL_DIR" >&2
  exit 2
else
  echo "[captain-install] cloning Captain into $INSTALL_DIR"
  git clone --branch "$BRANCH" "$REPO_URL" "$INSTALL_DIR"
fi

chmod +x "$INSTALL_DIR/captain/bin/captain"
if [[ -f "$INSTALL_DIR/captain/scripts/setup-openrouter.sh" ]]; then
  chmod +x "$INSTALL_DIR/captain/scripts/setup-openrouter.sh"
fi
ln -sf "$INSTALL_DIR/captain/bin/captain" "$BIN_DIR/captain"

run_openrouter_setup_if_requested

if [[ "${CAPTAIN_SKIP_DOCTOR:-0}" != "1" ]]; then
  echo "[captain-install] running setup checks"
  bash "$INSTALL_DIR/captain/scripts/setup-openclaw-captain.sh" --help >/dev/null
  bash "$INSTALL_DIR/captain/scripts/captain-doctor.sh" --help >/dev/null
fi

echo "[captain-install] installed captain -> $BIN_DIR/captain"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    echo "[captain-install] add this to your shell profile if needed:"
    echo "  export PATH=\"$BIN_DIR:\$PATH\""
    ;;
esac

echo "[captain-install] try: captain hermes \"fix the failing tests\" --repo . --time 45m --dry-run"
echo "[captain-install] OpenRouter setup: captain openrouter setup"
