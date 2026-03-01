# /code

This directory contains the LLM coding pipeline contract for the harness.

## Purpose

Make coding mode explicitly architecture-first and diff-driven:

1. Plan architecture for the selected task
2. Generate a scoped unified diff
3. Apply the diff in-repo
4. Verify + commit + push via existing coding loop hooks

## Layout

- `prompts/architecture.md` — plan prompt template
- `prompts/diff.md` — patch generation prompt template
- `policies/commit-quality.md` — commit subject + quality constraints
- `tasks/` — optional task packs per repository/domain

## Runtime module mapping

Rust implementation lives in `src/code/`:

- `planner.rs` => provider-backed architecture planner
- `diff.rs` => provider-backed unified-diff generator
- `apply.rs` => git apply executor
- `engine.rs` => plan->diff->apply orchestration for a coding cycle

This keeps prompt/policy assets in `/code` and executable logic in `src/code`.
