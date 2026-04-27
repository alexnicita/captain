#!/usr/bin/env bash
set -euo pipefail

WORKSPACE="/home/ec2-user/.openclaw/workspace"
ROOT_FS="/"
MIN_FREE_GB="${MIN_FREE_GB:-8}"
REPORT_ONLY="${1:-prune}" # prune|report

bytes_avail() {
  df -B1 --output=avail "$ROOT_FS" | tail -n1 | tr -d ' '
}

gb_avail() {
  awk "BEGIN {printf \"%.1f\", $(bytes_avail)/1024/1024/1024}"
}

log() {
  printf '[storage-guard] %s\n' "$*"
}

report() {
  log "free_gb=$(gb_avail) min_free_gb=$MIN_FREE_GB"
  du -sh "$WORKSPACE/tmp_research" 2>/dev/null | sed 's/^/[storage-guard] /' || true
  du -sh "$WORKSPACE/tmp" 2>/dev/null | sed 's/^/[storage-guard] /' || true
  du -sh "$WORKSPACE/harnesses/rust-harness/target" 2>/dev/null | sed 's/^/[storage-guard] /' || true
  du -sh "$WORKSPACE/harnesses/rust-harness/runs" 2>/dev/null | sed 's/^/[storage-guard] /' || true
}

prune_light() {
  log "light prune: deleting disposable research clones + stale run artifacts"
  rm -rf "$WORKSPACE/tmp_research"/* 2>/dev/null || true
  find "$WORKSPACE/tmp" -mindepth 1 -maxdepth 1 -type d -mtime +2 -exec rm -rf {} + 2>/dev/null || true
  find "$WORKSPACE/harnesses/rust-harness/runs" -type f -mtime +14 -delete 2>/dev/null || true
}

prune_aggressive() {
  log "aggressive prune: cleaning Rust build caches"
  (cd "$WORKSPACE/harnesses/rust-harness" && cargo clean) || true
  find "$WORKSPACE/tmp" -type d -name target -prune -exec rm -rf {} + 2>/dev/null || true
  find "$WORKSPACE/tmp_research" -type d -name target -prune -exec rm -rf {} + 2>/dev/null || true
}

report

if [[ "$REPORT_ONLY" == "report" ]]; then
  exit 0
fi

before=$(bytes_avail)
prune_light

current_gb=$(gb_avail)
need_aggressive=$(awk "BEGIN {print ($current_gb < $MIN_FREE_GB) ? 1 : 0}")
if [[ "$need_aggressive" == "1" ]]; then
  prune_aggressive
fi

after=$(bytes_avail)
reclaimed=$(awk "BEGIN {printf \"%.2f\", ($after-$before)/1024/1024/1024}")
log "reclaimed_gb=$reclaimed free_gb_now=$(gb_avail)"
report
