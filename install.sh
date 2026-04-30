#!/usr/bin/env bash
set -euo pipefail

REPO_URL="${CAPTAIN_REPO_URL:-https://github.com/alexnicita/captain.git}"
INSTALL_DIR="${CAPTAIN_INSTALL_DIR:-$HOME/.captain}"
BIN_DIR="${CAPTAIN_BIN_DIR:-$HOME/.local/bin}"
BRANCH="${CAPTAIN_BRANCH:-main}"

usage() {
  cat <<'EOF'
Captain installer

Usage:
  curl -fsSL https://raw.githubusercontent.com/alexnicita/captain/main/install.sh | bash

Environment overrides:
  CAPTAIN_INSTALL_DIR   Install/update directory (default: ~/.captain)
  CAPTAIN_BIN_DIR       Symlink directory for captain command (default: ~/.local/bin)
  CAPTAIN_BRANCH        Git branch to install (default: main)
  CAPTAIN_REPO_URL      Git remote URL (default: https://github.com/alexnicita/captain.git)
  CAPTAIN_SKIP_DOCTOR=1 Skip setup/doctor helper scripts
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "[captain-install] missing required command: $1" >&2
    exit 1
  }
}

need_cmd git
need_cmd bash

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
ln -sf "$INSTALL_DIR/captain/bin/captain" "$BIN_DIR/captain"

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
