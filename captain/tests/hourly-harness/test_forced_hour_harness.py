from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from pathlib import Path

import pytest


@pytest.fixture()
def mod(tmp_path: Path):
    file_path = (
        Path(__file__).resolve().parents[2]
        / "src"
        / "hourly-harness"
        / "forced_hour_harness.py"
    )
    spec = importlib.util.spec_from_file_location("forced_hour_harness", file_path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    sys.modules["forced_hour_harness"] = module
    spec.loader.exec_module(module)

    runs_dir = tmp_path / "runs"
    latest = runs_dir / "latest_run.txt"
    module.RUNS_DIR = runs_dir
    module.LATEST_PTR = latest
    return module


def test_parse_checklist_counts(mod, tmp_path: Path):
    checklist = tmp_path / "checklist.md"
    checklist.write_text(
        "\n".join(
            [
                "- [x] done item",
                "- [ ] pending item",
                "* [X] uppercase done",
                "not a checkbox line",
            ]
        ),
        encoding="utf-8",
    )

    stats = mod.parse_checklist(checklist)
    assert stats.total == 3
    assert stats.done == 2
    assert stats.pending == 1
    assert stats.all_done is False


def test_load_latest_run_dir_missing_pointer_raises(mod):
    with pytest.raises(FileNotFoundError):
        mod.load_latest_run_dir()


def test_load_latest_run_dir_stale_pointer_raises(mod):
    mod.RUNS_DIR.mkdir(parents=True, exist_ok=True)
    missing = mod.RUNS_DIR / "missing-run"
    mod.LATEST_PTR.write_text(str(missing), encoding="utf-8")
    with pytest.raises(FileNotFoundError):
        mod.load_latest_run_dir()


def test_fmt_ts_returns_iso(mod):
    assert "1970-01-01T00:00:00" in mod.fmt_ts(0)


def test_parse_checklist_missing_file_raises(mod, tmp_path: Path):
    with pytest.raises(FileNotFoundError):
        mod.parse_checklist(tmp_path / "does-not-exist.md")


def test_create_run_dir_and_latest_pointer(mod):
    run_dir = mod.create_run_dir("abc")
    assert run_dir.exists()
    assert run_dir.name.endswith("-abc")
    assert mod.LATEST_PTR.exists()
    assert Path(mod.LATEST_PTR.read_text(encoding="utf-8").strip()) == run_dir


def test_cmd_stop_writes_stop_file_and_log(mod, capsys):
    run_dir = mod.create_run_dir("stop")
    state = {
        "run_dir": str(run_dir),
        "checklist_path": str(run_dir / "c.md"),
        "start_epoch": 0,
        "start_iso_utc": "1970-01-01T00:00:00+00:00",
        "min_runtime_sec": 1,
        "heartbeat_sec": 1,
        "poll_sec": 1,
        "dry_run": True,
        "pid": 1,
        "status": "running",
    }
    mod.write_state(run_dir, state)

    rc = mod.cmd_stop(argparse.Namespace(run_dir=None))
    assert rc == 0
    assert (run_dir / "STOP").exists()
    assert "Stop requested:" in capsys.readouterr().out


def test_cmd_status_prints_finish_readiness(mod, tmp_path: Path, capsys):
    run_dir = mod.create_run_dir("status")
    checklist = tmp_path / "status.md"
    checklist.write_text("- [x] complete\n", encoding="utf-8")
    state = {
        "run_dir": str(run_dir),
        "checklist_path": str(checklist),
        "start_epoch": 0.0,
        "start_iso_utc": "1970-01-01T00:00:00+00:00",
        "min_runtime_sec": 0,
        "heartbeat_sec": 1,
        "poll_sec": 1,
        "dry_run": True,
        "pid": 1,
        "status": "complete",
    }
    mod.write_state(run_dir, state)

    rc = mod.cmd_status(argparse.Namespace(run_dir=str(run_dir)))
    assert rc == 0
    out = capsys.readouterr().out
    assert "can_finish_now: True" in out


def test_cmd_run_completes_in_dry_mode(mod, tmp_path: Path):
    checklist = tmp_path / "run.md"
    checklist.write_text("- [x] done\n", encoding="utf-8")

    args = argparse.Namespace(
        checklist=str(checklist),
        run_id="dry",
        min_runtime_minutes=60.0,
        heartbeat_minutes=10.0,
        poll_seconds=1,
        dry_run=True,
        dry_runtime_sec=1,
        dry_heartbeat_sec=1,
    )

    rc = mod.cmd_run(args)
    assert rc == 0

    run_dir = Path(mod.LATEST_PTR.read_text(encoding="utf-8").strip())
    state = json.loads((run_dir / "state.json").read_text(encoding="utf-8"))
    assert state["status"] == "complete"


def test_cmd_run_invalid_heartbeat_raises(mod, tmp_path: Path):
    checklist = tmp_path / "invalid.md"
    checklist.write_text("- [x] done\n", encoding="utf-8")

    args = argparse.Namespace(
        checklist=str(checklist),
        run_id=None,
        min_runtime_minutes=1.0,
        heartbeat_minutes=0.0,
        poll_seconds=1,
        dry_run=False,
        dry_runtime_sec=1,
        dry_heartbeat_sec=0,
    )
    with pytest.raises(ValueError):
        mod.cmd_run(args)


def test_cmd_run_stop_requested_path(mod, tmp_path: Path):
    checklist = tmp_path / "stop.md"
    checklist.write_text("- [ ] pending\n", encoding="utf-8")

    run_dir = mod.RUNS_DIR / "run-stop"
    run_dir.mkdir(parents=True, exist_ok=True)
    (run_dir / "STOP").write_text("stop", encoding="utf-8")
    mod.LATEST_PTR.parent.mkdir(parents=True, exist_ok=True)
    mod.LATEST_PTR.write_text(str(run_dir), encoding="utf-8")

    mod.create_run_dir = lambda run_id=None: run_dir

    args = argparse.Namespace(
        checklist=str(checklist),
        run_id="stop",
        min_runtime_minutes=1.0,
        heartbeat_minutes=1.0,
        poll_seconds=1,
        dry_run=True,
        dry_runtime_sec=60,
        dry_heartbeat_sec=1,
    )

    rc = mod.cmd_run(args)
    assert rc == 2


def test_main_status_flow(mod, capsys, tmp_path: Path, monkeypatch):
    run_dir = mod.create_run_dir("main")
    checklist = tmp_path / "main.md"
    checklist.write_text("- [x] done\n", encoding="utf-8")
    state = {
        "run_dir": str(run_dir),
        "checklist_path": str(checklist),
        "start_epoch": 0.0,
        "start_iso_utc": "1970-01-01T00:00:00+00:00",
        "min_runtime_sec": 0,
        "heartbeat_sec": 1,
        "poll_sec": 1,
        "dry_run": True,
        "pid": 1,
        "status": "complete",
    }
    mod.write_state(run_dir, state)

    monkeypatch.setattr("sys.argv", ["forced_hour_harness.py", "status", "--run-dir", str(run_dir)])
    assert mod.main() == 0
    assert "can_finish_now: True" in capsys.readouterr().out
