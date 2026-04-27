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

## Private-by-default area

- `private/` — local-only zone for confidential repo clones, secrets, and notes
- Principle: **personal = private**

## Agent persona/workspace context

- `templates/personal/` — tracked templates for local persona/profile markdown files
- local copies created via `scripts/init-local-profile.sh` and kept gitignored

## Local/private (intentionally excluded from OSS scope)

- `cc/`, `prompts/`, `memory/`, `reports/`, `tmp_research/`, `private/`, local secrets/logs

These are ignored by `.gitignore` where appropriate.
