# Operator Runbook

## 1) First-time setup

```bash
./scripts/check_toolchain.sh
cp ./config.example.toml ./config.local.toml
```

If using `provider.kind = "http"`, export your API key env referenced by `provider.api_key_env`.

## 2) Single run

```bash
cargo run -- --config ./config.local.toml run --objective "what time is it"
```

## 3) Batch queue run

```bash
cargo run -- --config ./config.local.toml batch --objectives-file ./fixtures/objectives.txt
```

Use `[p1]` prefix for high-priority queue items in the objectives file.

## 4) Timeboxed coding run (1 hour)

```bash
cargo run -- --config ./config.local.toml code --repo /path/to/repo --time 1h
```

Coding mode expects a provider capable of unified diff generation.
For real code output, use `provider.kind = "http"` (or `HARNESS_PROVIDER=http`) with a reachable endpoint/model.
Default target is Codex 5.3 via Responses API (`https://api.openai.com/v1/responses`, model `gpt-5.3-codex`).
For OpenRouter, run `captain openrouter setup` after install, or pass `--setup-openrouter --openrouter-model <model>` to the installer. Captain will use `https://openrouter.ai/api/v1/chat/completions` and forward the selected model to OpenClaw/Hermes agent runs when those CLIs support it.

## 4b) Supercycle run (architecture remap + task graph artifacts)

```bash
cargo run -- --config ./config.local.toml code --repo /path/to/repo --time 20m --supercycle
```

Supercycle writes planning artifacts under `.harness/supercycle/` per cycle:
- `cycle-*-ARCH_REMAP.md`
- `cycle-*-TASK_GRAPH.md`
- `cycle-*-TASK_PACK.md`

Optional prompt input (empty by default unless supplied):

```bash
cargo run -- --config ./config.local.toml code --repo /path/to/repo --time 1h --prompt "<session prompt>"
# or
cargo run -- --config ./config.local.toml code --repo /path/to/repo --time 1h --prompt-file ./prompt.txt
```

Useful flags:

```bash
--executor cargo|shell|openclaw|hermes|claude|codex
--plan-cmd "..." --act-cmd "..." --verify-cmd "..."   # repeatable
--allow-cmd "<binary>"                                    # extends allowlist
--commit-each-cycle --push-each-cycle
--cycle-output-file ./runs/cycle-output.jsonl
--runtime-log-file ./runs/coding-runtime.log
--noop-streak-limit 3
--conformance-interval-unchanged 3
--progress-file ./.harness/coding-progress.json
--run-lock-file ./.git/.agent-harness-code.lock
```

Live monitoring:

```bash
./scripts/events-pretty.sh ./runs/events.jsonl --format emoji

tail -F ./runs/console.log ./runs/runtime.log
```

Coding mode guarantees the phase order each cycle:

`architecture -> feature -> conformance -> cleanup -> pause`

If the repo is clean at architecture phase, the harness selects the next feature task from internal docs (`ARCHITECTURE.md`, `README.md`, `RUNBOOK.md`, `MIGRATION.md`) before running feature work.

Agent CLI executors (`openclaw`, `hermes`, `claude`, `codex`) use the same JSON-edit harness path. OpenClaw is invoked through `openclaw agent`; Hermes is invoked through `hermes chat --quiet -q ...` and uses `CAPTAIN_HERMES_TOOLSETS`; Claude Code is invoked through `claude --print` and uses `CAPTAIN_CLAUDE_TOOLS`; Codex is invoked through `codex exec` and uses `CAPTAIN_CODEX_SANDBOX`. `CAPTAIN_OPENROUTER_MODEL` maps to `openclaw agent --model openrouter/<model>` and `hermes chat --provider openrouter --model <model>`; set the agent-specific model env vars for Claude Code or Codex.

Cleanup always emits explicit git sync outcomes (`fetch`, `pull`, `conflict_resolution`, `commit`, `push`) so operators can see clean merges vs conflicts and unresolved/conflict-resolution status.

Hard anti-noop controls (defaults):
- `noop_streak_limit=3`: after 3 no-meaningful-diff cycles, the next architecture cycle forces a concrete scoped code-change task. If that forced cycle still has no meaningful diff, the run aborts explicitly.
- `conformance_interval_unchanged=3`: full conformance runs every 3 unchanged cycles, but always runs immediately when mutations exist.
- Single-instance per repo lock: lock file prevents concurrent coding runs on the same repo and emits `coding.lock.exists` with fail-fast metadata (`fail_fast=true`, `exit_code=1`) and `coding.lock.acquired` on success.
- Progress memory persists completed+attempted ids and task history (`.harness/coding-progress.json`) so ranking can score novelty + impact with cooldown.
- If the same task repeats with no net diff for >2 cycles, task selection escalates to alternate sources.
- Commit quality gate blocks internal-state-only diffs unless `src/` or task-tied docs changed.
- Commit subject quality gate enforces conventional + informative file-aware subjects, hard-blocks generic templated text, requires changed-scope tokens, and de-duplicates short-window repeats.
- Explicit git lifecycle events are emitted on every path: `git.commit`, `git.push` (success/skip/blocked/failure) including subject/result metadata.
- Counters are emitted every cycle: `noop_streak`, `forced_mutation`, `task_advanced`, `source_escalation`.

## 5) Runtime-gate checklist run (Rust)

```bash
cargo run -- gate start --checklist ./fixtures/gate_checklist.done.md --dry-run --dry-runtime-sec 3 --dry-heartbeat-sec 1 --poll-seconds 1
cargo run -- gate status
cargo run -- gate stop
```

`gate status` now reports terminal-aware elapsed time (`elapsed_sec`), plus heartbeat freshness via `last_heartbeat_epoch` and `heartbeat_stale_sec`.

## 6) Replay + eval

```bash
cargo run -- --config ./config.local.toml replay --path ./runs/events.jsonl --latest-run
cargo run -- --config ./config.local.toml eval --path ./runs/events.jsonl --latest-run
```

## 7) Tool policy hardening examples

Allow only `time.now`:

```bash
cargo run -- run --objective "get time" --allow-tool time.now
```

Block `echo` explicitly:

```bash
cargo run -- run --objective "debug" --deny-tool echo
```

## 8) Common failures

- **`cargo not found`**: install rustup, then source `$HOME/.cargo/env`
- **provider timeout**: increase `provider.timeout_ms`
- **excess retries**: lower `provider.max_retries` or validate endpoint/auth
- **empty replay**: confirm `event_log_path` and file permissions
- **`coding.lock.exists`**: another coding run already owns the lock; stop it first (or remove stale lock if process is gone).
- **`corrupt patch at line ...`**: malformed provider diff output; inspect `.harness/tmp/llm-patch-*.diff` and retry with the supercycle agent CLI JSON-edit path (`openclaw`, `hermes`, `claude`, or `codex`).
- **dirty-tree spin** (`repo has pending changes; continue current feature thread`): stop run, clean/reset tree, rerun from clean state.
- **forced no-diff abort**: your act phase is not producing meaningful scoped changes; tighten `--act-cmd` and validate with `git diff --stat`.
- **commit subject rejected by quality gate**: subject was generic or lacked changed-file scope; inspect staged names and commit event payload (`subject`, `detail`).
- **`chore(harness): watchdog keepalive cycle N`**: liveness keepalive commit after commit drought (allow-empty); indicates recovery path, not feature completion.
