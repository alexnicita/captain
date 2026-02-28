# seaport-harness architecture (sprint snapshot)

## Flow

1. CLI parses command/config
2. Provider factory builds `Box<dyn Provider>` from config
3. Orchestrator runs task loop with guardrails
4. Provider emits messages and optional tool call plans
5. Tool registry dispatches structured JSON I/O
6. Event sink appends JSONL events
7. Replay/eval read those events for offline analysis

## Modules

- `config.rs`: defaults + TOML + env overrides
- `provider.rs`: provider trait + adapters + factory
- `tools.rs`: tool registry/dispatcher
- `orchestrator.rs`: task execution loop with budgets
- `events.rs`: event schema + sink
- `replay.rs`: event-log summary
- `eval.rs`: replay-based checks

## Extension points

- add real provider adapters (OpenAI/Anthropic/local)
- introduce tool allowlist policies by objective/source
- add queue-backed multi-task scheduler
- add richer eval suite for regressions and safety assertions
