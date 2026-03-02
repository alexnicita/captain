# rust-harness (mega-restart)

Minimal OpenClaw generator/discriminator loop.

## Run

```bash
cargo run -- start --goal "Improve this repo" --time 5m
```

## How it works

- Starts two persistent OpenClaw sessions:
  - generator: writes code
  - discriminator: critiques and proposes next prompt
- Loops until time expires.
- Attempts commit+push on cadence/score.
- Logs discriminator prompts to `runs/discriminator-prompts-<epoch>.md`.

## Flags

- `--repo <path>` (default `.`)
- `--time <5m|300s|minutes>`
- `--goal <text>` (required)
- `--session-prefix <id>`
- `--generator-timeout <sec>`
- `--discriminator-timeout <sec>`
- `--commit-every <cycles>`
