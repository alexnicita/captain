#!/usr/bin/env bash
set -euo pipefail
# Run rust harness lint
pushd captain/harnesses/rust-harness > /dev/null
bash lint.sh
popd > /dev/null