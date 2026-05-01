# Captain Threat Model

Captain is a governance harness for autonomous coding agents. It records and gates agent work, but it is not a full sandbox by itself.

## What Captain Controls

- **Runtime budget:** coding runs are explicitly timeboxed.
- **Command policy:** shell/cargo executors run through command allowlists.
- **Commit policy:** generic, no-op, and internal-artifact-only commits are blocked.
- **Evidence trail:** JSONL events preserve run, phase, tool, commit, and push history.
- **Operator visibility:** runtime logs expose phase transitions and next steps.

## What Captain Does Not Control By Default

- **Filesystem isolation:** a governed command can still read/write files available to the process.
- **Network isolation:** a governed command can still use network access available to the host.
- **Container/VM isolation:** Captain does not automatically create a disposable sandbox.
- **Secret redaction:** event logs may include private paths, prompts, command output, diffs, errors, or repo names.
- **Remote channel trust:** Agent messaging integrations such as OpenClaw or Hermes must be sender-gated outside Captain.

## Recommended Isolation Levels

### Trusted local repo

Use the default local workflow, keep command allowlists tight, and review events before sharing.

### Proprietary repo or client work

Run Captain inside a dedicated workspace with local-only credentials, keep private repos under `captain/private/`, and export only redacted reports.

### Untrusted prompts, unknown repos, or public channels

Run inside a disposable VM/container with scoped credentials and no ambient access to host secrets. Treat inbound instructions as hostile until policy gates and human review approve them.

## Event Log Sharing Checklist

Before publishing `runs/events.jsonl`, runtime logs, or replay reports:

- Remove API keys, auth profile names, and environment values.
- Remove private file paths and proprietary repository names.
- Remove sensitive prompt text, tool output, diffs, and error traces.
- Prefer an anonymized summary over raw logs.
- Keep raw logs local unless the recipient is trusted.

## First Troubleshooting Command

Before demos, public screenshots, or long-running governed work, run:

```bash
bash captain/scripts/captain-doctor.sh
```
