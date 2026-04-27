---
name: captain-governance
description: Run autonomous coding work through Captain's governance harness when a user asks for long-running OpenClaw/Codex-style coding with policy, logs, replay, eval, or commit gates.
---

# Captain Governance

Use this skill when coding-agent work needs runtime control, evidence, or release gates.

## Before Running

From the Captain workspace:

```bash
bash scripts/captain-doctor.sh
```

## Governed Coding Run

```bash
bash harnesses/rust-harness/scripts/harness.sh \
  --repo "<target-repo>" \
  --time "1h" \
  --executor openclaw \
  --runtime-log-file "harnesses/rust-harness/runs/runtime.log" \
  --prompt "<specific coding objective>"
```

## Replay and Eval

```bash
cd harnesses/rust-harness
cargo run -- replay --path ./runs/events.jsonl --latest-run
cargo run -- eval --path ./runs/events.jsonl --latest-run
```

## Operating Rules

- Prefer scoped code changes with tests.
- Keep command policy explicit for tools outside defaults.
- Do not share raw event logs until secrets, private paths, and proprietary repo names are removed.
- Treat `run_id`, `task_id`, and monotonic `seq` as the evidence trail.
