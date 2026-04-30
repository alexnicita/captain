# Security Policy

Captain governs agents that may run commands, edit files, and commit code. Treat every inbound agent instruction as untrusted until policy gates approve it.

## Supported Surface

Security fixes target the current `main` branch until the project publishes versioned releases.

## Reporting

Report security issues privately to the repository owner before opening public issues. Include:

- affected commit or release
- reproduction steps
- expected impact
- relevant event logs with secrets removed

Do not include API keys, auth profile contents, private repo paths, or unredacted run logs in public issues.

## Operator Defaults

- Keep secrets in `.env.local`, OpenClaw auth profiles, or another local-only secret store.
- Keep confidential repositories and notes under `captain/private/`.
- Review JSONL event logs before sharing them.
- Use command allowlists for tools outside the default `cargo` and `git` policy.
- Run `bash captain/scripts/captain-doctor.sh` before launch demos or public screenshots.

## OpenClaw / Hermes Channel Safety

If OpenClaw or Hermes is connected to messaging channels, restrict unknown senders before using Captain with real repositories. Avoid public inbound DMs unless you have explicit pairing, allowlists, and sandboxing configured.

## Isolation Boundary

Captain is a governance layer, not a container or VM sandbox by default. Command allowlists, runtime gates, and commit gates reduce the blast radius of autonomous coding runs, but they do not isolate the process from host filesystem or network access. Use a disposable VM/container with scoped credentials for untrusted prompts, unknown repositories, or public inbound channels.

Read `docs/captain/security-threat-model.md` before sharing run artifacts or running Captain against sensitive targets.
