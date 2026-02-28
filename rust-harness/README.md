# seaport-harness (Rust scaffold)

Rust-first LLM harness starter for Seaport operations.

## Goals
- One-shot and loop execution modes
- Eventually support provider routing, tool adapters, queues, and cost controls

## Planned roadmap
1. Provider abstraction (`OpenAI`, `Anthropic`, local)
2. Tool registry with allowlist and structured traces
3. Queue + scheduler with budget gates
4. Memory ingestion + retrieval interfaces
5. Eval/replay harness for regressions

## Run (after Rust toolchain install)
```bash
cargo run -- status
cargo run -- run --objective "Draft weekly operator brief"
cargo run -- loop --interval-seconds 60
```
