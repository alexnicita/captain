# Contributing

## Goals

Keep this harness reusable and publishable:

- avoid project-specific assumptions
- keep behavior config-driven
- preserve provider/tool pluggability
- keep event taxonomy stable or migration-noted

## Development loop

1. `./scripts/check_toolchain.sh`
2. Implement a small increment
3. `cargo test --all-targets --all-features`
4. `./scripts/dogfood.sh`
5. Add/adjust fixture coverage if event behavior changed

## PR checklist

- [ ] feature is generic (no local hardcoding)
- [ ] docs updated (`README.md`, `ARCHITECTURE.md`, runbook if needed)
- [ ] replay/eval fixtures updated where appropriate
- [ ] no secrets in config/examples
