# agent-harness

A general-purpose, publishable Rust harness for provider/tool orchestration.

This project is intentionally **config-driven and pluggable**:
- swap providers (`echo`, `http`, `http-stub`) via config
- register tools with typed handlers + policy gates
- run single tasks, loops, queued batches, and runtime-gated checklist flows
- emit JSONL event streams for replay/eval/regression checks
- stay Rust-first (no Python runtime dependency for orchestration)

See `ARCHITECTURE.md` for internals and extension points.
If you're migrating from Python-side orchestration helpers, see `MIGRATION.md`.

## What ships now

- **Provider abstraction** (`Provider` trait, async)
  - `EchoProvider` deterministic local adapter
  - `HttpProvider` scaffold for OpenAI-compatible endpoints
  - timeout + retry settings wired through config
- **Tool registry + policy gates**
  - tool specs with input/output schemas
  - typed parsing for built-in tools
  - allow-list / deny-list execution policy
- **Robust orchestrator loop**
  - max steps/tool calls/runtime guardrails
  - provider retries with linear backoff
  - evented tool call/output/error lifecycle
- **Queue/scheduler primitives**
  - in-memory priority queue (`[p1]` high-priority lines)
  - bounded-concurrency batch runner (configurable)
- **Runtime-gate orchestration (Rust port)**
  - enforce minimum runtime + checklist completion gates
  - start/status/stop lifecycle with JSON state + progress logs
- **Replay + eval baseline**
  - event taxonomy summaries
  - fixture-backed regression checks
- **Observability**
  - run-scoped IDs + sequence numbers
  - event taxonomy (`run.*`, `task.*`, `provider.*`, `tool.*`, `cli.*`)

## Toolchain + local build checks

Run user-space readiness checks (no root assumptions):

```bash
./scripts/check_toolchain.sh
./scripts/build.sh
```

If Rust is missing, the checker prints rustup bootstrap commands.

## CLI

```bash
agent-harness --help
agent-harness status
agent-harness run --objective "what time is it"
agent-harness run --objective "what time is it" --allow-tool time.now
agent-harness batch --objectives-file ./fixtures/objectives.txt
# objectives format supports optional prefixes: [p1] high, [p0] normal
agent-harness gate start --checklist ./fixtures/gate_checklist.done.md --dry-run --dry-runtime-sec 3 --dry-heartbeat-sec 1 --poll-seconds 1
agent-harness gate status
agent-harness replay --path ./runs/events.jsonl
agent-harness eval --path ./runs/events.jsonl
agent-harness loop --interval-seconds 60 --max-iterations 5 --objective "heartbeat time task"
```

With cargo:

```bash
cargo run -- status
cargo run -- run --objective "what time is it"
cargo run -- batch --objectives-file ./fixtures/objectives.txt
cargo run -- replay --path ./runs/events.jsonl
cargo run -- eval --path ./runs/events.jsonl
```

## Config

Pass `--config ./config.example.toml` or rely on defaults.

Environment overrides:

- `HARNESS_PROVIDER` (`echo`, `http`, `http-stub`)
- `HARNESS_MODEL`
- `HARNESS_EVENT_LOG`
- `HARNESS_PROVIDER_ENDPOINT`
- `HARNESS_PROVIDER_TIMEOUT_MS`
- `HARNESS_PROVIDER_MAX_RETRIES`
- scheduler knobs live in TOML (`scheduler.max_concurrent_tasks`, `scheduler.queue_poll_ms`)

## Dogfood workflow (harness-on-harness)

```bash
./scripts/dogfood.sh
```

That script runs:
1. toolchain checks
2. `run` objective through orchestrator
3. `replay` on generated event log
4. `eval` regression checks on the same run
5. `gate start/status` dry-run to validate Rust runtime-gate orchestration

Use this flow continuously while extending modules.

## Contributor notes

- Keep features provider-agnostic and config-driven.
- Prefer additive extension points over hardcoded behavior.
- Add fixture coverage when introducing new event kinds or stop reasons.
