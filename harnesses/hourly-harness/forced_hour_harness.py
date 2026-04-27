#!/usr/bin/env python3
"""Compatibility entrypoint for the hourly harness.

The implementation lives in `captain/src/hourly-harness/forced_hour_harness.py`.
This wrapper preserves the historical command path.
"""

from __future__ import annotations

import runpy
from pathlib import Path


TARGET = (
    Path(__file__).resolve().parents[2]
    / "captain"
    / "src"
    / "hourly-harness"
    / "forced_hour_harness.py"
)

runpy.run_path(str(TARGET), run_name="__main__")
