---
id: app-screen-extensions
status: ACTIVE
links:
  - type: Extends
    target: spec/app/index.md
  - type: Relates-To
    target: spec/product.md
---

# Extensions Screen

## Goal
Enable discovery and management of Goddard extensions from a dedicated workspace surface.

## Hypothesis
We believe that making extension discovery native to the app will increase ecosystem usage and customization.

## Actors
- Developer installing workflow enhancements
- Maintainer auditing active extensions

## Conceptual State Machine
`Catalog Loading -> Search Active -> Extension Selected -> Manage Action`

## Behavioral Contract
- Extensions domain provides a dedicated sidebar for extension search.
- Search results should communicate extension identity and relevance clearly.
- Selecting an extension should present management affordances appropriate to lifecycle state.

## Data Requirements
- Extension catalog records
  - extension id/name
  - publisher/source attribution
  - summary/description
  - version marker
  - compatibility markers (if available)
  - install/enable state
- Search and filter state
  - query text
  - active filters/sort

## Constraints
- Must distinguish trusted vs unverified extension sources where metadata exists.
- Must preserve deterministic extension state reflection in UI.

## Non-Goals
- Executing arbitrary extension code without platform safeguards.
- Replacing extension authoring workflows.
