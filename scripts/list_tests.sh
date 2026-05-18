#!/usr/bin/env bash
set -euo pipefail

if git rev-parse --show-toplevel > /dev/null 2>&1; then
  REPO_ROOT=$(git rev-parse --show-toplevel)
else
  REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
fi

usage() {
  cat <<'EOF'
Usage: list_tests.sh [--help]

Prints a newline-separated list of all test script paths (files matching 'test_*.sh')
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
tests=()
while IFS= read -r test_path; do
  tests+=("$test_path")
done < <(git -C "$REPO_ROOT" ls-files "tests/test_*.sh" 2>/dev/null || true)

if [[ ${#tests[@]} -eq 0 ]]; then
  echo "No test scripts found" >&2
  exit 1
fi

# Print each test script path
for t in "${tests[@]}"; do
  echo "$t"
done
