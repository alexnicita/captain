# Final hardening dry-run evidence

Date: 2026-02-28 UTC

## Command

```bash
HARNESS_EVENT_LOG=/tmp/tmp.AUvxqWM6Ue/events.jsonl cargo run -- code \
  --repo /tmp/tmp.AUvxqWM6Ue/work \
  --time 6s \
  --executor shell \
  --allow-cmd bash \
  --plan-cmd "git status --short" \
  --act-cmd "bash -lc 'printf \"// cycle %s\\n\" \"$OPENCLAW_CODING_CYCLE\" >> src/lib.rs'" \
  --verify-cmd "git diff --stat" \
  --heartbeat-sec 1 \
  --cycle-pause-sec 0 \
  --commit-each-cycle \
  --push-each-cycle \
  --runtime-log-file /tmp/tmp.AUvxqWM6Ue/runtime.log \
  --progress-file /tmp/tmp.AUvxqWM6Ue/work/.harness/coding-progress.json
```

## Evidence: varied task selection

From runtime log:

- cycle 1 architecture task: `Strengthen lock refusal observability in src/main.rs`
- cycle 3 architecture task: `[ ] Harden lock handling for coding mode`

This shows task ranking/cooldown moving between high-impact fallback and roadmap tasks instead of repeating one fallback loop.

## Evidence: informative commit subject generation path

From `git.commit` events:

- `feat(harness): implement scoped code updates in .harness/coding-progress.json, src/lib.rs`
- `result=ok`, `subject=implement scoped code updates in .harness/coding-progress.json, src/lib.rs [cycle-refresh]`

This confirms file-aware conventional commit subject output + explicit event payload (`subject`, `message`, `result`, `success`).
