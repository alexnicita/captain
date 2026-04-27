#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

bash tests/test_scaffold.sh
bash tests/test_scripts.sh
bash tests/test_hourly_harness.sh
bash tests/test-python.sh

if [[ "${RUN_RUST_TESTS:-0}" == "1" ]]; then
  bash tests/test-rust.sh
else
  echo "rust tests skipped (set RUN_RUST_TESTS=1 to enable full rust test run)"
fi

echo "all tests: ok"
