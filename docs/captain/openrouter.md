# OpenRouter Setup

Captain can route governed runs through OpenRouter while still using the same agent shortcuts:

```bash
captain hermes "fix the failing tests" --repo . --time 45m
captain openclaw "implement the next scoped improvement" --repo . --time 1h
```

## Recommended Setup

Install Captain first:

```bash
curl -fsSL https://raw.githubusercontent.com/alexnicita/captain/main/install.sh | bash
```

Then configure OpenRouter:

```bash
captain openrouter setup
```

This writes a local `~/.captain/.env.openrouter` file. The harness sources that file automatically before each governed run.

## One-Line Installer Setup

For scripted machines, run OpenRouter setup during install:

```bash
curl -fsSL https://raw.githubusercontent.com/alexnicita/captain/main/install.sh | \
  bash -s -- --setup-openrouter --openrouter-model anthropic/claude-sonnet-4.6
```

Set `OPENROUTER_API_KEY` or `CAPTAIN_OPENROUTER_API_KEY` in the environment if you want the installer to write the key into the local env file:

```bash
OPENROUTER_API_KEY=... \
curl -fsSL https://raw.githubusercontent.com/alexnicita/captain/main/install.sh | \
  bash -s -- --setup-openrouter --openrouter-model anthropic/claude-sonnet-4.6
```

## Agent Behavior

Captain forwards the selected OpenRouter model to supported sub-agent CLIs when possible:

- OpenClaw receives `--model openrouter/<model>`.
- Hermes receives `--provider openrouter --model <model>`.
- Claude Code and Codex keep their native model configuration unless you set `CAPTAIN_CLAUDE_MODEL`, `CAPTAIN_CODEX_MODEL`, or `CAPTAIN_AGENT_MODEL`.

## Files and Environment

The setup command writes:

```bash
~/.captain/.env.openrouter
```

The file contains local-only values such as:

```bash
export OPENROUTER_API_KEY=...
export CAPTAIN_OPENROUTER_MODEL=anthropic/claude-sonnet-4.6
export HARNESS_PROVIDER=openai-compatible
export HARNESS_PROVIDER_ENDPOINT=https://openrouter.ai/api/v1/chat/completions
export HARNESS_PROVIDER_API_KEY_ENV=OPENROUTER_API_KEY
export HARNESS_MODEL=anthropic/claude-sonnet-4.6
```

Use `CAPTAIN_OPENROUTER_ENV=/path/to/file` if you need a different env file path.
