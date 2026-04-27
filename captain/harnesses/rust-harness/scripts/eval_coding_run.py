#!/usr/bin/env python3
import json
import sys
from collections import Counter

if len(sys.argv) < 3:
    print("usage: eval_coding_run.py <events.jsonl> <run_id>")
    sys.exit(2)

path, run_id = sys.argv[1], sys.argv[2]
counts = Counter()
cycles = 0
commit_ok = 0
commit_rejected = 0
act_ok = 0
act_fail = 0

with open(path, 'r', encoding='utf-8') as f:
    for line in f:
        o = json.loads(line)
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

print(json.dumps({
    'run_id': run_id,
    'cycles': cycles,
    'act_ok': act_ok,
    'act_fail': act_fail,
    'commit_ok': commit_ok,
    'commit_rejected': commit_rejected,
    'quality_score_100': quality,
    'counts': counts,
}, default=lambda x: dict(x), indent=2))
