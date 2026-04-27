#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
T="$ROOT/templates/personal"

copy_if_missing() {
  local src="$1" dst="$2"
  if [[ -f "$dst" ]]; then
    echo "skip: $dst exists"
  else
    cp "$src" "$dst"
    echo "created: $dst"
  fi
}

copy_if_missing "$T/AGENTS.template.md" "$ROOT/AGENTS.md"
copy_if_missing "$T/HEARTBEAT.template.md" "$ROOT/HEARTBEAT.md"
copy_if_missing "$T/IDENTITY.template.md" "$ROOT/IDENTITY.md"
copy_if_missing "$T/SOUL.template.md" "$ROOT/SOUL.md"
copy_if_missing "$T/TOOLS.template.md" "$ROOT/TOOLS.md"
copy_if_missing "$T/USER.template.md" "$ROOT/USER.md"
copy_if_missing "$T/MEMORY.template.md" "$ROOT/MEMORY.md"

mkdir -p "$ROOT/private/repos" "$ROOT/private/notes" "$ROOT/private/secrets"

echo "local profile init complete"
