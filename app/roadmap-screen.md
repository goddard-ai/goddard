---
id: app-screen-roadmap
status: ACTIVE
links:
  - type: Extends
    target: spec/app/index.md
  - type: Relates-To
    target: spec/product.md
---

# Roadmap Screen

## Goal
Provide visibility into current proposals and roadmap priorities across repositories.

## Hypothesis
We believe that a dedicated roadmap surface helps teams prioritize AI effort against strategic goals.

## Actors
- Product/engineering lead prioritizing initiatives
- Developer aligning execution with approved proposals

## Conceptual State Machine
`Roadmap Loading -> Proposals Ready -> Filters Applied -> Proposal Focused`

## Behavioral Contract
- Roadmap view lists proposals with prioritization context.
- Internal sidebar filters:
  - Repository
  - Priority
- Proposal selection opens richer detail context in tab content.

## Data Requirements
- Proposal records
  - proposal id
  - title/summary
  - repository linkage
  - priority level
  - status/lifecycle marker
  - last updated timestamp
- Filter facets
  - repository options
  - priority buckets

## Constraints
- Prioritization metadata must remain consistent across list and detail surfaces.
- Must support cross-repository comparisons without losing per-repo context.

## Non-Goals
- Full portfolio planning suite.
- Replacing dedicated PM tooling for advanced dependency planning.
