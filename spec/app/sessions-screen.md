---
id: app-screen-sessions
status: ACTIVE
links:
  - type: Extends
    target: spec/app/index.md
  - type: Depends-On
    target: spec/runtime-loop.md
  - type: Depends-On
    target: spec/daemon/pr-feedback-one-shot.md
  - type: Relates-To
    target: spec/cli/interactive.md
---

# Sessions Screen

## Goal
Give users a real-time control surface for all Goddard sessions, including monitoring, drill-down review, and feedback.

## Hypothesis
We believe that surfacing session state, latest activity, and one-click actions will increase successful session completion and reduce blocked work.

## Actors
- Developer supervising active AI work
- Reviewer providing follow-up instructions

## Conceptual State Machine
`List Loading -> List Ready -> Session Selected -> Detail Chatting -> Follow-up Triggered`

## Behavioral Contract
- Main Sessions view lists sessions by status and repository context.
- Each session cell shows:
  - Current status (Active, Blocked, Done)
  - Repository name
  - Associated pull request name (if available)
  - Latest AI activity snippet
- Session cell actions:
  - Open related pull requests in new tab
  - Open proposed changes in new tab
- Selecting session body opens a dedicated session detail tab.
- Session detail tab includes full message history and bottom chat bar for feedback.
- Sending feedback from detail view should surface related spec context for user reference.
- Main Sessions view provides filters:
  - Status (Blocked, Unblocked, All)
  - Repository
- Main Sessions view includes a new-session chat bar with repository selection on input expansion.

## Data Requirements
- Session summary records
  - session id
  - status
  - repository id/name
  - linked PR id/title (optional)
  - latest activity text + timestamp
- Session detail records
  - ordered message timeline
  - sender role metadata
  - feedback thread continuity identifiers
- Filter and composition data
  - available repositories
  - active filter values
- New-session initiation payload
  - repository selection
  - user prompt

## Constraints
- Must support frequent live updates without dropping user scroll/focus context.
- Must communicate blocked vs unblocked state clearly.

## Non-Goals
- Full diff editing inside session detail.
- Replacing repository-level PR workflows.
