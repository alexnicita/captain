# Workspace Structure

A readability-first map of the public scaffold.

## Core framework

- `harnesses/`
  - `hourly-harness/` — runtime-duration + checklist gating harness
  - `rust-harness/` — coding loop harness for OpenClaw-compatible agents

## Strategy + planning

- `knowledge-base/` — business and operating knowledge library
- `roadmap/backlog.md` — prioritized task backlog

## Setup + operations

- `scripts/setup-openclaw-captain.sh` — bootstrap OpenClaw + workspace wiring
- `scripts/setup-harness-env.sh` — initialize local harness runtime files
- `scripts/storage_guard.sh` — disk maintenance helper

## Agent persona/workspace context

- `AGENTS.md`, `SOUL.md`, `USER.md`, `TOOLS.md`, `HEARTBEAT.md`

## Local/private (intentionally excluded from OSS scope)

- `cc/`, `prompts/`, `memory/`, `reports/`, `tmp_research/`, local secrets/logs

These are ignored by `.gitignore` where appropriate.
