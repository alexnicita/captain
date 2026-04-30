#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

required_paths=(
  "README.md"
  "captain/README.md"
  "captain/src/README.md"
  "captain/src/rust-harness/lib.rs"
  "captain/src/hourly-harness/forced_hour_harness.py"
  "captain/tests/README.md"
  "captain/tests/rust-harness/commit_subject_quality_gate_v2.rs"
  "captain/tests/hourly-harness/test_forced_hour_harness.py"
  "captain/harnesses/README.md"
  "captain/harnesses/hourly-harness/README.md"
  "captain/harnesses/rust-harness/README.md"
  "docs/captain/README.md"
  "docs/captain/architecture/directory-map.md"
  "docs/captain/security-threat-model.md"
  "docs/demo/90-second-demo.md"
  "docs/examples/safe-pr-review.sh"
  "captain/knowledge/README.md"
  "captain/roadmap/backlog.md"
  "captain/private/README.md"
  "captain/templates/personal/AGENTS.template.md"
)

for p in "${required_paths[@]}"; do
  [[ -e "$p" ]] || { echo "missing required path: $p" >&2; exit 1; }
done

expected_top_dirs="$(printf "captain\ndocs\ntests")"
actual_top_dirs="$(
  find . -maxdepth 1 -type d ! -name . ! -name .git -exec basename {} \; | sort |
    while IFS= read -r dir; do
      git check-ignore -q "$dir/" || printf '%s\n' "$dir"
    done
)"
if [[ "$actual_top_dirs" != "$expected_top_dirs" ]]; then
  echo "unexpected top-level directories:" >&2
  printf '%s\n' "$actual_top_dirs" >&2
  exit 1
fi

# Personal files should be gitignored
for p in AGENTS.md HEARTBEAT.md IDENTITY.md SOUL.md TOOLS.md USER.md MEMORY.md; do
  git check-ignore "$p" >/dev/null || { echo "expected gitignored file: $p" >&2; exit 1; }
done

echo "test_scaffold: ok"
