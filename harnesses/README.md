# Harness Framework

This folder is the framework layer for governing autonomous coding behavior for **OpenClaw** and similar agent systems.

Related scaffold at repo root:
- `knowledge-base/` for strategic reference material
- `roadmap/backlog.md` for prioritized execution planning

## Included harnesses

- `hourly-harness/` — runtime + checklist enforcement harness (minimum-time gate)
- `rust-harness/` — full coding orchestration harness (plan/act/verify cycles)

## How to choose

- Use **hourly-harness** when you need strict execution duration + checklist completion gates.
- Use **rust-harness** when you need iterative coding loops with logs, policies, and commit/push flows.

## Quick start

```bash
# initialize local harness runtime files
bash scripts/setup-harness-env.sh

# run rust harness (from repo root)
cd harnesses/rust-harness
./start.sh 1h

# run hourly harness (from repo root)
cd harnesses/hourly-harness
./run.sh start ./checklist.example.md
```
