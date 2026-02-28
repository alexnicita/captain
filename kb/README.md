# Knowledge Base Architecture (Seaport & Co / Alex Nicita)

## Purpose
Create an evidence-first operating KB that supports strategy, delivery, pipeline qualification, and recurring intelligence updates.

## Design Principles
1. **Evidence before opinion** (every non-obvious claim linked to source).
2. **Time-bounded truth** (stamp observations with date).
3. **Decision-oriented structure** (organize for action, not archive).
4. **Assumption hygiene** (explicitly tag assumptions and confidence).

## Proposed Structure

```text
kb/
  README.md
  sources/
    index.md                  # canonical source list + reliability notes
    snapshots/
      2026-02-28.md           # point-in-time captures
  entities/
    alex-nicita.md            # founder profile, network, theses, timeline
    seaport-and-co.md         # firm profile, offers, proof assets, positioning
    partners.md               # referral ecosystem, VCs, operators, studios
  offers/
    advisory.md               # strategic advisory package
    architecture.md           # architecture sprint package
    build-cell.md             # build execution package
  market/
    icp-seed-series-a.md      # ideal customer profile segment 1
    icp-smb-transformation.md # ideal customer profile segment 2
    competitors.md            # alternatives and differentiation map
  pipeline/
    qualification-rubric.md   # fit scoring model
    objections.md             # recurring objections + rebuttals
    win-loss.md               # deal postmortems
  delivery/
    operating-system.md       # cadence, rituals, QA gates
    playbooks/
      discovery.md
      architecture-review.md
      launch-readiness.md
  insights/
    weekly-briefs/
      2026-W09.md
    monthly-reviews/
      2026-02.md
  decisions/
    ADR-0001-positioning.md   # architecture decision records
```

## Metadata Standard (frontmatter)
Use YAML frontmatter in each KB file:

```yaml
owner: alex
last_updated: 2026-02-28
status: active
confidence: high|medium|low
evidence_links:
  - https://...
assumptions:
  - ...
next_review: 2026-03-15
```

## Core Workflows
1. **Capture**: ingest signals from web, calls, delivery retros.
2. **Validate**: label as verified vs assumption.
3. **Synthesize**: convert into opportunities/risks/decisions.
4. **Activate**: update backlog and operating plans.
5. **Review**: weekly refresh + monthly pruning.

## Governance
- **Update cadence**: weekly for pipeline/market, biweekly for offers, monthly for entity profiles.
- **Quality gate**: no strategic claim enters active docs without at least one source URL.
- **Deprecation rule**: archive stale assumptions after 60 days if unvalidated.

## Immediate Next Files to Create
1. `kb/sources/index.md`
2. `kb/entities/alex-nicita.md`
3. `kb/entities/seaport-and-co.md`
4. `kb/pipeline/qualification-rubric.md`
5. `kb/delivery/operating-system.md`
