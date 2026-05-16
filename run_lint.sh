#!/usr/bin/env bash
set -euo pipefail
# Run rust harness lint
pushd captain/harnesses/rust-harness > /dev/null
./lint.sh
popd > /dev/null