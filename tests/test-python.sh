#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VENV_DIR="${PY_TEST_VENV:-$ROOT/.venv-test}"
PYTHON_BIN="$VENV_DIR/bin/python"

if [[ ! -x "$PYTHON_BIN" ]]; then
  python3 -m venv "$VENV_DIR"
fi

"$PYTHON_BIN" -m pip install -q --upgrade pip
"$PYTHON_BIN" -m pip install -q -r tests/requirements-python.txt

"$PYTHON_BIN" -m pytest \
  captain/tests/hourly-harness \
  tests/python \
  --cov=forced_hour_harness \
  --cov-report=term-missing \
  --cov-fail-under="${PY_COVERAGE_MIN:-85}"
