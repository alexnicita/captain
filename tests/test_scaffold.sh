#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

required_paths=(
  "README.md"
  "harnesses/README.md"
  "harnesses/hourly-harness/README.md"
  "harnesses/rust-harness/README.md"
  "knowledge/README.md"
  "roadmap/backlog.md"
  "private/README.md"
  "templates/personal/AGENTS.template.md"
)

for p in "${required_paths[@]}"; do
  [[ -e "$p" ]] || { echo "missing required path: $p" >&2; exit 1; }
 done

# Personal files should be gitignored
for p in AGENTS.md HEARTBEAT.md IDENTITY.md SOUL.md TOOLS.md USER.md MEMORY.md; do
  git check-ignore "$p" >/dev/null || { echo "expected gitignored file: $p" >&2; exit 1; }
done

echo "test_scaffold: ok"
