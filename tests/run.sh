#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

bash tests/test_scaffold.sh
bash tests/test_scripts.sh
bash tests/test_hourly_harness.sh

echo "all tests: ok"
