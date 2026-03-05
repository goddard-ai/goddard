---
id: app-screen-spec
status: ACTIVE
links:
  - type: Extends
    target: spec/app/index.md
  - type: Depends-On
    target: spec/manifest.md
  - type: Relates-To
    target: spec/vision.md
---

# Spec Screen

## Goal
Allow users to navigate and steer repository specifications from within the desktop app.

## Hypothesis
We believe that in-context spec editing requests improve alignment between human intent and AI execution.

## Actors
- Developer refining requirements
- Reviewer correcting or clarifying intent

## Conceptual State Machine
`Repo Selection Required -> Repo Selected -> Spec File Browsing -> Spec File Viewing -> Edit Prompt Submitted`

## Behavioral Contract
- Entering Spec domain opens a full-screen repository selection modal.
- Modal behavior:
  - shows most-interacted repositories by default
  - supports search + Enter-to-select flow
- After repository selection:
  - internal sidebar lists available spec files
  - selecting a file displays its content in main area
- Bottom input (`Edit the spec...`) submits AI instruction to modify specs for selected repository with full-spec awareness.

## Data Requirements
- Repository selection data
  - repository id/name
  - interaction frequency/recency
- Spec file index
  - file path
  - title/label
  - hierarchy/group metadata
- Spec content view model
  - current file content
  - last updated metadata
- Edit request envelope
  - selected repository
  - selected/current file context
  - user instruction text

## Constraints
- Must prevent ambiguity about active repository context.
- Must preserve graph navigation semantics of spec files.

## Non-Goals
- Bypassing spec-first governance.
- Turning spec view into a generic code editor.
