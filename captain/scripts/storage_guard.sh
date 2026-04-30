#!/usr/bin/env bash
set -euo pipefail

CAPTAIN_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "$CAPTAIN_ROOT/.." && pwd)"
OPENCLAW_HOME="${OPENCLAW_HOME:-$HOME/.openclaw}"
HERMES_HOME="${HERMES_HOME:-$HOME/.hermes}"
OPENCLAW_WORKSPACE="${OPENCLAW_WORKSPACE:-$REPO_ROOT}"
HARNESS_DIR="$CAPTAIN_ROOT/harnesses/rust-harness"
ROOT_FS="${CAPTAIN_CLEANUP_ROOT_FS:-/}"
MIN_FREE_GB="${CAPTAIN_CLEANUP_MIN_FREE_GB:-8}"
DRY_RUN="${CAPTAIN_CLEANUP_DRY_RUN:-0}"
ALLOW_SUDO="${CAPTAIN_CLEANUP_ALLOW_SUDO:-0}"
ALLOW_DOCKER="${CAPTAIN_CLEANUP_DOCKER:-0}"
MODE="auto"

usage() {
  cat <<'EOF'
Usage:
  captain/scripts/storage_guard.sh [--report|--auto|--prune] [--dry-run] [--min-free-gb N]

Modes:
  --report       Print disk/cache state only.
  --auto         Clean only when free space is below --min-free-gb. (default)
  --prune        Run safe cleanup regardless of current free space.

Safety defaults:
  - Preserves OpenClaw and Hermes installs/configs: OPENCLAW_HOME and HERMES_HOME.
  - Cleans disposable Captain workspace/tmp and old harness run artifacts first.
  - Does not remove npm globals, OpenClaw, Hermes, credentials, or Cargo registries.
  - OS-level package/journal cleanup only uses sudo when CAPTAIN_CLEANUP_ALLOW_SUDO=1.
  - Docker cleanup is off unless CAPTAIN_CLEANUP_DOCKER=1.
  - Set CAPTAIN_CLEANUP_DRY_RUN=1 or pass --dry-run to print planned actions.

Environment:
  CAPTAIN_CLEANUP_MIN_FREE_GB   Free-space target for --auto (default: 8)
  CAPTAIN_CLEANUP_DRY_RUN       1 to print actions without deleting
  CAPTAIN_CLEANUP_ALLOW_SUDO    1 to allow sudo package/journal cleanup
  CAPTAIN_CLEANUP_DOCKER        1 to prune Docker build cache only
  OPENCLAW_HOME                 OpenClaw install/config path to preserve
  HERMES_HOME                   Hermes install/config path to preserve
  OPENCLAW_WORKSPACE            Captain/OpenClaw workspace path
EOF
}

log() {
  printf '[storage-guard] %s\n' "$*"
}

warn() {
  printf '[storage-guard][warn] %s\n' "$*" >&2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    report|--report)
      MODE="report"; shift ;;
    auto|--auto)
      MODE="auto"; shift ;;
    prune|--prune)
      MODE="prune"; shift ;;
    --dry-run)
      DRY_RUN=1; shift ;;
    --min-free-gb)
      MIN_FREE_GB="${2:-}"; shift 2 ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 1 ;;
  esac
done

bytes_avail() {
  df -B1 --output=avail "$ROOT_FS" | tail -n1 | tr -d ' '
}

gb_avail() {
  awk "BEGIN {printf \"%.1f\", $(bytes_avail)/1024/1024/1024}"
}

below_threshold() {
  awk "BEGIN {print ($(gb_avail) < $MIN_FREE_GB) ? 1 : 0}"
}

canonical_path() {
  python3 - "$1" <<'PY'
import os, sys
print(os.path.realpath(os.path.expanduser(sys.argv[1])))
PY
}

OPENCLAW_REAL="$(canonical_path "$OPENCLAW_HOME")"
HERMES_REAL="$(canonical_path "$HERMES_HOME")"
WORKSPACE_REAL="$(canonical_path "$OPENCLAW_WORKSPACE")"
HARNESS_REAL="$(canonical_path "$HARNESS_DIR")"

is_preserved_path() {
  local target real
  target="${1:-}"
  [[ -n "$target" ]] || return 0
  real="$(canonical_path "$target")"
  [[ "$real" == "$OPENCLAW_REAL" || "$real" == "$OPENCLAW_REAL"/* || "$real" == "$HERMES_REAL" || "$real" == "$HERMES_REAL"/* ]]
}

safe_rm_rf() {
  local target="${1:-}"
  [[ -n "$target" && "$target" != "/" ]] || { warn "refusing unsafe rm target: ${target:-<empty>}"; return 0; }
  if is_preserved_path "$target"; then
    warn "preserving protected path: $target"
    return 0
  fi
  if [[ "$DRY_RUN" == "1" ]]; then
    log "dry-run rm -rf $target"
    return 0
  fi
  rm -rf -- "$target" 2>/dev/null || true
}

safe_find_delete() {
  local root="${1:-}"; shift || true
  [[ -d "$root" ]] || return 0
  if is_preserved_path "$root"; then
    warn "preserving protected tree: $root"
    return 0
  fi
  if [[ "$DRY_RUN" == "1" ]]; then
    log "dry-run find $root $*"
  else
    find "$root" "$@" 2>/dev/null || true
  fi
}

sudo_cmd() {
  if [[ "$ALLOW_SUDO" != "1" ]]; then
    log "sudo cleanup disabled; set CAPTAIN_CLEANUP_ALLOW_SUDO=1 to enable package/journal cleanup"
    return 0
  fi
  if ! command -v sudo >/dev/null 2>&1; then
    warn "sudo not found; skipping: $*"
    return 0
  fi
  if [[ "$DRY_RUN" == "1" ]]; then
    log "dry-run sudo -n $*"
    return 0
  fi
  sudo -n "$@" 2>/dev/null || warn "sudo command failed/skipped: $*"
}

report() {
  log "mode=$MODE dry_run=$DRY_RUN min_free_gb=$MIN_FREE_GB free_gb=$(gb_avail) root_fs=$ROOT_FS"
  log "preserve=openclaw:$OPENCLAW_REAL hermes:$HERMES_REAL"
  log "workspace=$WORKSPACE_REAL harness=$HARNESS_REAL"
  du -sh "$WORKSPACE_REAL/tmp_research" 2>/dev/null | sed 's/^/[storage-guard] /' || true
  du -sh "$WORKSPACE_REAL/tmp" 2>/dev/null | sed 's/^/[storage-guard] /' || true
  du -sh "$HARNESS_REAL/target" 2>/dev/null | sed 's/^/[storage-guard] /' || true
  du -sh "$HARNESS_REAL/runs" 2>/dev/null | sed 's/^/[storage-guard] /' || true
  du -sh "$HOME/.cache" 2>/dev/null | sed 's/^/[storage-guard] /' || true
}

prune_light() {
  log "light prune: disposable Captain workspace caches and stale run artifacts"
  if [[ -d "$WORKSPACE_REAL/tmp_research" ]]; then
    while IFS= read -r -d '' item; do safe_rm_rf "$item"; done < <(find "$WORKSPACE_REAL/tmp_research" -mindepth 1 -maxdepth 1 -print0 2>/dev/null || true)
  fi
  safe_find_delete "$WORKSPACE_REAL/tmp" -mindepth 1 -maxdepth 1 -type d -mtime +2 -exec rm -rf {} +
  safe_find_delete "$HARNESS_REAL/runs" -type f -mtime +14 -delete
  safe_find_delete "$HOME/.cache" -mindepth 1 -maxdepth 2 \( -name pip -o -name pip-tools -o -name uv -o -name pytest_cache -o -name '*pytest*' \) -exec rm -rf {} +
}

prune_os() {
  log "os prune: package metadata and journal vacuum (installs preserved)"
  if command -v journalctl >/dev/null 2>&1; then
    sudo_cmd journalctl --vacuum-time=7d
  fi
  if command -v apt-get >/dev/null 2>&1; then
    sudo_cmd apt-get clean
    sudo_cmd apt-get autoremove -y
  elif command -v dnf >/dev/null 2>&1; then
    sudo_cmd dnf clean all
    sudo_cmd dnf autoremove -y
  elif command -v yum >/dev/null 2>&1; then
    sudo_cmd yum clean all
  fi
}

prune_aggressive() {
  log "aggressive prune: Rust build outputs and optional Docker build cache"
  if [[ -d "$HARNESS_REAL/target" ]]; then
    if [[ "$DRY_RUN" == "1" ]]; then
      log "dry-run cargo clean --manifest-path $HARNESS_REAL/Cargo.toml"
    else
      cargo clean --manifest-path "$HARNESS_REAL/Cargo.toml" 2>/dev/null || true
    fi
  fi
  safe_find_delete "$WORKSPACE_REAL/tmp" -type d -name target -prune -exec rm -rf {} +
  safe_find_delete "$WORKSPACE_REAL/tmp_research" -type d -name target -prune -exec rm -rf {} +
  if [[ "$ALLOW_DOCKER" == "1" && $(below_threshold) == "1" && $(command -v docker >/dev/null 2>&1; echo $?) == "0" ]]; then
    if [[ "$DRY_RUN" == "1" ]]; then
      log "dry-run docker builder prune -af --filter until=24h"
    else
      docker builder prune -af --filter until=24h 2>/dev/null || true
    fi
  else
    log "docker cleanup disabled; set CAPTAIN_CLEANUP_DOCKER=1 to prune build cache when still low"
  fi
}

report

if [[ "$MODE" == "report" ]]; then
  exit 0
fi

if [[ "$MODE" == "auto" && $(below_threshold) != "1" ]]; then
  log "free space is above threshold; cleanup not needed"
  exit 0
fi

before=$(bytes_avail)
prune_light
if [[ $(below_threshold) == "1" || "$MODE" == "prune" ]]; then
  prune_os
fi
if [[ $(below_threshold) == "1" || "$MODE" == "prune" ]]; then
  prune_aggressive
fi
after=$(bytes_avail)
reclaimed=$(awk "BEGIN {printf \"%.2f\", ($after-$before)/1024/1024/1024}")
log "reclaimed_gb=$reclaimed free_gb_now=$(gb_avail)"
report
