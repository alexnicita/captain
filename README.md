# Captain

Captain turns autonomous coding agents into governed coding runs.

It is a local-first harness for OpenClaw, Hermes, Claude Code, Codex, and other tool-using coding agents that need more than a prompt: runtime budgets, command policy, phase logs, replayable JSONL events, commit discipline, and operator-visible release gates.

The operating idea is simple: agents can work longer when the run has a flight recorder.

## What Captain Does

- Runs timeboxed coding sessions against a target repo.
- Emits stable JSONL events for replay, eval, and regression checks.
- Enforces command allowlists, runtime limits, no-op detection, and commit-quality gates.
- Integrates with OpenClaw, Hermes, Claude Code, or Codex as agent CLI executors while preserving a stable local CLI runner.
- Keeps personal/operator files private by default.

## Use Captain on an Agent Machine

Captain is for the machine where your agents already live: an EC2 instance or MacBook with OpenClaw, Hermes, Claude Code, Codex, or another coding-agent CLI installed. Add Captain to that box, put `captain` on `PATH`, and run agents through Captain when you want better control: timeboxes, logs, command policy, commit gates, replayable events, and opt-in pushes.

```bash
curl -fsSL https://raw.githubusercontent.com/alexnicita/captain/main/install.sh | bash
```

The installer clones or updates Captain under `~/.captain`, links the `captain` command into `~/.local/bin`, and runs lightweight setup/doctor checks. If `~/.local/bin` is not already on your shell path, it prints the one `PATH` line to add.

Run Hermes through Captain:

```bash
captain hermes "fix the failing tests" \
  --repo /path/to/target/repo \
  --time 45m \
  --runtime-log-file ./runs/hermes-runtime.log \
  --commit-each-cycle
```

Run OpenClaw through Captain:

```bash
captain openclaw "implement the next scoped improvement" \
  --repo /path/to/target/repo \
  --time 1h \
  --runtime-log-file ./runs/openclaw-runtime.log \
  --commit-each-cycle
```

Run Claude Code or Codex through the same harness:

```bash
captain claude "tighten the parser tests" --repo /path/to/target/repo --time 30m
captain codex "refactor the flaky fixture loader" --repo /path/to/target/repo --time 30m
```

Preview the exact lower-level harness command without launching an agent:

```bash
captain hermes "ship useful code" --repo . --time 30m --dry-run
```

Use explicit executor mode when building scripts or APIs around Captain:

```bash
captain code --executor hermes --prompt "ship useful code" --repo . --time 30m
captain code --executor openclaw --prompt "review and improve this PR" --repo . --time 1h
captain code --executor claude --prompt "add regression tests" --repo . --time 30m
captain code --executor codex --prompt "simplify this module" --repo . --time 30m
```

Pushes are opt-in. `--commit-each-cycle` can create local commits; add `--push-each-cycle` only when you intentionally want Captain to push after successful committed cycles.

These shortcuts route through the stable harness interface:

```bash
captain/harnesses/rust-harness/scripts/harness.sh --repo <path> --time <duration> --executor openclaw
captain/harnesses/rust-harness/scripts/harness.sh --repo <path> --time <duration> --executor hermes
captain/harnesses/rust-harness/scripts/harness.sh --repo <path> --time <duration> --executor claude
captain/harnesses/rust-harness/scripts/harness.sh --repo <path> --time <duration> --executor codex
```

## Why This Exists

OpenClaw, Hermes, Claude Code, Codex, and similar agents make it easy to give a model real tools and persistent workspace access. Captain adds the governance layer around that power:

- **Policy**: commands and tools are explicit.
- **Evidence**: every phase and gate emits structured events.
- **Replay**: runs can be inspected after the fact.
- **Commit discipline**: generic/no-op/internal-only commits are blocked.
- **Operator control**: local private files and secrets stay out of git.

## Product Naming

- **Captain** is the product and repository: the governance/control plane for autonomous coding runs.
- **`agent-harness`** is the current Rust package/binary retained as the stable harness implementation and compatibility surface.
- **`captain/harnesses/rust-harness/scripts/harness.sh`** is the canonical operator entrypoint for launch docs and demos.

## Project Layout

- `captain/` - canonical product tree, with product source, compatibility entrypoints, skills, private workspace support, and subsystem ownership metadata.
- `docs/` - public launch docs, examples, demos, and architecture notes.
- `tests/` - repository-level smoke tests.
- `captain/src/rust-harness/` - active Rust governance harness implementation.
- `captain/src/hourly-harness/` - active Python runtime/checklist gate implementation.
- `captain/tests/` - product tests referenced by Cargo/pytest.
- `captain/harnesses/rust-harness/` - stable Cargo/scripts compatibility layer for the Rust harness.
- `captain/harnesses/hourly-harness/` - stable CLI compatibility layer for the runtime/checklist gate.
- `docs/examples/` - copy-paste launch scenarios for PR review, one-hour sprints, and risky-change blocking.
- `docs/demo/90-second-demo.md` - launch demo script/storyboard.
- `captain/skills/` - OpenClaw-compatible skill packs, including Captain governance integration.
- `captain/templates/personal/` - local-only operator profile templates.
- `captain/knowledge/` - optional background material, not required for the Captain launch path.
- `captain/private/` - gitignored local zone for confidential repos, notes, and secrets.

## Requirements

- Node.js 24 recommended, Node.js 22.14+ minimum.
- Rust 1.76+ for `captain/harnesses/rust-harness`.
- OpenClaw CLI for `--executor openclaw`, Hermes CLI for `--executor hermes`, Claude Code CLI for `--executor claude`, or Codex CLI for `--executor codex`.
- A model credential via `OPENAI_API_KEY` / OpenClaw auth profiles, or the selected agent CLI's own auth/config for Hermes, Claude Code, and Codex runs.

## Verification

Run the smoke suite:

```bash
bash tests/run.sh
```

Run the full Rust suite:

```bash
cargo test --manifest-path captain/harnesses/rust-harness/Cargo.toml
```

Run stricter Rust checks:

```bash
cd captain/harnesses/rust-harness
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
cd captain/harnesses/rust-harness
cargo run -- replay --path ./runs/events.jsonl --latest-run
cargo run -- eval --path ./runs/events.jsonl --latest-run
```

## OpenClaw Integration

Captain can be mounted as an OpenClaw workspace:

```bash
openclaw config set agents.defaults.workspace "$PWD"
openclaw gateway restart
```

The governance skill lives at `captain/skills/captain-governance/SKILL.md`.

## Security

Captain is designed for local operator control, but governed agents still run real commands. Start from these defaults:

- Keep secrets in `.env.local`, OpenClaw auth profiles, or another local secret store.
- Keep confidential repos under `captain/private/`.
- Use command allowlists for non-default tools.
- Review `runs/events.jsonl` before sharing any run artifact publicly.
- Read `SECURITY.md` and `docs/captain/security-threat-model.md` before exposing agent channels or remote access.
- Treat Captain as governance, not a VM/container sandbox; use disposable isolation for untrusted prompts or repos.

## Optional Knowledge Base

The `captain/knowledge/` folder contains optional business and operating-system research material. It is not part of Captain's core governance runtime and should not be required for new users to understand or run the product.
