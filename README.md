# captain (OpenClaw Harness Workspace)

This repo is a ready-to-clone workspace for running the **Rust harness** (`rust-harness/`) with OpenClaw.

It includes:
- a production-style harness runner (`rust-harness/start.sh`)
- canonical timeboxed runner (`rust-harness/scripts/harness.sh`)
- bootstrap scripts to set up OpenClaw + harness local files

---

## What the harness does

The harness runs a timeboxed coding loop with explicit phases (architecture → feature → conformance → cleanup → pause), event logs, and optional commit/push-per-cycle behavior.

Primary entrypoints:
- `rust-harness/start.sh` → fast default for coding runs
- `rust-harness/scripts/harness.sh --repo <path> --time <duration>` → canonical interface

---

## One-command setup (recommended)

```bash
git clone https://github.com/alexnicita/captain.git
cd captain
bash scripts/setup-openclaw-captain.sh
```

This will:
1. verify/install OpenClaw
2. clone/update workspace
3. create `~/.openclaw/openclaw.json` if missing
4. set `agents.defaults.workspace` to this repo
5. initialize harness local files:
   - `rust-harness/config.local.toml`
   - `rust-harness/.env.local`
   - `rust-harness/prompts/session-prompt.txt`
   - `rust-harness/runs/`

---

## Add this harness to OpenClaw

### Option A (auto via setup script)
Already handled by `scripts/setup-openclaw-captain.sh` using:

```bash
openclaw config set agents.defaults.workspace <this-repo-path>
```

### Option B (manual)

```bash
openclaw config set agents.defaults.workspace ~/.openclaw/workspace/captain
openclaw gateway restart
openclaw config get agents.defaults.workspace
```

If you prefer direct file edit, `~/.openclaw/openclaw.json` should include:

```json5
{
  agents: {
    defaults: {
      workspace: "~/.openclaw/workspace/captain",
    },
  },
}
```

---

## Configure credentials for harness runs

Edit local-only env file:

```bash
nano rust-harness/.env.local
```

Set at minimum:

```bash
OPENAI_API_KEY=your_key_here
```

Then load it for your shell session:

```bash
set -a
source rust-harness/.env.local
set +a
```

---

## First run

```bash
cd rust-harness
./scripts/check_toolchain.sh
./start.sh 1h
```

Alternative (explicit canonical runner):

```bash
./scripts/harness.sh --repo . --time 45m --prompt-file ./prompts/session-prompt.txt
```

---

## Script guide

### `scripts/setup-openclaw-captain.sh`
Bootstrap OpenClaw + workspace + harness local files.

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
- `--no-init-harness`
- `--no-config-update`

### `scripts/setup-harness-env.sh`
Initialize just harness local runtime files.

```bash
bash scripts/setup-harness-env.sh [--harness-dir <path>]
```

---

## OpenClaw verification commands

```bash
openclaw status
openclaw doctor
openclaw config get agents.defaults.workspace
```

Control UI:
- `http://127.0.0.1:18789`

---

## Security / OSS notes

- Never commit `.env` or API keys.
- Keep `rust-harness/.env.local` local.
- Verify with:

```bash
git status
git log --oneline -n 10
```

---

## References

- OpenClaw docs: https://docs.openclaw.ai
- OpenClaw source: https://github.com/openclaw/openclaw
- Harness deep docs: `rust-harness/README.md`
