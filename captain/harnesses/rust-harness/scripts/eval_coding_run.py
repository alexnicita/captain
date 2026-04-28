#!/usr/bin/env python3
import json
import sys
from collections import Counter
from typing import Iterable, Dict, Any

def evaluate_records(records: Iterable[Dict[str, Any]], run_id: str) -> Dict[str, Any]:
    counts = Counter()
    cycles = 0
    commit_ok = 0
    commit_rejected = 0
    act_ok = 0
    act_fail = 0
    malformed_events = 0

    for o in records:
        if not isinstance(o, dict):
            malformed_events += 1
            continue
        if o.get('run_id') != run_id:
            continue
        k = o.get('kind')
        counts[k] += 1
        if k == 'task.finished':
            cycles += 1
        if k == 'coding.cycle.act':
            if o.get('data', {}).get('success'):
                act_ok += 1
            else:
                act_fail += 1
        if k == 'git.commit':
            d = o.get('data', {})
            if d.get('result') == 'ok':
                commit_ok += 1
            if d.get('result') == 'rejected':
                commit_rejected += 1

    quality = 0
    if cycles:
        quality += int((act_ok / cycles) * 50)
        quality += int((commit_ok / cycles) * 40)
    quality -= min(commit_rejected * 5, 20)
    quality = max(0, min(100, quality))

    return {
        'run_id': run_id,
        'cycles': cycles,
        'act_ok': act_ok,
        'act_fail': act_fail,
        'commit_ok': commit_ok,
        'commit_rejected': commit_rejected,
        'malformed_events': malformed_events,
        'quality_score_100': quality,
        'counts': dict(counts),
    }

def load_jsonl(path: str):
    with open(path, 'r', encoding='utf-8') as f:
        for line in f:
            stripped = line.strip()
            if not stripped:
                continue
            try:
                yield json.loads(stripped)
            except json.JSONDecodeError:
                yield None


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        print("usage: eval_coding_run.py <events.jsonl> <run_id>")
        return 2

    path, run_id = argv[1], argv[2]
    result = evaluate_records(load_jsonl(path), run_id)
    print(json.dumps(result, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main(sys.argv))
