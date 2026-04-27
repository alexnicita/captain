# Captain

Captain turns autonomous coding agents into governed coding runs.

It is a local-first harness for OpenClaw, Codex-style agents, and other tool-using coding agents that need more than a prompt: runtime budgets, command policy, phase logs, replayable JSONL events, commit discipline, and operator-visible release gates.

The operating idea is simple: agents can work longer when the run has a flight recorder.

## What Captain Does

- Runs timeboxed coding sessions against a target repo.
- Emits stable JSONL events for replay, eval, and regression checks.
- Enforces command allowlists, runtime limits, no-op detection, and commit-quality gates.
- Integrates with OpenClaw as an executor while preserving a stable local CLI runner.
- Keeps personal/operator files private by default.

## Five-Minute Start

```bash
git clone https://github.com/alexnicita/captain.git
cd captain
bash scripts/setup-openclaw-captain.sh
bash scripts/captain-doctor.sh
```

Run a governed one-hour coding session:

```bash
bash harnesses/rust-harness/scripts/harness.sh \
  --repo /path/to/target/repo \
  --time 1h \
  --executor openclaw \
  --runtime-log-file ./runs/runtime.log
```

The canonical interface is intentionally stable:

```bash
scripts/harness.sh --repo <path> --time <duration> --executor openclaw
```

## Why This Exists

OpenClaw and similar agents make it easy to give a model real tools and persistent workspace access. Captain adds the governance layer around that power:

- **Policy**: commands and tools are explicit.
- **Evidence**: every phase and gate emits structured events.
- **Replay**: runs can be inspected after the fact.
- **Commit discipline**: generic/no-op/internal-only commits are blocked.
- **Operator control**: local private files and secrets stay out of git.

## Project Layout

- `harnesses/rust-harness/` - Rust coding harness for plan/act/verify loops, event replay, eval, and commit gates.
- `harnesses/hourly-harness/` - minimum-runtime plus checklist gate harness.
- `examples/` - copy-paste launch scenarios for PR review, one-hour sprints, and risky-change blocking.
- `demo/90-second-demo.md` - launch demo script/storyboard.
- `skills/` - OpenClaw-compatible skill packs, including Captain governance integration.
- `templates/personal/` - local-only operator profile templates.
- `knowledge/` - optional background material, not required for the Captain launch path.
- `private/` - gitignored local zone for confidential repos, notes, and secrets.

## Requirements

- Node.js 24 recommended, Node.js 22.14+ minimum.
- Rust 1.76+ for `harnesses/rust-harness`.
- OpenClaw CLI for `--executor openclaw`.
- A model credential via `OPENAI_API_KEY` or OpenClaw auth profiles.

## Verification

Run the smoke suite:

```bash
bash tests/run.sh
```

Run the full Rust suite:

```bash
cargo test --manifest-path harnesses/rust-harness/Cargo.toml
```

Run stricter Rust checks:

```bash
cd harnesses/rust-harness
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Event Contract

Captain treats JSONL events as a public interface. Events include:

- `run.started`, `run.finished`
- `task.started`, `task.finished`
- `provider.request`, `provider.response`, `provider.retry`, `provider.timeout`, `provider.error`
- `tool.call`, `tool.output`, `tool.error`
- `scheduler.dispatch`, `scheduler.result`, `scheduler.tick`
- `coding.run.started`, `coding.cycle.started`, `coding.phase`, `coding.cycle.finished`, `coding.run.finished`
- `git.commit`, `git.push`

Each emitted event has a run id and monotonic sequence number stamped by `EventSink`.

Replay and evaluate a run:

```bash
cd harnesses/rust-harness
cargo run -- replay --path ./runs/events.jsonl --latest-run
cargo run -- eval --path ./runs/events.jsonl --latest-run
```

## OpenClaw Integration

Captain can be mounted as an OpenClaw workspace:

```bash
openclaw config set agents.defaults.workspace "$PWD"
openclaw gateway restart
```

The governance skill lives at `skills/captain-governance/SKILL.md`.

## Security

Captain is designed for local operator control, but governed agents still run real commands. Start from these defaults:

- Keep secrets in `.env.local`, OpenClaw auth profiles, or another local secret store.
- Keep confidential repos under `private/`.
- Use command allowlists for non-default tools.
- Review `runs/events.jsonl` before sharing any run artifact publicly.
- Read `SECURITY.md` before exposing OpenClaw channels or remote access.

## Optional Knowledge Base

The `knowledge/` folder contains optional business and operating-system research material. It is not part of Captain's core governance runtime and should not be required for new users to understand or run the product.
