---
id: app-screen-search
status: ACTIVE
links:
  - type: Extends
    target: spec/app/index.md
  - type: Depends-On
    target: spec/data-flows.md
---

# Search Screen

## Goal
Enable a single global search interface across all major Goddard desktop domains.

## Hypothesis
We believe that cross-domain retrieval from one query entry point reduces navigation friction and discovery time.

## Actors
- Developer trying to locate artifacts quickly
- Maintainer correlating items across domains

## Conceptual State Machine
`Idle -> Querying -> Results Grouped -> Result Opened`

## Behavioral Contract
- Search bar is placed at top of side pane.
- Query scope includes: sessions, pull requests, tasks, roadmap proposals, specs, pages, extensions.
- Results should preserve domain context so users understand where each hit belongs.
- Selecting a result opens the appropriate destination screen/tab.

## Data Requirements
- Unified searchable index abstraction containing:
  - entity type
  - entity id
  - display title/snippet
  - repository context (if applicable)
  - recency/score metadata
- Recent searches / quick history (optional preference-level data)

## Constraints
- Must degrade gracefully when some domain indexes are temporarily unavailable.
- Must avoid ambiguous result routing.

## Non-Goals
- Semantic code search inside repository source trees.
- Replacing specialized search tools in external editors.
