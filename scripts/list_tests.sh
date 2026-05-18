#!/usr/bin/env bash
# List all pytest tests in the repository
# Usage: ./scripts/list_tests.sh
set -euo pipefail

# Find the repository root (assumed current working directory)
PYTEST_CMD="pytest -q --collect-only"
if command -v pytest >/dev/null 2>&1; then
  $PYTEST_CMD
elif command -v python >/dev/null 2>&1; then
  python -m pytest -q --collect-only
else
  echo "Neither pytest nor python found in PATH. Please install test dependencies."
  exit 1
fi