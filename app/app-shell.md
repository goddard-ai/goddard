---
id: app-shell-navigation-model
status: ACTIVE
links:
  - type: Extends
    target: spec/app/index.md
  - type: Depends-On
    target: spec/architecture.md
  - type: Depends-On
    target: spec/data-flows.md
  - type: Relates-To
    target: spec/cli/interactive.md
---

# App Shell & Navigation Model

## Goal
Define the desktop application's global layout and navigation behavior so all domain screens behave consistently.

## Hypothesis
We believe that a predictable, VS Code-like shell lowers cognitive load for users who manage many concurrent AI operations.

## Actors
- Developer switching across domains quickly
- Reviewer opening multiple detail contexts in parallel

## Conceptual State Machine
`Launching -> Auth Required -> Workspace Ready -> Domain Navigating -> Detail Reviewing`

## Behavioral Contract
- The far-left Main Tab is always present and cannot be closed.
- Selecting a left-sidebar domain icon changes Main Tab content.
- Detail interactions open new tabs on the right side end of tab strip.
- Maximum open tabs: 20.
- When tab limit is reached, the app must require explicit user action before displacing existing tabs.

## Data Requirements
- Tab registry
  - tab id
  - tab type (main/domain/detail)
  - linked domain entity id (if detail)
  - open timestamp / last focused timestamp
- Navigation state
  - active sidebar domain
  - active tab id
- Persisted workspace preferences
  - recently opened domains/tabs
  - optional pin/favorite metadata

## Constraints
- No hidden tab creation side effects.
- Must preserve user context during live data refreshes.
- Shell behavior must be consistent across all domain screens.

## Non-Goals
- Modeling window manager behavior beyond app-level tabs.
- Replacing native OS-level multi-window paradigms in this phase.
