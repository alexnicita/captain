#!/usr/bin/env python3
"""Forced runtime harness.

Blocks completion until BOTH:
1) minimum runtime has elapsed
2) all checklist items are marked complete

Usage examples:
  python3 captain/harnesses/hourly-harness/forced_hour_harness.py run --checklist captain/harnesses/hourly-harness/checklist.md
  python3 captain/harnesses/hourly-harness/forced_hour_harness.py status
  python3 captain/harnesses/hourly-harness/forced_hour_harness.py stop
  python3 captain/harnesses/hourly-harness/forced_hour_harness.py run --checklist captain/harnesses/hourly-harness/checklist.md --dry-run
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import os
import re
import signal
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Tuple

ROOT = Path(__file__).resolve().parent
RUNS_DIR = ROOT / "runs"
LATEST_PTR = RUNS_DIR / "latest_run.txt"
CHECKBOX_RE = re.compile(r"^\s*[-*]\s+\[( |x|X)\]\s+(.+?)\s*$")


@dataclasses.dataclass
class ChecklistStats:
    total: int
    done: int

    @property
    def pending(self) -> int:
        return self.total - self.done

    @property
    def all_done(self) -> bool:
        return self.total > 0 and self.done == self.total


def utc_now() -> datetime:
    return datetime.now(timezone.utc)


def fmt_ts(ts: float) -> str:
    return datetime.fromtimestamp(ts, tz=timezone.utc).isoformat()


def parse_checklist(path: Path) -> ChecklistStats:
    if not path.exists():
        raise FileNotFoundError(f"Checklist not found: {path}")

    total = 0
    done = 0
    for line in path.read_text(encoding="utf-8").splitlines():
        m = CHECKBOX_RE.match(line)
        if not m:
            continue
        total += 1
        if m.group(1).lower() == "x":
            done += 1
    return ChecklistStats(total=total, done=done)

def parse_checklist_safe(path: Path) -> ChecklistStats:
    """Parse checklist, returning empty stats if file missing instead of raising."""
    try:
        return parse_checklist(path)
    except FileNotFoundError:
        return ChecklistStats(total=0, done=0)

def load_latest_run_dir() -> Path:
    if not LATEST_PTR.exists():
        raise FileNotFoundError("No latest run pointer found. Start a run first.")
    path = Path(LATEST_PTR.read_text(encoding="utf-8").strip())
    if not path.exists():
        raise FileNotFoundError(f"Latest run path no longer exists: {path}")
    return path


def append_log(run_dir: Path, message: str) -> None:
    line = f"[{utc_now().isoformat()}] {message}\n"
    with (run_dir / "progress.log").open("a", encoding="utf-8") as f:
        f.write(line)


def write_state(run_dir: Path, state: dict) -> None:
    (run_dir / "state.json").write_text(json.dumps(state, indent=2), encoding="utf-8")


def read_state(run_dir: Path) -> dict:
    return json.loads((run_dir / "state.json").read_text(encoding="utf-8"))


def create_run_dir(run_id: str | None) -> Path:
    RUNS_DIR.mkdir(parents=True, exist_ok=True)
    stamp = utc_now().strftime("%Y%m%dT%H%M%SZ")
    suffix = f"-{run_id}" if run_id else ""
    run_dir = RUNS_DIR / f"run-{stamp}{suffix}"
    run_dir.mkdir(parents=True, exist_ok=False)
    LATEST_PTR.write_text(str(run_dir), encoding="utf-8")
    return run_dir


def summarize_state(state: dict, checklist: ChecklistStats) -> Tuple[float, float, bool]:
    now = time.time()
    elapsed = now - state["start_epoch"]
    remaining = max(0.0, state["min_runtime_sec"] - elapsed)
    gate_open = remaining <= 0
    return elapsed, remaining, gate_open
    now = time.time()
    elapsed = now - state["start_epoch"]
    remaining = max(0.0, state["min_runtime_sec"] - elapsed)
    gate_open = remaining <= 0
    return elapsed, remaining, gate_open


def cmd_run(args: argparse.Namespace) -> int:
    checklist = Path(args.checklist).resolve()
    if args.dry_run:
        min_runtime_sec = int(args.dry_runtime_sec)
        heartbeat_sec = int(args.dry_heartbeat_sec)
    else:
        min_runtime_sec = int(args.min_runtime_minutes * 60)
        heartbeat_sec = int(args.heartbeat_minutes * 60)

    if heartbeat_sec <= 0:
        raise ValueError("heartbeat interval must be > 0")

    run_dir = create_run_dir(args.run_id)
    state = {
        "run_dir": str(run_dir),
        "checklist_path": str(checklist),
        "start_epoch": time.time(),
        "start_iso_utc": utc_now().isoformat(),
        "min_runtime_sec": min_runtime_sec,
        "heartbeat_sec": heartbeat_sec,
        "poll_sec": args.poll_seconds,
        "dry_run": bool(args.dry_run),
        "pid": os.getpid(),
        "status": "running",
    }
    write_state(run_dir, state)
    append_log(
        run_dir,
        (
            f"START pid={state['pid']} checklist={checklist} "
            f"min_runtime_sec={min_runtime_sec} heartbeat_sec={heartbeat_sec}"
        ),
    )

    print(f"Run directory: {run_dir}")
    print(f"Started at: {state['start_iso_utc']}")
    print("Harness is now enforcing runtime + checklist completion gates...")

    stop_path = run_dir / "STOP"
    next_heartbeat = 0.0

    def _handle_term(signum, _frame):
        append_log(run_dir, f"RECEIVED_SIGNAL signum={signum}; stopping")
        cur = read_state(run_dir)
        cur["status"] = "stopped_by_signal"
        cur["stop_iso_utc"] = utc_now().isoformat()
        write_state(run_dir, cur)
        sys.exit(2)

    signal.signal(signal.SIGTERM, _handle_term)
    signal.signal(signal.SIGINT, _handle_term)

    while True:
        if stop_path.exists():
            append_log(run_dir, "STOP request file detected")
            state = read_state(run_dir)
            state["status"] = "stopped"
            state["stop_iso_utc"] = utc_now().isoformat()
            write_state(run_dir, state)
            print("Stopped by operator request.")
            return 2

        checklist_stats = parse_checklist(checklist)
        elapsed, remaining, gate_open = summarize_state(state, checklist_stats)

        now = time.time()
        if now >= next_heartbeat:
            append_log(
                run_dir,
                (
                    "HEARTBEAT "
                    f"elapsed_sec={elapsed:.0f} remaining_sec={remaining:.0f} "
                    f"checklist_done={checklist_stats.done}/{checklist_stats.total} "
                    f"gate_open={gate_open} all_done={checklist_stats.all_done}"
                ),
            )
            next_heartbeat = now + heartbeat_sec

        if gate_open and checklist_stats.all_done:
            append_log(
                run_dir,
                (
                    "COMPLETE criteria satisfied "
                    f"elapsed_sec={elapsed:.0f} checklist={checklist_stats.done}/{checklist_stats.total}"
                ),
            )
            state = read_state(run_dir)
            state["status"] = "complete"
            state["finish_epoch"] = time.time()
            state["finish_iso_utc"] = utc_now().isoformat()
            state["elapsed_sec"] = elapsed
            write_state(run_dir, state)
            print("DONE: Minimum runtime + checklist are both satisfied.")
            print(f"Progress log: {run_dir / 'progress.log'}")
            return 0

        time.sleep(args.poll_seconds)


def cmd_status(args: argparse.Namespace) -> int:
    run_dir = Path(args.run_dir).resolve() if args.run_dir else load_latest_run_dir()
    state = read_state(run_dir)
    checklist = parse_checklist(Path(state["checklist_path"]))
    elapsed, remaining, gate_open = summarize_state(state, checklist)

    print(f"run_dir: {run_dir}")
    print(f"status: {state.get('status', 'unknown')}")
    print(f"started: {state['start_iso_utc']}")
    print(f"elapsed_sec: {elapsed:.0f}")
    print(f"remaining_sec: {remaining:.0f}")
    print(f"runtime_gate_open: {gate_open}")
    print(f"checklist: {checklist.done}/{checklist.total} done")
    print(f"all_checklist_done: {checklist.all_done}")
    print(f"can_finish_now: {gate_open and checklist.all_done}")
    print(f"progress_log: {run_dir / 'progress.log'}")
    return 0


def cmd_stop(args: argparse.Namespace) -> int:
    run_dir = Path(args.run_dir).resolve() if args.run_dir else load_latest_run_dir()
    stop_path = run_dir / "STOP"
    stop_path.write_text(f"stop requested at {utc_now().isoformat()}\n", encoding="utf-8")
    append_log(run_dir, "STOP requested by operator")
    print(f"Stop requested: {stop_path}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Forced one-hour execution harness")
    sub = parser.add_subparsers(dest="command", required=True)

    p_run = sub.add_parser("run", help="Start and enforce a run")
    p_run.add_argument("--checklist", required=True, help="Markdown checklist with - [ ] / - [x] items")
    p_run.add_argument("--run-id", help="Optional run suffix")
    p_run.add_argument("--min-runtime-minutes", type=float, default=60.0)
    p_run.add_argument("--heartbeat-minutes", type=float, default=10.0)
    p_run.add_argument("--poll-seconds", type=int, default=15)
    p_run.add_argument("--dry-run", action="store_true", help="Use short timings for testing")
    p_run.add_argument("--dry-runtime-sec", type=int, default=75)
    p_run.add_argument("--dry-heartbeat-sec", type=int, default=12)
    p_run.set_defaults(func=cmd_run)

    p_status = sub.add_parser("status", help="Show state for current/latest run")
    p_status.add_argument("--run-dir", help="Explicit run dir; otherwise uses latest")
    p_status.set_defaults(func=cmd_status)

    p_stop = sub.add_parser("stop", help="Request stop for current/latest run")
    p_stop.add_argument("--run-dir", help="Explicit run dir; otherwise uses latest")
    p_stop.set_defaults(func=cmd_stop)

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
