# captain — Harness Framework for Agent Governance

`captain` is a framework-style workspace for building and running **harnesses that govern coding agents** (OpenClaw and similar systems).

> Guiding principle: **personal = private**.

## Core idea

Use harnesses as policy + runtime control layers around autonomous agents:
- enforce execution constraints
- standardize loop behavior
- collect reproducible logs/artifacts
- keep operator control explicit

---

## Project layout

- `harnesses/hourly-harness/` — minimum-runtime + checklist gate harness
- `harnesses/rust-harness/` — full Rust coding harness (timeboxed plan/act/verify loop)
- `knowledge-base/` — curated business/operating research library
- `roadmap/backlog.md` — prioritized execution backlog
- `private/` — local private zone (repos/secrets/notes), excluded from git
- `scripts/setup-openclaw-captain.sh` — bootstrap OpenClaw + workspace + harness init
- `scripts/setup-harness-env.sh` — create local harness runtime files

---

## Quick start

```bash
git clone https://github.com/alexnicita/captain.git
cd captain
bash scripts/setup-openclaw-captain.sh
bash scripts/init-local-profile.sh
```

This sets up OpenClaw and initializes local harness files for `harnesses/rust-harness`.
It also creates local personal markdown files from templates.

## Personal markdown templates (gitignored local copies)

Personal/operator files are local-only and ignored:

- `AGENTS.md`, `HEARTBEAT.md`, `IDENTITY.md`, `SOUL.md`, `TOOLS.md`, `USER.md`, `MEMORY.md`

Templates live in:

- `templates/personal/*.template.md`

Initialize local copies with:

```bash
bash scripts/init-local-profile.sh
```

---

## Add this framework to OpenClaw

### Automatic
Included in setup script (`openclaw config set agents.defaults.workspace ...`).

### Manual

```bash
openclaw config set agents.defaults.workspace ~/.openclaw/workspace/captain
openclaw gateway restart
openclaw config get agents.defaults.workspace
```

Expected value: your local clone path for this repo.

---

## Harness setup (local files)

```bash
bash scripts/setup-harness-env.sh
```

Creates (if missing):
- `harnesses/rust-harness/config.local.toml`
- `harnesses/rust-harness/.env.local`
- `harnesses/rust-harness/prompts/session-prompt.txt`
- `harnesses/rust-harness/runs/`

Set credentials in:

```bash
nano harnesses/rust-harness/.env.local
```

Load into shell:

```bash
set -a
source harnesses/rust-harness/.env.local
set +a
```

---

## Run the harnesses

### Rust harness (coding loop)

```bash
cd harnesses/rust-harness
./scripts/check_toolchain.sh
./start.sh 1h
```

Canonical interface:

```bash
./scripts/harness.sh --repo . --time 45m --prompt-file ./prompts/session-prompt.txt
```

### Hourly harness (duration + checklist gate)

```bash
cd harnesses/hourly-harness
cp checklist.example.md checklist.my-run.md
./run.sh start ./checklist.my-run.md
```

---

## OpenClaw verification

```bash
openclaw status
openclaw doctor
openclaw config get agents.defaults.workspace
```

Control UI:
- http://127.0.0.1:18789

---

## Security notes

- Never commit API keys or local secrets.
- Keep `harnesses/rust-harness/.env.local` local.
- Clone confidential repos into `private/repos/`.
- Validate before publishing:

```bash
git status
git log --oneline -n 10
```

---

## Docs

- Framework index: `harnesses/README.md`
- Rust harness deep docs: `harnesses/rust-harness/README.md`
- Private workspace rules: `private/README.md`
- OpenClaw docs: https://docs.openclaw.ai

---

## Workspace structure (quick map)

A readability-first map of the public scaffold.

### Core framework

- `harnesses/`
  - `hourly-harness/` — runtime-duration + checklist gating harness
  - `rust-harness/` — coding loop harness for OpenClaw-compatible agents

### Strategy + planning

- `knowledge-base/` — business and operating knowledge library
- `roadmap/backlog.md` — prioritized task backlog

### Setup + operations

- `scripts/setup-openclaw-captain.sh` — bootstrap OpenClaw + workspace wiring
- `scripts/setup-harness-env.sh` — initialize local harness runtime files
- `scripts/storage_guard.sh` — disk maintenance helper

### Private-by-default area

- `private/` — local-only zone for confidential repo clones, secrets, and notes
- Principle: **personal = private**

### Agent persona/workspace context

- `templates/personal/` — tracked templates for local persona/profile markdown files
- local copies created via `scripts/init-local-profile.sh` and kept gitignored

### Local/private (intentionally excluded from OSS scope)

- `cc/`, `prompts/`, `memory/`, `reports/`, `tmp_research/`, `private/`, local secrets/logs

These are ignored by `.gitignore` where appropriate.
