#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

python3 -m pip install -q -r tests/requirements-python.txt

python3 -m pytest \
  harnesses/hourly-harness/tests \
  tests/python \
  --cov=forced_hour_harness \
  --cov-report=term-missing \
  --cov-fail-under="${PY_COVERAGE_MIN:-85}"
