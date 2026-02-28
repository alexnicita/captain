# KB Framework (Refined) — Seaport & Co. / Alex Nicita

## Objective
Maintain an **evidence-first intelligence architecture** that separates facts from inference, supports decision-making, and can be refreshed on a weekly operating rhythm.

## Architecture Principles
1. **Source-first**: every meaningful claim links to a URL.
2. **Fact vs inference split**: no blended statements.
3. **Confidence tagging**: each claim labeled High / Medium / Low.
4. **Time-scoped intelligence**: all entries include observed date + review date.
5. **Actionability over volume**: every section should inform a decision, experiment, or risk mitigation.

## Recommended Structure

```text
kb/
  README.md
  sources/
    index.md                    # canonical URL list + source reliability
    access-notes.md             # paywalls/login blocks/tooling limits
    snapshots/
      2026-02-28.md             # dated research snapshots

  entities/
    alex-nicita.md              # identity timeline, channels, project footprint
    seaport-and-co.md           # positioning, offers, proof density, evolution

  intelligence/
    facts.md                    # normalized verified claims only
    assumptions.md              # explicit assumptions with test plans
    confidence-register.md      # claim_id -> confidence rationale

  evidence/
    project-evidence-map.md     # client/project proof matrix
    social-footprint.md         # channel status + accessibility + signals
    web-presence-audit.md       # crawl/index depth, discoverability health

  strategy/
    opportunity-map.md          # ranked opportunities + expected impact
    risk-map.md                 # risks, likelihood, impact, mitigations
    positioning-evolution.md    # messaging changes over time

  primary-research/
    open-questions.md           # interview/validation questions
    interview-notes/            # optional future captures
```

## Claim Data Model (minimal schema)

Use this structure inside intelligence docs:

```yaml
claim_id: C-YYYYMMDD-001
statement: "Seaport sitemap currently exposes only / and /404."
type: verified_fact # verified_fact | inference | assumption
confidence: high
observed_at: 2026-02-28
source_urls:
  - https://seaportand.co/sitemap.xml
notes: "Machine-readable sitemap capture via web_fetch"
review_by: 2026-03-15
```

## Confidence Rubric
- **High**: direct first-party or machine-readable source confirms claim.
- **Medium**: supported by indirect evidence or partially blocked sources.
- **Low**: weak/ambiguous source, unresolved identity matching, or strong assumptions.

## Source Reliability Tiers
- **Tier A (Primary)**: official domains, first-party machine-readable files, direct APIs.
- **Tier B (Secondary)**: reputable third-party profiles/directories.
- **Tier C (Contextual)**: search snippets, scraped listings, ambiguous profiles.

## Operating Workflow
1. **Collect**: ingest first-party + third-party URLs.
2. **Normalize**: convert to atomic claims.
3. **Classify**: fact vs inference vs assumption.
4. **Score**: assign confidence + source tier.
5. **Synthesize**: update opportunity/risk maps.
6. **Decide**: record owner + next action + review date.

## Cadence
- **Weekly**: sources index, social footprint, project evidence map.
- **Biweekly**: opportunity/risk reprioritization.
- **Monthly**: positioning evolution summary + stale assumption pruning.

## Guardrails
- No unnamed strategic claim enters active docs without at least one source URL.
- If source access is blocked (login/paywall), capture it in `sources/access-notes.md` and downgrade confidence.
- Keep identity disambiguation explicit when names are non-unique.

## Immediate Next Expansion Priorities
1. Build `intelligence/facts.md` and `intelligence/assumptions.md` from latest dossier.
2. Populate `evidence/project-evidence-map.md` with current proof gaps and required artifacts.
3. Maintain `primary-research/open-questions.md` as the live interview agenda.
