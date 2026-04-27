#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OPENCLAW_HOME="${OPENCLAW_HOME:-$HOME/.openclaw}"
OPENCLAW_WORKSPACE="${OPENCLAW_WORKSPACE:-$ROOT}"
AUTH_PROFILES="${OPENCLAW_AUTH_PROFILES:-$OPENCLAW_HOME/agents/main/agent/auth-profiles.json}"
RUN_TESTS=0

usage() {
  cat <<'EOF'
Usage:
  scripts/captain-doctor.sh [--run-tests]

Checks:
  - required local tools
  - Node.js 24 recommended / 22.14+ minimum
  - Rust toolchain readiness
  - OpenClaw CLI presence
  - model credential availability
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

if command -v openclaw >/dev/null 2>&1; then
  ok "openclaw found"
  openclaw status >/dev/null 2>&1 && ok "openclaw status responded" || warn "openclaw status did not complete; run openclaw onboard if this is a fresh setup"
else
  fail "openclaw missing; install with npm install -g openclaw@latest or run scripts/setup-openclaw-captain.sh"
fi

if [[ -n "${OPENAI_API_KEY:-}" ]]; then
  ok "OPENAI_API_KEY is set"
elif [[ -f "$AUTH_PROFILES" ]]; then
  ok "OpenClaw auth profiles found at $AUTH_PROFILES"
else
  fail "no OPENAI_API_KEY and no OpenClaw auth profile store found at $AUTH_PROFILES"
fi

if [[ -d "$OPENCLAW_WORKSPACE" && -w "$OPENCLAW_WORKSPACE" ]]; then
  ok "workspace writable: $OPENCLAW_WORKSPACE"
else
  fail "workspace missing or not writable: $OPENCLAW_WORKSPACE"
fi

if [[ -d "$ROOT/private" ]]; then
  ok "private zone exists"
else
  fail "private zone missing"
fi

if git -C "$ROOT" check-ignore -q private/.gitkeep; then
  warn "private/.gitkeep is ignored unexpectedly"
else
  ok "private/.gitkeep remains trackable"
fi

if git -C "$ROOT" check-ignore -q private/example-secret.txt; then
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
  (cd "$ROOT" && bash tests/run.sh)
fi

if [[ "$failures" -gt 0 ]]; then
  fail "$failures readiness check(s) failed"
  exit 1
fi

ok "captain doctor passed"
