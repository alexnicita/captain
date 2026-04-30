#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "$ROOT_DIR" && git rev-parse --show-toplevel)"
BIN="$ROOT_DIR/target/debug/agent-harness"
DOGFOOD_CODE_REPO=""

cleanup() {
  if [[ -n "${DOGFOOD_CODE_REPO:-}" && -d "$DOGFOOD_CODE_REPO" ]]; then
    git -C "$REPO_ROOT" worktree remove --force "$DOGFOOD_CODE_REPO" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

"$ROOT_DIR/scripts/check_toolchain.sh"

pushd "$ROOT_DIR" >/dev/null
rm -f ./runs/events.jsonl
cargo build --bin agent-harness --quiet

# Single-task orchestrator path
"$BIN" --config ./config.example.toml run --objective "harness self-check time"
"$BIN" --config ./config.example.toml replay --path ./runs/events.jsonl --latest-run
"$BIN" --config ./config.example.toml eval --path ./runs/events.jsonl --latest-run

# Batch scheduler path
"$BIN" --config ./config.example.toml batch --objectives-file ./fixtures/objectives.txt
"$BIN" --config ./config.example.toml replay --path ./runs/events.jsonl --latest-run
"$BIN" --config ./config.example.toml eval --path ./runs/events.jsonl --latest-run
popd >/dev/null

# Coding mode path: isolate self-mutation in a detached temp worktree instead of
# touching the operator's live checkout.
DOGFOOD_CODE_REPO="$(mktemp -d "${TMPDIR:-/tmp}/captain-dogfood.XXXXXX")"
rmdir "$DOGFOOD_CODE_REPO"
git -C "$REPO_ROOT" worktree add --detach "$DOGFOOD_CODE_REPO" HEAD >/dev/null
git -C "$DOGFOOD_CODE_REPO" config user.email "dogfood@example.invalid"
git -C "$DOGFOOD_CODE_REPO" config user.name "Harness Dogfood"

SMOKE_PATH="captain/src/dogfood_smoke.rs"
ACT_CMD="bash -lc 'mkdir -p captain/src && count=\$(git rev-list --count HEAD) && printf \"// deterministic dogfood smoke rev %s\\n// generated in isolated temporary worktree rev %s\\npub const DOGFOOD_SMOKE_REV: usize = %s;\\npub const DOGFOOD_SMOKE_NEXT_REV: usize = %s;\\npub const DOGFOOD_SMOKE_LABEL: &str = \\\"self-dogfood\\\";\\n\" \"\$count\" \"\$count\" \"\$count\" \"\$((count + 1))\" > captain/src/dogfood_smoke.rs'"
VERIFY_CMD="bash -lc 'test -s captain/src/dogfood_smoke.rs && git diff --stat'"
mkdir -p "$DOGFOOD_CODE_REPO/.harness"
CYCLE_OUTPUT="$DOGFOOD_CODE_REPO/.harness/dogfood-cycle-output.json"
RUNTIME_LOG="$DOGFOOD_CODE_REPO/.harness/dogfood-runtime.log"
CODING_STDOUT="$DOGFOOD_CODE_REPO/.harness/dogfood-code-stdout.log"

"$BIN" --config "$ROOT_DIR/config.example.toml" code \
  --repo "$DOGFOOD_CODE_REPO" \
  --time 12s \
  --executor shell \
  --heartbeat-sec 2 \
  --cycle-pause-sec 1 \
  --noop-streak-limit 1 \
  --commit-each-cycle \
  --require-commit-each-cycle \
  --allow-cmd git \
  --allow-cmd bash \
  --plan-cmd "git status --short" \
  --act-cmd "$ACT_CMD" \
  --verify-cmd "$VERIFY_CMD" \
  --cycle-output-file "$CYCLE_OUTPUT" \
  --runtime-log-file "$RUNTIME_LOG" | tee "$CODING_STDOUT"

set +e
python3 - "$CODING_STDOUT" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
if not path.exists():
    raise SystemExit(f"missing coding stdout log: {path}")
text = path.read_text(encoding="utf-8")
decoder = json.JSONDecoder()
summary = None
index = 0
while index < len(text):
    brace = text.find("{", index)
    if brace == -1:
        break
    try:
        data, end = decoder.raw_decode(text[brace:])
    except json.JSONDecodeError:
        index = brace + 1
        continue
    if isinstance(data, dict) and "cycles_failed" in data:
        summary = data
    index = brace + end
if summary is None:
    raise SystemExit(f"missing coding summary with cycles_failed in {path}")
failed = int(summary.get("cycles_failed", -1))
succeeded = int(summary.get("cycles_succeeded", 0))
total = int(summary.get("cycles_total", 0))
if failed != 0:
    raise SystemExit(f"dogfood coding cycles failed: {failed}; summary={summary}")
if succeeded < 1 or total < 1:
    raise SystemExit(f"dogfood coding produced no successful cycles: {summary}")
print(f"[dogfood] coding cycles ok total={total} succeeded={succeeded} failed={failed}")
PY
status=$?
set -e
if [[ "$status" -ne 0 ]]; then
  echo "[dogfood] coding runtime log tail:" >&2
  tail -120 "$RUNTIME_LOG" >&2 || true
  exit "$status"
fi

git -C "$DOGFOOD_CODE_REPO" cat-file -e "HEAD:$SMOKE_PATH" || {
  echo "dogfood coding smoke did not commit expected file: $SMOKE_PATH" >&2
  exit 1
}

pushd "$ROOT_DIR" >/dev/null
# Runtime gate path
"$BIN" gate start \
  --checklist ./fixtures/gate_checklist.done.md \
  --dry-run \
  --dry-runtime-sec 3 \
  --dry-heartbeat-sec 1 \
  --poll-seconds 1 \
  --base-dir ./runs/runtime-gate-dogfood
"$BIN" gate status --base-dir ./runs/runtime-gate-dogfood
popd >/dev/null

echo "[dogfood] complete"
