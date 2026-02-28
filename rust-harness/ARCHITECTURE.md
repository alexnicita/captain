# agent-harness architecture

## Core flow

1. CLI parses command + loads config
2. Provider factory builds `Box<dyn Provider>` from config
3. Orchestrator executes task loop with budgets + retries
4. Provider returns messages + optional tool calls
5. Tool registry dispatches calls through policy gate
6. Event sink writes structured JSONL telemetry
7. Replay/eval consume events for offline analysis and regression checks

## Modules

- `config.rs`
  - defaults + TOML loading + env overrides
  - provider/orchestrator/scheduler runtime knobs
- `provider.rs`
  - async provider trait
  - `EchoProvider`, `HttpProvider`, `HttpProviderStub`
  - OpenAI-compatible adapter scaffolding
- `tools.rs`
  - typed tool specs + handlers
  - policy modes (`AllowAll`, `AllowList`) + deny-list
- `orchestrator.rs`
  - guarded execution loop
  - provider timeout/retry mechanics
  - tool lifecycle event emission
- `scheduler.rs`
  - queue primitives + bounded-concurrency batch execution
- `events.rs`
  - event taxonomy constants
  - run IDs + sequence numbers
  - JSONL sink
- `replay.rs`
  - aggregate event summaries by kind/run/task
- `eval.rs`
  - baseline checks for event integrity and regressions

## Event taxonomy

- `run.started`, `run.finished`
- `task.started`, `task.finished`
- `provider.request`, `provider.response`, `provider.retry`, `provider.timeout`, `provider.error`
- `tool.call`, `tool.output`, `tool.error`
- `cli.run.summary`, `cli.batch.summary`

## Design principles

- **General-purpose first**: no project-specific behavior in core modules.
- **Policy before execution**: enforce tool gates at dispatch boundary.
- **Observability by default**: every state transition is evented.
- **Config over constants**: all knobs exposed in TOML/env.
- **Fixture-backed evolution**: replay/eval fixtures protect regressions.

## Planned next increments

1. parallel scheduler worker pool (bounded concurrency)
2. provider plugins behind feature flags
3. JSON-Schema validation crate integration for tool IO
4. richer eval suites (latency budgets, retry profile checks)
