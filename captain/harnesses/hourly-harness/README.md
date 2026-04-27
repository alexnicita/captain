# Forced One-Hour Execution Harness

This harness enforces **both** conditions before allowing a run to finish:

1. Minimum runtime gate is open (`elapsed >= configured minimum`, default **60 minutes**)  
2. Checklist gate is open (all markdown checklist items are marked `[x]`)

It also writes heartbeat/progress logs at a fixed interval (default **every 10 minutes**).

---

## Files

- `captain/src/hourly-harness/forced_hour_harness.py` — main runner + status/stop commands
- `captain/harnesses/hourly-harness/forced_hour_harness.py` — compatibility entrypoint
- `captain/harnesses/hourly-harness/run.sh` — small convenience wrapper
- `captain/harnesses/hourly-harness/checklist.example.md` — checklist template for each run
- `captain/harnesses/hourly-harness/test_dry_run.sh` — smoke test for short dry-run
- `captain/harnesses/hourly-harness/runs/` — generated per-run state and logs

Each run creates:

- `state.json` — start timestamp, min runtime, status, finish timestamp
- `progress.log` — append-only timestamped events (`START`, `HEARTBEAT`, `COMPLETE`, `STOP`)
- `STOP` — optional operator stop signal file

---

## Quick Start

1) Create a checklist for the run:

```bash
cp captain/harnesses/hourly-harness/checklist.example.md captain/harnesses/hourly-harness/checklist.my-run.md
```

2) Start enforcement run (default 60 min, 10 min heartbeat):

```bash
python3 captain/harnesses/hourly-harness/forced_hour_harness.py run --checklist captain/harnesses/hourly-harness/checklist.my-run.md
# or
captain/harnesses/hourly-harness/run.sh start captain/harnesses/hourly-harness/checklist.my-run.md
```

3) In another terminal, monitor status/logs:

```bash
python3 captain/harnesses/hourly-harness/forced_hour_harness.py status
tail -f $(cat captain/harnesses/hourly-harness/runs/latest_run.txt)/progress.log
```

4) Mark checklist items complete by editing checklist file (`[ ]` -> `[x]`).

5) Harness exits with `DONE` only when runtime + checklist are both satisfied.

---

## Dry-Run Mode (no 60-minute wait)

Use dry-run mode to test behavior quickly.

```bash
python3 captain/harnesses/hourly-harness/forced_hour_harness.py run \
  --checklist captain/harnesses/hourly-harness/checklist.my-run.md \
  --dry-run
```

Dry-run defaults:
- runtime gate: 75 seconds
- heartbeat: 12 seconds

You can override via:
- `--dry-runtime-sec`
- `--dry-heartbeat-sec`

Smoke test:

```bash
captain/harnesses/hourly-harness/test_dry_run.sh
```

---

## Monitor / Stop

Status for latest run:

```bash
python3 captain/harnesses/hourly-harness/forced_hour_harness.py status
```

Status for specific run:

```bash
python3 captain/harnesses/hourly-harness/forced_hour_harness.py status --run-dir captain/harnesses/hourly-harness/runs/run-...
```

Request stop for latest run:

```bash
python3 captain/harnesses/hourly-harness/forced_hour_harness.py stop
```

Stop creates `STOP` in the run folder; active run exits on next poll cycle.

---

## Runtime Enforcement Model

- `start_epoch` is written at run start in `state.json`.
- `remaining_sec = max(0, min_runtime_sec - elapsed_sec)`.
- Completion gate is true only when:

```text
elapsed_sec >= min_runtime_sec AND checklist_all_done == true
```

Until this is true, harness keeps running and emitting periodic heartbeat lines.

---

## Notes / Limitations

- Checklist parser uses markdown checkboxes (`- [ ]`, `- [x]`).
- At least one checklist item is required for completion.
- If checklist file path is wrong/missing, run fails fast.
- This harness enforces at runner level; it does not forcibly wrap every external tool by itself.
