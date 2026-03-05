---
id: app-screen-pull-requests
status: ACTIVE
links:
  - type: Extends
    target: spec/app/index.md
  - type: Depends-On
    target: spec/data-flows.md
  - type: Relates-To
    target: spec/cli/interactive.md
---

# Pull Requests Screen

## Goal
Provide a repository-aware, centralized view of Goddard-generated pull requests and their lifecycle.

## Hypothesis
We believe that grouping PRs by status and repository in one place reduces review latency and improves closure rates.

## Actors
- Developer reviewing AI-generated PRs
- Maintainer triaging open vs closed outcomes

## Conceptual State Machine
`List Loading -> List Ready -> Filter Applied -> PR Opened`

## Behavioral Contract
- Main view lists pull requests associated with Goddard sessions.
- Internal sidebar filters:
  - Status (Open, Closed)
  - Repository
- Selecting a PR opens details in a new tab.

## Data Requirements
- PR summary records
  - PR id
  - title
  - status
  - repository id/name
  - author/source attribution
  - updated timestamp
- PR detail linkage
  - URL/reference for full review context
  - related session id (if available)
- Filter data
  - repository list
  - status counts

## Constraints
- Live status transitions (open/closed) must reconcile cleanly in filtered views.
- Must handle repositories with large PR volumes without blocking UI.

## Non-Goals
- Full code review editor in-app.
- Replacing host provider PR pages for deep review tasks.
