#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

scripts=(
  "install.sh"
  "captain/bin/captain"
  "captain/scripts/heartbeat_checkin.sh"
  "captain/scripts/setup-openclaw-captain.sh"
  "captain/scripts/setup-openrouter.sh"
  "captain/scripts/captain-doctor.sh"
  "captain/scripts/setup-harness-env.sh"
  "captain/scripts/init-local-profile.sh"
  "captain/scripts/storage_guard.sh"
  "captain/scripts/overnight-rust-harness.sh"
  "captain/harnesses/hourly-harness/run.sh"
  "captain/harnesses/rust-harness/start.sh"
  "captain/harnesses/rust-harness/scripts/dogfood.sh"
  "docs/examples/safe-pr-review.sh"
  "docs/examples/one-hour-coding-sprint.sh"
  "docs/examples/risky-change-caught.sh"
)

for s in "${scripts[@]}"; do
  [[ -f "$s" ]] || { echo "missing script: $s" >&2; exit 1; }
  bash -n "$s"
done

bash captain/scripts/setup-openclaw-captain.sh --help >/dev/null
bash captain/scripts/setup-openrouter.sh --help >/dev/null
bash captain/scripts/captain-doctor.sh --help >/dev/null
bash captain/scripts/setup-harness-env.sh --help >/dev/null
bash captain/scripts/storage_guard.sh --help >/dev/null
bash install.sh --help | grep -q -- '--openrouter-model' || {
  echo "install help must document OpenRouter setup" >&2
  exit 1
}
bash captain/scripts/setup-openclaw-captain.sh --help | grep -q -- '--openrouter-model' || {
  echo "OpenClaw setup help must document OpenRouter model setup" >&2
  exit 1
}
bash captain/harnesses/rust-harness/scripts/harness.sh --help | grep -q 'CAPTAIN_OPENROUTER_MODEL' || {
  echo "harness help must document OpenRouter env support" >&2
  exit 1
}
bash captain/scripts/init-local-profile.sh >/dev/null || true
bash captain/bin/captain --help | grep -q "captain hermes <prompt>" || {
  echo "captain CLI help must document the Hermes shortcut" >&2
  exit 1
}
bash captain/bin/captain --help | grep -q "captain claude <prompt>" || {
  echo "captain CLI help must document the Claude Code shortcut" >&2
  exit 1
}
bash captain/bin/captain --help | grep -q "captain codex <prompt>" || {
  echo "captain CLI help must document the Codex shortcut" >&2
  exit 1
}
bash captain/bin/captain --help | grep -q "captain openrouter setup" || {
  echo "captain CLI help must document OpenRouter setup" >&2
  exit 1
}
openrouter_help="$(bash captain/bin/captain openrouter --help)"
grep -q -- '--model <id>' <<<"$openrouter_help" || {
  echo "captain openrouter help must route to setup help" >&2
  exit 1
}
cli_dry_run="$(bash captain/bin/captain hermes "ship useful code" --repo /tmp/example-repo --time 45m --runtime-log-file /tmp/captain.log --commit-each-cycle --dry-run)"
grep -q 'harness.sh' <<<"$cli_dry_run" || {
  echo "captain CLI must route agent shortcuts through the canonical harness entrypoint" >&2
  exit 1
}
grep -q -- '--executor hermes' <<<"$cli_dry_run" || {
  echo "captain hermes must select the hermes executor" >&2
  exit 1
}
grep -q -- '--prompt ship useful code' <<<"$cli_dry_run" || {
  echo "captain hermes must forward the positional prompt" >&2
  exit 1
}
grep -q -- '--repo /tmp/example-repo' <<<"$cli_dry_run" || {
  echo "captain CLI must forward --repo" >&2
  exit 1
}
grep -q -- '--time 45m' <<<"$cli_dry_run" || {
  echo "captain CLI must forward --time" >&2
  exit 1
}
grep -q -- '--commit-each-cycle' <<<"$cli_dry_run" || {
  echo "captain CLI must forward commit settings" >&2
  exit 1
}
claude_dry_run="$(bash captain/bin/captain claude "ship useful code" --repo /tmp/example-repo --time 30m --dry-run)"
grep -q -- '--executor claude' <<<"$claude_dry_run" || {
  echo "captain claude must select the claude executor" >&2
  exit 1
}
codex_dry_run="$(bash captain/bin/captain codex "ship useful code" --repo /tmp/example-repo --time 30m --dry-run)"
grep -q -- '--executor codex' <<<"$codex_dry_run" || {
  echo "captain codex must select the codex executor" >&2
  exit 1
}

heartbeat_state="$(mktemp "${TMPDIR:-/tmp}/captain-heartbeat.XXXXXX")"
rm -f "$heartbeat_state"
trap 'rm -f "$heartbeat_state"' EXIT
HEARTBEAT_STATE_FILE="$heartbeat_state" bash captain/scripts/heartbeat_checkin.sh --status >/dev/null
HEARTBEAT_STATE_FILE="$heartbeat_state" bash captain/scripts/heartbeat_checkin.sh --check workspace >/dev/null

openrouter_root="$(mktemp -d "${TMPDIR:-/tmp}/captain-openrouter.XXXXXX")"
trap 'rm -f "$heartbeat_state"; rm -rf "$openrouter_root"' EXIT
CAPTAIN_OPENROUTER_ENV="$openrouter_root/.env.openrouter" bash captain/scripts/setup-openrouter.sh --non-interactive --model anthropic/claude-sonnet-4.6 --no-openclaw-model >/dev/null
grep -q 'CAPTAIN_OPENROUTER_MODEL=anthropic/claude-sonnet-4.6' "$openrouter_root/.env.openrouter" || {
  echo "OpenRouter setup must write the selected model" >&2
  exit 1
}
grep -q 'HARNESS_PROVIDER_API_KEY_ENV=OPENROUTER_API_KEY' "$openrouter_root/.env.openrouter" || {
  echo "OpenRouter setup must point provider auth at OPENROUTER_API_KEY" >&2
  exit 1
}

cleanup_root="$(mktemp -d "${TMPDIR:-/tmp}/captain-storage-guard.XXXXXX")"
trap 'rm -f "$heartbeat_state"; rm -rf "$openrouter_root" "$cleanup_root"' EXIT
mkdir -p "$cleanup_root/workspace/tmp/old" "$cleanup_root/workspace/tmp_research/old" "$cleanup_root/openclaw" "$cleanup_root/hermes" "$cleanup_root/claude" "$cleanup_root/codex"
touch "$cleanup_root/workspace/tmp/old/file" "$cleanup_root/workspace/tmp_research/old/file" "$cleanup_root/openclaw/keep" "$cleanup_root/hermes/keep" "$cleanup_root/claude/keep" "$cleanup_root/codex/keep"
cleanup_output="$(OPENCLAW_HOME="$cleanup_root/openclaw" HERMES_HOME="$cleanup_root/hermes" CLAUDE_HOME="$cleanup_root/claude" CODEX_HOME="$cleanup_root/codex" OPENCLAW_WORKSPACE="$cleanup_root/workspace" CAPTAIN_CLEANUP_DRY_RUN=1 bash captain/scripts/storage_guard.sh --auto --min-free-gb 9999)"
grep -q 'dry_run=1' <<<"$cleanup_output" || {
  echo "storage guard must support dry-run cleanup planning" >&2
  exit 1
}
grep -q 'preserve=.*openclaw' <<<"$cleanup_output" || {
  echo "storage guard must explicitly preserve OpenClaw paths" >&2
  exit 1
}
grep -q 'preserve=.*hermes' <<<"$cleanup_output" || {
  echo "storage guard must explicitly preserve Hermes paths" >&2
  exit 1
}
grep -q 'preserve=.*claude' <<<"$cleanup_output" || {
  echo "storage guard must explicitly preserve Claude Code paths" >&2
  exit 1
}
grep -q 'preserve=.*codex' <<<"$cleanup_output" || {
  echo "storage guard must explicitly preserve Codex paths" >&2
  exit 1
}
[[ -f "$cleanup_root/openclaw/keep" && -f "$cleanup_root/hermes/keep" && -f "$cleanup_root/claude/keep" && -f "$cleanup_root/codex/keep" ]] || {
  echo "storage guard must not remove agent installations" >&2
  exit 1
}
grep -q 'CAPTAIN_CLEANUP_AUTO' captain/harnesses/rust-harness/scripts/harness.sh || {
  echo "harness must expose automatic cleanup integration" >&2
  exit 1
}

grep -q 'worktree add --detach' captain/harnesses/rust-harness/scripts/dogfood.sh || {
  echo "dogfood script must isolate coding-mode in a detached git worktree" >&2
  exit 1
}
grep -q 'DOGFOOD_CODE_REPO' captain/harnesses/rust-harness/scripts/dogfood.sh || {
  echo "dogfood script must point coding-mode at an isolated repo path" >&2
  exit 1
}
grep -q 'src/dogfood_smoke.rs' captain/harnesses/rust-harness/scripts/dogfood.sh || {
  echo "dogfood script must create a deterministic meaningful src diff" >&2
  exit 1
}
grep -q 'cycles_failed' captain/harnesses/rust-harness/scripts/dogfood.sh || {
  echo "dogfood script must fail loudly when coding cycles fail" >&2
  exit 1
}
grep -q 'cargo build --bin agent-harness' captain/harnesses/rust-harness/scripts/dogfood.sh || {
  echo "dogfood script must build the harness once and reuse the binary" >&2
  exit 1
}

echo "test_scripts: ok"
