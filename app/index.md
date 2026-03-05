---
id: app-desktop-index
status: ACTIVE
links:
  - type: Extends
    target: spec/vision.md
  - type: Extends
    target: spec/product.md
  - type: Depends-On
    target: spec/architecture.md
  - type: Depends-On
    target: spec/data-flows.md
  - type: Relates-To
    target: spec/non-goals.md
  - type: Leads-To
    target: spec/app/app-shell.md
  - type: Leads-To
    target: spec/app/sessions-screen.md
  - type: Leads-To
    target: spec/app/pull-requests-screen.md
  - type: Leads-To
    target: spec/app/search-screen.md
  - type: Leads-To
    target: spec/app/tasks-screen.md
  - type: Leads-To
    target: spec/app/roadmap-screen.md
  - type: Leads-To
    target: spec/app/spec-screen.md
  - type: Leads-To
    target: spec/app/pages-screen.md
  - type: Leads-To
    target: spec/app/extensions-screen.md
  - type: Leads-To
    target: spec/app/settings-screen.md
---

# Desktop App Intent Index

## Goal
Provide a unified desktop workspace for Goddard operations so developers can run sessions, review outputs, and steer work from one visual surface instead of fragmented terminal + web tooling.

## Hypothesis
We believe that consolidating sessions, pull requests, specs, tasks, and roadmap context into one desktop app will reduce context switching and speed up AI-assisted delivery.

## Big Picture
The desktop app is an alternative interface to the same Goddard runtime used by CLI and backend flows. It is not a forked product surface with separate logic. The app should reflect the same source of truth for identity, repository context, and real-time activity.

## Primary Actors
- Developer/operator managing one or more repositories
- Reviewer giving feedback on AI output
- Maintainer monitoring throughput and blockers

## Global UI Model
- IDE-like shell with:
  - Persistent left navigation icons by domain
  - Persistent non-closable Main Tab
  - Additional closable detail tabs (max 20 concurrent)
- Navigation icon selection updates Main Tab content.
- Drill-down interactions open domain-specific detail tabs.

## Shared Data Requirements
All screens consume normalized, real-time domain records with stable identities:
- Repository
- Session
- Pull request
- Message/activity event
- Task
- Roadmap proposal
- Spec file metadata/content
- Page metadata/content
- Extension metadata
- User workspace preferences (filters, recents)

## Cross-Cutting Constraints
- Must remain lightweight (Tauri-first footprint expectations).
- Must authenticate against existing Goddard backend and use `@goddard-ai/sdk` authority model.
- Must handle streaming updates gracefully for high-churn views.

## Non-Goals
- Replacing CLI for advanced automation and CI usage.
- Implementing a full in-app code editor.

## Screen Map
- Shell & tab model: [`app-shell.md`](./app-shell.md)
- Sessions: [`sessions-screen.md`](./sessions-screen.md)
- Pull requests: [`pull-requests-screen.md`](./pull-requests-screen.md)
- Search: [`search-screen.md`](./search-screen.md)
- Tasks: [`tasks-screen.md`](./tasks-screen.md)
- Roadmap: [`roadmap-screen.md`](./roadmap-screen.md)
- Spec: [`spec-screen.md`](./spec-screen.md)
- Pages: [`pages-screen.md`](./pages-screen.md)
- Extensions: [`extensions-screen.md`](./extensions-screen.md)
- Settings: [`settings-screen.md`](./settings-screen.md)
