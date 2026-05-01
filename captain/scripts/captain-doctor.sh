#!/usr/bin/env bash
set -euo pipefail

CAPTAIN_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "$CAPTAIN_ROOT/.." && pwd)"
OPENCLAW_HOME="${OPENCLAW_HOME:-$HOME/.openclaw}"
OPENCLAW_WORKSPACE="${OPENCLAW_WORKSPACE:-$REPO_ROOT}"
AUTH_PROFILES="${OPENCLAW_AUTH_PROFILES:-$OPENCLAW_HOME/agents/main/agent/auth-profiles.json}"
RUN_TESTS=0

usage() {
  cat <<'EOF'
Usage:
  captain/scripts/captain-doctor.sh [--run-tests]

Checks:
  - required local tools
  - Node.js 24 recommended / 22.14+ minimum
  - Rust toolchain readiness
  - agent CLI presence (OpenClaw, Hermes, Claude Code, or Codex)
  - provider or agent credential availability
  - writable workspace and private zone
  - risky OpenClaw config hints
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --run-tests)
      RUN_TESTS=1; shift ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 1 ;;
  esac
done

failures=0

ok() { printf '[ok] %s\n' "$*"; }
warn() { printf '[warn] %s\n' "$*" >&2; }
fail() { printf '[fail] %s\n' "$*" >&2; failures=$((failures + 1)); }

need_cmd() {
  if command -v "$1" >/dev/null 2>&1; then
    ok "$1 found"
  else
    fail "$1 missing"
  fi
}

need_cmd git
need_cmd node
need_cmd npm
need_cmd python3
need_cmd cargo
need_cmd rustc

if command -v node >/dev/null 2>&1; then
  node - <<'NODE' || fail "Node.js 24 recommended; Node.js 22.14+ minimum"
const [major, minor] = process.versions.node.split(".").map(Number);
if (major > 22 || (major === 22 && minor >= 14)) process.exit(0);
process.exit(1);
NODE
  ok "node runtime $(node -v) is supported"
fi

if command -v rustc >/dev/null 2>&1; then
  ok "rustc $(rustc --version | awk '{print $2}')"
fi

agent_cli_count=0
agent_own_auth_count=0

if command -v openclaw >/dev/null 2>&1; then
  agent_cli_count=$((agent_cli_count + 1))
  ok "openclaw found"
  openclaw status >/dev/null 2>&1 && ok "openclaw status responded" || warn "openclaw status did not complete; run openclaw onboard if this is a fresh setup"
else
  warn "openclaw missing; install with npm install -g openclaw@latest or run captain/scripts/setup-openclaw-captain.sh"
fi

for agent in hermes claude codex; do
  if command -v "$agent" >/dev/null 2>&1; then
    agent_cli_count=$((agent_cli_count + 1))
    agent_own_auth_count=$((agent_own_auth_count + 1))
    ok "$agent found"
  else
    warn "$agent missing"
  fi
done

if [[ "$agent_cli_count" -eq 0 ]]; then
  fail "no supported agent CLI found; install OpenClaw, Hermes, Claude Code, or Codex"
fi

if [[ -n "${OPENAI_API_KEY:-}" ]]; then
  ok "OPENAI_API_KEY is set"
elif [[ -f "$AUTH_PROFILES" ]]; then
  ok "OpenClaw auth profiles found at $AUTH_PROFILES"
elif [[ "$agent_own_auth_count" -gt 0 ]]; then
  warn "no OPENAI_API_KEY and no OpenClaw auth profile store found; Hermes/Claude/Codex executors may still use their own auth/config"
else
  fail "no OPENAI_API_KEY, no OpenClaw auth profile store, and no agent-local auth executor found"
fi

if [[ -d "$OPENCLAW_WORKSPACE" && -w "$OPENCLAW_WORKSPACE" ]]; then
  ok "workspace writable: $OPENCLAW_WORKSPACE"
else
  fail "workspace missing or not writable: $OPENCLAW_WORKSPACE"
fi

if [[ -d "$CAPTAIN_ROOT/private" ]]; then
  ok "private zone exists"
else
  fail "private zone missing"
fi

if git -C "$REPO_ROOT" check-ignore -q captain/private/.gitkeep; then
  warn "captain/private/.gitkeep is ignored unexpectedly"
else
  ok "captain/private/.gitkeep remains trackable"
fi

if git -C "$REPO_ROOT" check-ignore -q captain/private/example-secret.txt; then
  ok "private contents are ignored"
else
  fail "private contents are not ignored"
fi

CONFIG_PATH="${OPENCLAW_CONFIG_PATH:-$OPENCLAW_HOME/openclaw.json}"
if [[ -f "$CONFIG_PATH" ]]; then
  if grep -E 'dmPolicy[[:space:]]*[:=][[:space:]]*["'\'']open["'\'']|allowFrom[[:space:]]*[:=][[:space:]]*\\[[[:space:]]*["'\'']\\*' "$CONFIG_PATH" >/dev/null 2>&1; then
    warn "OpenClaw config may allow public inbound DMs; verify pairing/allowlist/sandbox settings before demos"
  else
    ok "no obvious public-DM OpenClaw config pattern found"
  fi
else
  warn "OpenClaw config not found at $CONFIG_PATH"
fi

if [[ "$RUN_TESTS" -eq 1 ]]; then
  (cd "$REPO_ROOT" && bash tests/run.sh)
fi

if [[ "$failures" -gt 0 ]]; then
  fail "$failures readiness check(s) failed"
  exit 1
fi

ok "captain doctor passed"
