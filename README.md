# captain

Public starter workspace for running a proactive OpenClaw operator agent.

This repo gives you:
- Agent identity + behavior files (`SOUL.md`, `AGENTS.md`, `USER.md`, `TOOLS.md`, `HEARTBEAT.md`)
- Automation helpers (`scripts/`)
- A starter knowledge base + harness examples

---

## Quick start (recommended)

### 1) Install prerequisites
- Git
- Node.js **v20+** (v22 recommended)
- npm

### 2) Clone + bootstrap

```bash
git clone https://github.com/alexnicita/captain.git
cd captain
bash scripts/setup-openclaw-captain.sh
```

This script will:
- verify prerequisites
- install `openclaw` globally if missing
- keep the repo updated
- create `~/.openclaw/openclaw.json` if it does not exist (and point workspace to this repo)

### 3) Run OpenClaw onboarding

```bash
openclaw onboard
```

Use this to configure:
- model providers (OpenAI/Anthropic/etc)
- channels (Telegram/Signal/Discord/etc)
- pairing/allowlist policy

### 4) Verify runtime

```bash
openclaw status
openclaw config get agents.defaults.workspace
```

Expected workspace value:
- your local clone path for this repo

### 5) Open Control UI

```text
http://127.0.0.1:18789
```

---

## Manual setup (no helper script)

```bash
npm install -g openclaw
mkdir -p ~/.openclaw/workspace
git clone https://github.com/alexnicita/captain.git ~/.openclaw/workspace/captain
```

Create `~/.openclaw/openclaw.json`:

```json5
{
  agents: {
    defaults: {
      workspace: "~/.openclaw/workspace/captain",
    },
  },
}
```

Then run:

```bash
openclaw onboard
openclaw status
```

---

## Configure this workspace for yourself

Before using in production, personalize these files:

1. `IDENTITY.md` — your agent name/persona
2. `USER.md` — who you are helping
3. `SOUL.md` — communication style and boundaries
4. `TOOLS.md` — machine-specific notes (SSH hosts, stack prefs, etc.)
5. `HEARTBEAT.md` — proactive checks/tasks

Optional:
- `MEMORY.md` and `memory/YYYY-MM-DD.md` for persistent context

---

## Suggested first-run commands

```bash
# validate config + runtime health
openclaw doctor
openclaw status

# inspect key config
openclaw config get agents.defaults.workspace

# restart gateway after config updates
openclaw gateway restart
```

---

## Security notes for open-sourcing your own fork

- Do **not** commit secrets (`.env`, API keys, local tokens).
- Keep local/private memory files out of git.
- Review channel allowlists before enabling external messaging.
- Run a final check before making your repo public:

```bash
git status
git log --oneline -n 10
```

---

## Script reference

### `scripts/setup-openclaw-captain.sh`

Usage:

```bash
bash scripts/setup-openclaw-captain.sh [options]
```

Options:
- `--repo-url <url>`
- `--install-dir <path>`
- `--target-dir <path>`
- `--repo-name <name>`
- `--config-path <path>`
- `--skip-openclaw-install`

Example custom install:

```bash
bash scripts/setup-openclaw-captain.sh \
  --repo-url https://github.com/<you>/captain.git \
  --target-dir ~/work/captain \
  --config-path ~/.openclaw/openclaw.json
```

---

## OpenClaw docs

- Local docs: `/home/ec2-user/.npm-global/lib/node_modules/openclaw/docs`
- Online docs: https://docs.openclaw.ai
- Source: https://github.com/openclaw/openclaw
