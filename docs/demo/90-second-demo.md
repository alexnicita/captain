# 90-Second Demo: Captain Catches the Run

## Goal

Show Captain as the governance layer for an autonomous coding agent: the agent attempts a real task, Captain timeboxes it, logs every phase, blocks weak commits, and produces replay/eval evidence.

## Script

1. Show the target repo and a one-line task:

   ```bash
   git status --short
   ```

2. Start a governed OpenClaw coding run:

   ```bash
   bash captain/harnesses/rust-harness/scripts/harness.sh \
     --repo /path/to/target/repo \
     --time 15m \
     --executor openclaw \
     --runtime-log-file ./runs/demo-runtime.log \
     --prompt "Make one scoped reliability improvement with tests"
   ```

3. Tail the operator stream:

   ```bash
   tail -f ./runs/demo-runtime.log
   ```

4. Show the event replay:

   ```bash
   cd captain/harnesses/rust-harness
   cargo run -- replay --path ./runs/events.jsonl --latest-run
   cargo run -- eval --path ./runs/events.jsonl --latest-run
   ```

5. Close with the message:

   Captain is the flight recorder and release gate for autonomous coding agents.

## Visual Beats

- `coding.phase` events: architecture, feature, conformance, cleanup, pause.
- Commit gate blocks generic or internal-only commits.
- Replay confirms ordered events with run id and sequence numbers.
- Eval report turns an agent session into an inspectable artifact.
