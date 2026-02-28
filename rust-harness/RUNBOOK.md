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

## 4) Replay + eval

```bash
cargo run -- --config ./config.local.toml replay --path ./runs/events.jsonl
cargo run -- --config ./config.local.toml eval --path ./runs/events.jsonl
```

## 5) Tool policy hardening examples

Allow only `time.now`:

```bash
cargo run -- run --objective "get time" --allow-tool time.now
```

Block `echo` explicitly:

```bash
cargo run -- run --objective "debug" --deny-tool echo
```

## 6) Common failures

- **`cargo not found`**: install rustup, then source `$HOME/.cargo/env`
- **provider timeout**: increase `provider.timeout_ms`
- **excess retries**: lower `provider.max_retries` or validate endpoint/auth
- **empty replay**: confirm `event_log_path` and file permissions
