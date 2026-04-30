# Work Toolset

Non-code workflow planning area for broader agent operations.

## First cycle spec

- `cycle-spec.md` defines the first work harness cycle: `customer-or-market-research`.
- The spec covers inputs, constraints, cycle steps, outputs, commit/log policy, and acceptance criteria.
- Work cycles are for operator-facing artifacts such as research briefs, ops/admin summaries, outreach drafts, strategy synthesis, and knowledge-base maintenance outputs.

## Safety posture

Work toolset cycles default to no external side effects. They may read allowed sources and write local/operator-approved artifacts, but they must not contact customers, publish messages, modify SaaS records, or send email unless a later cycle explicitly grants that action.
