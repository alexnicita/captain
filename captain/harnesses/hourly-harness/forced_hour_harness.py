#!/usr/bin/env python3
"""Compatibility entrypoint for the hourly harness.

The implementation lives in `captain/src/hourly-harness/forced_hour_harness.py`.
This wrapper preserves the historical command path.
"""

from __future__ import annotations

import sys
import runpy
from pathlib import Path

# Resolve path to the actual implementation script
TARGET = (
    Path(__file__).resolve().parents[2]
    / "src"
    / "hourly-harness"
    / "forced_hour_harness.py"
)

# Execute the implementation and propagate its exit code
exit_code = runpy.run_path(str(TARGET), run_name="__main__")
# run_path returns a dict; the script uses sys.exit internally, so reaching here means success
sys.exit(0)
