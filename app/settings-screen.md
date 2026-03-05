---
id: app-screen-settings
status: ACTIVE
links:
  - type: Extends
    target: spec/app/index.md
  - type: Relates-To
    target: spec/non-goals.md
---

# Settings Screen

## Goal
Reserve a stable navigation slot for future desktop configuration capabilities.

## Hypothesis
We believe that exposing Settings as a visible placeholder now creates a predictable mental model for future account and workspace controls.

## Actors
- Any desktop user expecting configurable behavior

## Conceptual State Machine
`Placeholder Visible -> Future Configuration (deferred)`

## Behavioral Contract
- Settings appears in left navigation.
- Current state is explicitly marked as "coming soon."
- Placeholder should not imply unavailable controls are currently active.

## Data Requirements
- Minimal placeholder content only.
- No persisted settings contract is defined in this phase.

## Constraints
- Must avoid false affordances that suggest complete settings support.

## Non-Goals
- Defining full settings taxonomy in this phase.
- Implementing account/security preferences here.
