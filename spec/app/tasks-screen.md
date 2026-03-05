---
id: app-screen-tasks
status: ACTIVE
links:
  - type: Extends
    target: spec/app/index.md
  - type: Relates-To
    target: spec/product.md
---

# Tasks Screen

## Goal
Expose task management in-app with JulesUp integration as a first-class workflow surface.

## Hypothesis
We believe that colocating tasks beside sessions and PRs improves execution continuity from planning to delivery.

## Actors
- Developer managing assigned work
- Maintainer coordinating AI and human task queues

## Conceptual State Machine
`Tasks Loading -> Backlog Ready -> Task Focused -> Task Updated`

## Behavioral Contract
- Tasks screen presents actionable task inventory.
- Task interactions should preserve repository context when available.
- Navigation from task to related session/PR/spec should be direct when links exist.

## Data Requirements
- Task records
  - task id
  - title/summary
  - status
  - priority (if available)
  - repository linkage (optional)
  - related session/PR/spec references (optional)
- Integration metadata
  - source system attribution (JulesUp)
  - sync/update timestamp

## Constraints
- Must tolerate partial linkage (tasks without complete graph references).
- Must preserve source-of-truth ownership for external task fields.

## Non-Goals
- Rebuilding JulesUp feature parity inside Goddard desktop.
- Introducing a separate task model disconnected from integration source.
