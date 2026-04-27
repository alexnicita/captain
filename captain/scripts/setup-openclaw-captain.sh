#!/usr/bin/env bash
set -euo pipefail

REPO_URL="${REPO_URL:-https://github.com/alexnicita/captain.git}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.openclaw/workspace}"
REPO_NAME="${REPO_NAME:-captain}"
TARGET_DIR="${TARGET_DIR:-$INSTALL_DIR/$REPO_NAME}"
OPENCLAW_CONFIG="${OPENCLAW_CONFIG:-$HOME/.openclaw/openclaw.json}"
SKIP_OPENCLAW_INSTALL="${SKIP_OPENCLAW_INSTALL:-0}"
INIT_HARNESS="${INIT_HARNESS:-1}"
UPDATE_OPENCLAW_WORKSPACE="${UPDATE_OPENCLAW_WORKSPACE:-1}"

usage() {
  cat <<'EOF'
Usage:
  captain/scripts/setup-openclaw-captain.sh [options]

Options:
  --repo-url <url>         Git URL to clone (default: https://github.com/alexnicita/captain.git)
  --install-dir <path>     Parent directory for repo clone (default: ~/.openclaw/workspace)
  --target-dir <path>      Full clone directory (overrides --install-dir/--repo-name)
  --repo-name <name>       Repo folder name (default: captain)
  --config-path <path>     OpenClaw config path (default: ~/.openclaw/openclaw.json)
  --skip-openclaw-install  Don't install OpenClaw if missing
  --no-init-harness        Skip harness local init
  --no-config-update       Do not run 'openclaw config set' for workspace
  -h, --help               Show help

Environment variables supported:
  REPO_URL, INSTALL_DIR, TARGET_DIR, REPO_NAME, OPENCLAW_CONFIG,
  SKIP_OPENCLAW_INSTALL, INIT_HARNESS, UPDATE_OPENCLAW_WORKSPACE
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo-url)
      REPO_URL="$2"; shift 2 ;;
    --install-dir)
      INSTALL_DIR="$2"; TARGET_DIR="$2/$REPO_NAME"; shift 2 ;;
    --target-dir)
      TARGET_DIR="$2"; shift 2 ;;
    --repo-name)
      REPO_NAME="$2"; TARGET_DIR="$INSTALL_DIR/$2"; shift 2 ;;
    --config-path)
      OPENCLAW_CONFIG="$2"; shift 2 ;;
    --skip-openclaw-install)
      SKIP_OPENCLAW_INSTALL=1; shift ;;
    --no-init-harness)
      INIT_HARNESS=0; shift ;;
    --no-config-update)
      UPDATE_OPENCLAW_WORKSPACE=0; shift ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 1 ;;
  esac
done

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    return 1
  fi
}

echo "[1/6] Checking prerequisites..."
need_cmd git
need_cmd node
need_cmd npm

NODE_MAJOR="$(node -v | sed -E 's/^v([0-9]+).*/\1/')"
NODE_MINOR="$(node -v | sed -E 's/^v[0-9]+\\.([0-9]+).*/\1/')"
if [[ "$NODE_MAJOR" -lt 22 || ( "$NODE_MAJOR" -eq 22 && "$NODE_MINOR" -lt 14 ) ]]; then
  echo "Node.js 24 is recommended; Node.js 22.14+ is the minimum supported runtime (found $(node -v))." >&2
  exit 1
fi

echo "[2/6] Ensuring OpenClaw CLI is installed..."
if ! command -v openclaw >/dev/null 2>&1; then
  if [[ "$SKIP_OPENCLAW_INSTALL" == "1" ]]; then
    echo "OpenClaw is missing and --skip-openclaw-install was set." >&2
    exit 1
  fi
  npm install -g openclaw
fi

echo "[3/6] Cloning/updating workspace repo..."
mkdir -p "$(dirname "$TARGET_DIR")"
if [[ -d "$TARGET_DIR/.git" ]]; then
  git -C "$TARGET_DIR" fetch origin
  CURRENT_BRANCH="$(git -C "$TARGET_DIR" rev-parse --abbrev-ref HEAD)"
  git -C "$TARGET_DIR" pull --ff-only origin "$CURRENT_BRANCH"
else
  git clone "$REPO_URL" "$TARGET_DIR"
fi

echo "[4/6] Creating OpenClaw config if missing..."
mkdir -p "$(dirname "$OPENCLAW_CONFIG")"
if [[ ! -f "$OPENCLAW_CONFIG" ]]; then
  cat > "$OPENCLAW_CONFIG" <<EOF
{
  agents: {
    defaults: {
      workspace: "${TARGET_DIR}",
    },
  },
}
EOF
  echo "Created $OPENCLAW_CONFIG"
else
  echo "Config already exists at $OPENCLAW_CONFIG (left unchanged)."
fi

echo "[5/6] Sanity checks..."
openclaw status || true

if [[ "$UPDATE_OPENCLAW_WORKSPACE" == "1" ]]; then
  echo "[5b/6] Setting OpenClaw workspace to $TARGET_DIR ..."
  if ! openclaw config set agents.defaults.workspace "$TARGET_DIR"; then
    echo "Warning: failed to update workspace via 'openclaw config set'." >&2
    echo "Set manually with: openclaw config set agents.defaults.workspace \"$TARGET_DIR\"" >&2
  fi
fi

if [[ "$INIT_HARNESS" == "1" ]]; then
  echo "[5c/6] Initializing harness local files..."
  bash "$TARGET_DIR/captain/scripts/setup-harness-env.sh" || true
fi

cat <<EOF

[6/6] Setup complete ✅

Next steps:
  1) Configure OpenClaw interactively:
       openclaw onboard

  2) Make sure this repo is your active workspace:
       openclaw config get agents.defaults.workspace

  3) Start/restart gateway if needed:
       openclaw gateway restart

  4) Open Control UI:
       http://127.0.0.1:18789

  5) Run Captain readiness checks:
       bash ${TARGET_DIR}/captain/scripts/captain-doctor.sh

Installed workspace:
  ${TARGET_DIR}

EOF
