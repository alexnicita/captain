# Work Toolset Cycle Spec: Customer or Market Research

## Objective

Define the first non-code `work` harness cycle: `customer-or-market-research`.

The cycle turns a bounded research question into an operator-readable brief with evidence, assumptions, follow-up tasks, and a replayable log trail. It is intended for market/customer research, ops/admin routines, messaging research, and strategy synthesis where the output is a document or decision packet rather than a code diff.

## Inputs

Each cycle accepts:

- **Objective:** One sentence describing the research or operations goal.
- **Scope:** Product, customer segment, market, competitor, workflow, or internal process under review.
- **Timebox:** Default 30 minutes unless the operator supplies a shorter budget.
- **Allowed sources:** Explicit source list or source policy, such as public web, local notes, repository docs, or operator-provided files.
- **Output target:** Markdown brief path, issue draft, message draft, or JSONL event stream.
- **Decision owner:** Optional person/team who will consume the result.

## Constraints

- No secrets or credentials may be copied into outputs, prompts, events, or summaries.
- Prefer primary sources and include links or file paths for claims that affect decisions.
- Label assumptions explicitly when evidence is incomplete.
- Keep outreach/messaging drafts separate from factual research notes.
- Do not contact customers, publish messages, modify SaaS records, or send email unless a later cycle explicitly grants that side effect.
- Stay within the timebox; if the cycle cannot finish, produce a partial brief with next questions.

## Cycle Steps

1. **Frame:** Restate the objective, scope, decision owner, timebox, and allowed sources.
2. **Collect:** Gather evidence from allowed sources only. Capture source names, URLs, file paths, and timestamps when available.
3. **Synthesize:** Convert evidence into findings, risks, opportunities, and unknowns.
4. **Draft output:** Produce the requested brief/message/ops artifact in the output target.
5. **Review gates:** Check for secrets, unsupported claims, unmarked assumptions, and side effects.
6. **Log:** Emit or append a compact cycle summary that includes objective, source count, output path, blocked items, and next-step recommendations.

## Outputs

A successful cycle produces:

- A Markdown brief or operator-facing artifact.
- A source list with enough detail for later replay or verification.
- A decision summary with recommended next action.
- A follow-up task list for unresolved questions.
- Structured cycle metadata suitable for JSONL events or run summaries.

## Commit and Log Policy

- Commit generated work artifacts only when the operator has requested repository-backed work products.
- Never commit private notes, credentials, raw customer data, or unredacted transcripts.
- Commit subjects should use the changed artifact scope, for example `docs(work): add market research brief`.
- Logs should include objective, source count, output path, and completion status.
- Logs should not include secret values, credential names, or sensitive personal data.

## Acceptance Criteria

A `customer-or-market-research` work cycle is acceptable when:

- The objective and scope are visible at the top of the output.
- Every decision-relevant claim is sourced or marked as an assumption.
- The output includes concrete next steps.
- The cycle records whether side effects were intentionally disabled.
- The final artifact is safe to share with the stated decision owner.
