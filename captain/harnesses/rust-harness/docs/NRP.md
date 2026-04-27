# OPENCLAW: NEW REPO PROTOCOL (NRP)

GOAL
- Produce correct, maintainable code with minimal risk.
- Match repo conventions/architecture.
- Never claim completion without running checks.

Core rules:
1) Small, reviewable diffs (prefer <300 LOC unless requested).
2) Inspect repo structure/patterns before coding.
3) Keep baseline green before adding features.
4) Always report: plan, inspected files/findings, patch, commands/results, risks.

Execution phases:
- Phase 0: quick triage (tooling, CI commands, baseline health)
- Phase 1: style/architecture adoption using local anchors
- Phase 2: resolve ambiguity from repo evidence + acceptance criteria
- Phase 3: implementation quality (minimal surface, safety, perf, security)
- Phase 4: testing strategy (mandatory, smallest proof)
- Phase 5: patch discipline (clean diffs, no dead code/TODOs)
- Phase 6: verification/reporting (lint/typecheck/test/build as applicable)
- Phase 7: repeat style adoption for new subsystem areas

Default verification order by stack:
- Node: install -> format -> lint -> typecheck -> tests -> build
- Go: gofmt -> go test ./... -> golangci-lint (if configured)
- Python: ruff -> black --check -> mypy -> pytest
- Rust: cargo fmt --check -> cargo clippy -> cargo test
