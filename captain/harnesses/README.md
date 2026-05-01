# Harness Framework

This folder is the framework layer for governing autonomous coding behavior for **OpenClaw, Hermes, Claude Code, Codex**, and similar agent systems.

Related scaffold at repo root:
- `captain/knowledge/` for strategic reference material
- `captain/roadmap/backlog.md` for prioritized execution planning

## Included harnesses

- `hourly-harness/` — stable wrapper around `captain/src/hourly-harness`
- `rust-harness/` — Cargo/scripts compatibility layer for `captain/src/rust-harness`

## How to choose

- Use **hourly-harness** when you need strict execution duration + checklist completion gates.
- Use **rust-harness** when you need iterative coding loops with logs, policies, and commit/push flows.

## Quick start

```bash
# initialize local harness runtime files
bash captain/scripts/setup-harness-env.sh

# run rust harness (from repo root)
cd captain/harnesses/rust-harness
./start.sh 1h

# run hourly harness (from repo root)
cd captain/harnesses/hourly-harness
./run.sh start ./checklist.example.md
```
