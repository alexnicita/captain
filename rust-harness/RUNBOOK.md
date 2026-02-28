# Operator Runbook

## 1) First-time setup

```bash
./scripts/check_toolchain.sh
cp ./config.example.toml ./config.local.toml
```

If using `provider.kind = "http"`, export your API key env referenced by `provider.api_key_env`.

## 2) Single run

```bash
cargo run -- --config ./config.local.toml run --objective "what time is it"
```

## 3) Batch queue run

```bash
cargo run -- --config ./config.local.toml batch --objectives-file ./fixtures/objectives.txt
```

Use `[p1]` prefix for high-priority queue items in the objectives file.

## 4) Runtime-gate checklist run (Rust)

```bash
cargo run -- gate start --checklist ./fixtures/gate_checklist.done.md --dry-run --dry-runtime-sec 3 --dry-heartbeat-sec 1 --poll-seconds 1
cargo run -- gate status
cargo run -- gate stop
```

`gate status` now reports terminal-aware elapsed time (`elapsed_sec`), plus heartbeat freshness via `last_heartbeat_epoch` and `heartbeat_stale_sec`.

## 5) Replay + eval

```bash
cargo run -- --config ./config.local.toml replay --path ./runs/events.jsonl --latest-run
cargo run -- --config ./config.local.toml eval --path ./runs/events.jsonl --latest-run
```

## 6) Tool policy hardening examples

Allow only `time.now`:

```bash
cargo run -- run --objective "get time" --allow-tool time.now
```

Block `echo` explicitly:

```bash
cargo run -- run --objective "debug" --deny-tool echo
```

## 7) Common failures

- **`cargo not found`**: install rustup, then source `$HOME/.cargo/env`
- **provider timeout**: increase `provider.timeout_ms`
- **excess retries**: lower `provider.max_retries` or validate endpoint/auth
- **empty replay**: confirm `event_log_path` and file permissions
