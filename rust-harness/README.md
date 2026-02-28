# seaport-harness

Rust-first autonomous harness skeleton focused on safe orchestration loops.

## What ships now

- **Provider abstraction** (`Provider` trait)
  - `EchoProvider` deterministic local adapter
  - `HttpProviderStub` for future remote model wiring
- **Tool registry/dispatcher**
  - Structured JSON input/output
  - Built-in tools: `echo`, `time.now`
- **Task orchestrator with guardrails**
  - max steps
  - max tool calls
  - max runtime seconds
- **Event model + tracing/logging**
  - JSONL event sink (`task.started`, `provider.response`, `tool.output`, `task.finished`, `cli.run.summary`)
- **Config loading**
  - defaults + optional TOML file + env overrides
- **Replay + eval scaffolding**
  - Replay event file into aggregate summary
  - Basic eval checks for run health

## CLI

```bash
seaport-harness --help
seaport-harness status
seaport-harness run --objective "what time is it"
seaport-harness replay --path ./runs/events.jsonl
seaport-harness eval --path ./runs/events.jsonl
seaport-harness loop --interval-seconds 60 --objective "heartbeat time task"
```

With cargo:

```bash
cargo run -- status
cargo run -- run --objective "what time is it"
cargo run -- replay --path ./runs/events.jsonl
cargo run -- eval --path ./runs/events.jsonl
```

## Config

Pass `--config ./config/harness.toml` or rely on defaults.

Env overrides:

- `HARNESS_PROVIDER` (e.g. `echo`, `http-stub`)
- `HARNESS_MODEL`
- `HARNESS_EVENT_LOG`

## Dogfood workflow (local)

1. Run a task:
   - `cargo run -- run --objective "harness self-check time"`
2. Replay generated events:
   - `cargo run -- replay --path ./runs/events.jsonl`
3. Run eval checks:
   - `cargo run -- eval --path ./runs/events.jsonl`

This validates the harness with its own orchestration/event pipeline.

## Notes

- Current runtime in this environment may not have the Rust toolchain installed (`cargo` unavailable).
- Code is structured to compile with stable Rust once toolchain is present.
