import importlib.util
from pathlib import Path


def _load_module():
    path = Path(__file__).resolve().parents[2] / "harnesses" / "rust-harness" / "scripts" / "eval_coding_run.py"
    spec = importlib.util.spec_from_file_location("eval_coding_run", path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


def test_evaluate_records_counts_and_quality():
    mod = _load_module()
    records = [
        {"run_id": "r1", "kind": "task.finished"},
        {"run_id": "r1", "kind": "coding.cycle.act", "data": {"success": True}},
        {"run_id": "r1", "kind": "git.commit", "data": {"result": "ok"}},
        {"run_id": "r1", "kind": "git.commit", "data": {"result": "rejected"}},
        {"run_id": "r2", "kind": "git.commit", "data": {"result": "ok"}},
        None,
    ]

    result = mod.evaluate_records(records, "r1")
    assert result["cycles"] == 1
    assert result["act_ok"] == 1
    assert result["commit_ok"] == 1
    assert result["commit_rejected"] == 1
    assert result["malformed_events"] == 1
    assert result["quality_score_100"] == 85


def test_load_jsonl_skips_blank_and_marks_malformed(tmp_path):
    mod = _load_module()
    p = tmp_path / "events.jsonl"
    p.write_text('\n{"run_id":"r1","kind":"task.finished"}\nnot-json\n', encoding="utf-8")

    rows = list(mod.load_jsonl(str(p)))
    assert isinstance(rows[0], dict)
    assert rows[1] is None
