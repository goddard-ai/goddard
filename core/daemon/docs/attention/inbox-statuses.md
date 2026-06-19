# Inbox Statuses

> Inbox rows track the current workflow state for daemon-owned work, not a history of notifications. This page defines the statuses a user or client may see and how later daemon activity can change them.

## Core idea

- Inbox status is the user's workflow state for the current daemon-owned entity.
- Later daemon attention can reopen rows when the entity needs attention again.
- Status changes are not a complete event log; they describe what the user should do with the row now.

## Statuses

- `unread`
  - The daemon says the entity needs human attention.
  - This is the normal state after new daemon attention arrives.
- `read`
  - The user acknowledged the row and no newer daemon attention has arrived.
- `replied`
  - The user replied to a related session and attention is not needed until new daemon activity.
- `saved`
  - The user intentionally parked the row for later.
- `archived`
  - The user hid the row from the active workflow.
- `completed`
  - The entity is no longer a current concern according to that entity's own lifecycle.

## Completion

- Completion is entity-specific.
- A completed session inbox row means the user completed the session concern.
- A completed pull request row means the pull request is no longer open, such as after merge or closure.
- Completion does not delete the underlying session or managed pull request record.
- If the entity later produces supported daemon attention, the row can become active again.

## Priority

- `normal` rows participate in the standard attention queue.
- `low` rows remain relevant but sort behind normal-priority work.
- Priority is a user workflow signal and should not be casually overwritten by daemon refreshes.

## Reopening

- New daemon attention can reopen rows that were read, replied, saved, archived, or completed.
- This keeps the row focused on current attention rather than historical acknowledgement.
- Clients should present reopened rows as current daemon attention, not as duplicate notifications.
