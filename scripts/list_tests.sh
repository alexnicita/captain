#!/usr/bin/env bash
set -euo pipefail

# Determine repository root robustly
if git rev-parse --show-toplevel > /dev/null 2>&1; then
  REPO_ROOT=$(git rev-parse --show-toplevel)
else
  # Fallback: assume script resides in <repo>/captain/scripts
  REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
fi

usage() {
  cat <<'EOF'
Usage: list_tests.sh [--help]

Prints a newline‑separated list of all test script paths (files matching 'test_*.sh')
relative to the repository root.

Options:
  -h, --help   Show this help message and exit.
EOF
}

# Handle help flags
case "${1-}" in
  -h|--help) usage; exit 0 ;;
  "") ;;
  *) echo "Unexpected argument: $1" >&2; usage; exit 1 ;;
esac

# Find test scripts
mapfile -t tests < <(git -C "$REPO_ROOT" ls-files "tests/test_*.sh" 2>/dev/null || true)

if [[ ${#tests[@]} -eq 0 ]]; then
  echo "No test scripts found" >&2
  exit 1
fi

# Print each test script path
for t in "${tests[@]}"; do
  echo "$t"
done
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