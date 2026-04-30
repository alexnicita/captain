# Captain

Captain turns autonomous coding agents into governed coding runs.

It is a local-first harness for OpenClaw, Codex-style agents, and other tool-using coding agents that need more than a prompt: runtime budgets, command policy, phase logs, replayable JSONL events, commit discipline, and operator-visible release gates.

The operating idea is simple: agents can work longer when the run has a flight recorder.

## What Captain Does

- Runs timeboxed coding sessions against a target repo.
- Emits stable JSONL events for replay, eval, and regression checks.
- Enforces command allowlists, runtime limits, no-op detection, and commit-quality gates.
- Integrates with OpenClaw or Hermes as agent CLI executors while preserving a stable local CLI runner.
- Keeps personal/operator files private by default.

## Five-Minute Start

```bash
git clone https://github.com/alexnicita/captain.git
cd captain
bash captain/scripts/setup-openclaw-captain.sh
bash captain/scripts/captain-doctor.sh
export PATH="$PWD/captain/bin:$PATH"
```

Run a governed one-hour coding session:

```bash
captain openclaw "implement the next scoped improvement" \
  --repo /path/to/target/repo \
  --time 1h \
  --runtime-log-file ./runs/runtime.log
```

The operator CLI is intentionally shaped around agent shortcuts:

```bash
captain hermes "fix the failing tests" --repo <path> --time 45m --commit-each-cycle
captain openclaw "implement feature X" --repo <path> --time 1h
captain code --executor hermes --prompt "ship useful code" --repo <path> --time 30m
```

These shortcuts route through the stable harness interface:

```bash
captain/harnesses/rust-harness/scripts/harness.sh --repo <path> --time <duration> --executor openclaw
captain/harnesses/rust-harness/scripts/harness.sh --repo <path> --time <duration> --executor hermes
```

## Why This Exists

OpenClaw, Hermes, Codex-style CLIs, and similar agents make it easy to give a model real tools and persistent workspace access. Captain adds the governance layer around that power:

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
- OpenClaw CLI for `--executor openclaw`, or Hermes CLI for `--executor hermes`.
- A model credential via `OPENAI_API_KEY` / OpenClaw auth profiles, or Hermes auth/config for Hermes runs.

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
- Read `SECURITY.md` and `docs/captain/security-threat-model.md` before exposing OpenClaw/Hermes channels or remote access.
- Treat Captain as governance, not a VM/container sandbox; use disposable isolation for untrusted prompts or repos.

## Optional Knowledge Base

The `captain/knowledge/` folder contains optional business and operating-system research material. It is not part of Captain's core governance runtime and should not be required for new users to understand or run the product.
