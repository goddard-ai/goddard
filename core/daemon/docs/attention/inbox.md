# Inbox

> The daemon-local inbox is the list of Goddard work items that may need a human decision now. Each row represents one current session or managed pull request, so clients can triage attention without inventing their own rules.

## Core idea

- The daemon-local inbox helps humans decide which daemon-owned work needs attention now.
- It stores current attention state, not notification history.
- Each row belongs to exactly one daemon-owned entity.

## Supported entities

- Sessions.
- Managed pull requests.
- Each entity type defines what completion means for its own row.

## Daemon ownership

- The daemon owns row creation and attention refreshes.
- Clients may list rows and update user workflow state.
- Clients must not create rows or infer daemon attention independently.
- When new daemon attention arrives, the daemon can reopen a row that a user previously read, replied to, saved, archived, or completed.

## Client operations

- List rows using daemon ordering and filtering.
- Update one row by daemon entity id.
- Bulk-update rows as one user workflow action.
- Complete a session's inbox concern through entity-specific validation.
- Stream daemon-published inbox item updates.
- If a client misses updates, it should reload rows from the daemon instead of replaying old local assumptions.

## Row lifecycle

- A row usually starts when session or pull request activity first needs attention.
- User workflow updates can move the row out of the active queue without deleting the daemon-owned entity.
- Later daemon activity can refresh the row when the same entity needs attention again.
- Completion means the current concern is no longer active; it is not a guarantee that the underlying session or pull request record disappears.

## Boundaries

- The inbox is local to one daemon store.
- It is not an external notification aggregator.
- It is not append-only notification history.
- It does not identify rows by external service ids such as GitHub pull request numbers.
