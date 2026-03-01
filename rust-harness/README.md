# agent-harness

A general-purpose, publishable Rust harness for provider/tool orchestration.

This project is intentionally **config-driven and pluggable**:
- swap providers (`echo`, `http`, `http-stub`) via config
- register tools with typed handlers + policy gates
- run single tasks, loops, queued batches, and runtime-gated checklist flows
- emit JSONL event streams for replay/eval/regression checks
- stay Rust-first (no Python runtime dependency for orchestration)

See `ARCHITECTURE.md` for internals and extension points.
If you're migrating from Python-side orchestration helpers, see `MIGRATION.md`.

## What ships now

- **Provider abstraction** (`Provider` trait, async)
  - `EchoProvider` deterministic local adapter
  - `HttpProvider` scaffold for OpenAI-compatible endpoints
  - timeout + retry settings wired through config
- **Tool registry + policy gates**
  - tool specs with input/output schemas
  - typed parsing for built-in tools
  - allow-list / deny-list execution policy
- **Robust orchestrator loop**
  - max steps/tool calls/runtime guardrails
  - provider retries with linear backoff
  - evented tool call/output/error lifecycle
- **Queue/scheduler primitives**
  - in-memory priority queue (`[p1]` high-priority lines)
  - bounded-concurrency batch runner (configurable)
- **Runtime-gate orchestration (Rust port)**
  - enforce minimum runtime + checklist completion gates
  - reusable `RuntimeGate` primitive with deadline/remaining semantics
  - start/status/stop lifecycle with JSON state + progress logs
- **Code intelligence scaffold (`/toolsets/code` + `src/code`)**
  - architecture planning contract + diff generation prompts in `/toolsets/code/prompts`
  - Rust module scaffold in `src/code` for `plan -> generate diff -> apply`
  - provider-backed planner/diff generator + git-apply applier
- **Work intelligence scaffold (`/toolsets/work`)**
  - non-technical harness workflow definitions (roadmapped via `toolsets/work/TODO.md`)

`/code` is retained as a compatibility alias that points operators to `/toolsets/code`.
- **Coding mode (timeboxed active execution)**
  - canonical interface: `repo + time`
  - guaranteed phase state machine per cycle: `architecture -> feature -> conformance -> cleanup -> pause` (repeats until deadline)
  - when repo is clean, architecture phase selects/builds the next feature task from internal docs/roadmap before feature execution
  - trait-based `WorkExecutor` abstraction (provider/tool agnostic)
  - built-in shell/cargo executor with command allowlist policy
  - human-readable phase events (`coding.phase`) include phase, reason, selected task, and result
  - optional user-session prompt input (`--prompt` / `--prompt-file`) threaded into cycle context and logs
- **Replay + eval baseline**
  - event taxonomy summaries
  - fixture-backed regression checks
- **Observability**
  - run-scoped IDs + sequence numbers
  - event taxonomy (`run.*`, `task.*`, `provider.*`, `tool.*`, `scheduler.*`, `cli.*`)

## Toolchain + local build checks

Run user-space readiness checks (no root assumptions):

```bash
./scripts/check_toolchain.sh
./scripts/build.sh
```

If Rust is missing, the checker prints rustup bootstrap commands.

## CLI

```bash
agent-harness --help
agent-harness status
agent-harness run --objective "what time is it"
agent-harness run --objective "what time is it" --allow-tool time.now
agent-harness batch --objectives-file ./fixtures/objectives.txt
# objectives format supports optional prefixes: [p1] high, [p0] normal
agent-harness gate start --checklist ./fixtures/gate_checklist.done.md --dry-run --dry-runtime-sec 3 --dry-heartbeat-sec 1 --poll-seconds 1
agent-harness gate status
agent-harness code --repo /path/to/repo --time 1h
agent-harness code --repo . --time 45m --executor shell --allow-cmd ls --plan-cmd "git status --short" --act-cmd "ls -la" --verify-cmd "git diff --stat"
agent-harness code --repo . --time 1h --prompt "optional session prompt"
agent-harness replay --path ./runs/events.jsonl
agent-harness replay --path ./runs/events.jsonl --latest-run
agent-harness eval --path ./runs/events.jsonl --run-id run-123
agent-harness loop --interval-seconds 60 --max-iterations 5 --objective "heartbeat time task"
```

With cargo:

```bash
cargo run -- status
cargo run -- run --objective "what time is it"
cargo run -- batch --objectives-file ./fixtures/objectives.txt
cargo run -- code --repo . --time 1h
cargo run -- code --repo . --time 1h --prompt "optional prompt"
cargo run -- replay --path ./runs/events.jsonl --latest-run
cargo run -- eval --path ./runs/events.jsonl --run-id run-123
```

`status` reports provider resolution (`requested_kind`, `resolved_kind`, `fallback_reason`) so fallback-to-stub behavior is explicit.

## Config

Pass `--config ./config.example.toml` or rely on defaults.

Environment overrides:

- `HARNESS_PROVIDER` (`echo`, `http`, `http-stub`)
- `HARNESS_MODEL`
- `HARNESS_EVENT_LOG`
- `HARNESS_PROVIDER_ENDPOINT`
- `HARNESS_PROVIDER_TIMEOUT_MS`
- `HARNESS_PROVIDER_MAX_RETRIES`
- scheduler knobs live in TOML (`scheduler.max_concurrent_tasks`, `scheduler.queue_poll_ms`)

⚠️ Coding mode now runs a provider-backed `plan -> diff -> apply` stage before verify/commit hooks.
Use a provider that can return unified diffs (`HARNESS_PROVIDER=http` with a working endpoint/model).
`echo` / `http-stub` are useful for scaffolding, but they will not generate meaningful code patches.

## Dogfood workflow (harness-on-harness)

```bash
./scripts/dogfood.sh
```

## Canonical operator interface: time + repo

For strict timeboxed coding execution against a target repository:

```bash
bash ./scripts/harness.sh --repo /path/to/repo --time 1h
```

Optional prompt plumbing (empty by default unless you provide one):

```bash
bash ./scripts/harness.sh --repo /path/to/repo --time 1h --prompt "your session prompt"
```

Also supports `--time 3600`, `--time 90m`, `--time 45s`.

The script now executes **coding mode** directly (active plan/act/verify cycles), rather than idle runtime waiting.

Recommended production-ish defaults:

```bash
agent-harness code --repo /path/to/repo --time 1h \
  --heartbeat-sec 30 --cycle-pause-sec 2 \
  --noop-streak-limit 3 --conformance-interval-unchanged 3 \
  --commit-each-cycle --push-each-cycle \
  --runtime-log-file ./runs/coding-runtime.log
```

## Safety notes for coding mode

- Commands run through an **allowlisted command policy** (`cargo`, `git` by default).
- Add explicit extras with `--allow-cmd <tool>` for non-default executables.
- Every meaningful cleanup cycle attempts git sync (`fetch` + `pull`), commit, and push (with explicit event/log status).
- Commit quality gate blocks internal-state-only diffs (for example `.harness/*`) unless `src/` or task-tied docs are also changed.
- Commit subjects are strict conventional commits (`feat|fix|docs|test|refactor|chore`) with file-scoped subjects; generic templates (including variants of `build a generalizable ...` / `harness: coding cycle`) are hard-rejected.
- Subject generation is deterministic from staged files, must reference changed scope, and still passes through short-window de-duplication.
- Hard anti-noop defaults: `noop_streak_limit=3` triggers a forced concrete scoped code-change task; if that forced cycle still has no meaningful diff, the run aborts explicitly.
- Single-instance lock per repo prevents parallel coding loops (`.git/.agent-harness-code.lock`) and fail-fast exits with `coding.lock.exists` (`fail_fast=true`) plus `coding.lock.acquired` on success.
- Task progression memory persists completed/attempted ids plus per-task selection history (`.harness/coding-progress.json`) so architecture ranking uses novelty + impact with cooldown to avoid repetitive picks.
- Explicit cycle counters are emitted: `noop_streak`, `forced_mutation`, `task_advanced`, `source_escalation`.
- Explicit `git.commit` and `git.push` events are emitted every cycle path (success, skip, blocked, failure) including subject/message/result metadata.
- Prompt input is optional and empty by default; no prompt content is hardcoded.
- Prompt values are threaded into cycle context/logs and command env (`OPENCLAW_USER_PROMPT`) only when supplied.

## Human-readable runtime stream format

Pass `--runtime-log-file` in coding mode to emit a first-class operator stream with entries shaped like:

- timestamp (`[unix_epoch]`)
- phase label (`architecture|feature|conformance|cleanup|pause`)
- concise bullets (`reason`, selected task, result)
- explicit `next` step

Cleanup logs include git sync outcomes each cycle:

- `git_fetch`: success/failure
- `git_pull`: clean merge vs conflict/failure
- `conflict_resolution`: unresolved conflict status
- `commit` summary
- `push` outcome

## Troubleshooting quick hits (coding mode)

- `coding.lock.exists`: another run already owns the repo lock. Stop the active run or remove stale lock file if the process is gone.
- `forced scoped code-change task produced no meaningful diff`: your act stage is not producing real scoped changes. Update `--act-cmd` to perform concrete edits.
- `commit subject rejected by quality gate`: generated subject was generic or did not mention changed scope. Check staged files and subject generation inputs.
- repeated `git.push` blocked/failure: inspect remote auth/upstream tracking; push events now include explicit result + detail fields.

## Extensibility points

- Implement additional executors by conforming to `WorkExecutor` (`plan/act/verify`).
- Keep provider/tool integrations outside executor policy boundaries for portability.
- Add new hook types by extending `run_cycle_hooks` and emitting `coding.cycle.hook` data.

## Contributor notes

- Keep features provider-agnostic and config-driven.
- Prefer additive extension points over hardcoded behavior.
- Add fixture coverage when introducing new event kinds or stop reasons.
