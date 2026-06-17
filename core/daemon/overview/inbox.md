# Inbox

- **Core idea**
  - The daemon-local inbox helps humans decide which daemon-owned work needs attention now.
  - It stores current attention state, not notification history.
  - Each row belongs to exactly one daemon-owned entity.
  - Supported entities are sessions and managed pull requests.

- **Daemon-owned attention**
  - The daemon owns row creation and attention refreshes.
  - Clients may list rows and update user workflow state.
  - Clients must not create rows or infer daemon attention independently.
  - Later daemon attention may reopen a row that a user previously read, replied to, saved, archived, or completed.

- **Attention sources**
  - A session can need attention when:
    - it reports a blocker
    - a turn ends without another entity taking responsibility for attention
  - A managed pull request can need attention when:
    - it is created by daemon activity
    - it is updated by daemon activity

- **Workflow statuses**
  - `unread`
    - The daemon says the entity needs human attention.
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

- **Priority**
  - `normal` rows participate in the standard attention queue.
  - `low` rows remain relevant but sort behind normal-priority work.
  - Priority is a user workflow signal and should not be casually overwritten by daemon refreshes.

- **Metadata**
  - Session attention can include a short scope and headline.
  - Scope names the work area.
  - Headline says what changed or why attention matters now.
  - This metadata is hidden workflow state, not visible chat content.

- **Client operations**
  - Clients can list rows with daemon ordering and filtering.
  - Clients can update one row by daemon entity id.
  - Clients can bulk-update rows as one user workflow action.
  - Clients can complete a session's inbox concern through entity-specific validation.
  - Clients can stream daemon-published inbox item updates.

- **Boundaries**
  - The inbox is local to one daemon store.
  - It is not an external notification aggregator.
  - It is not an append-only audit trail.
  - It does not use external service identifiers, such as GitHub pull request numbers, as row identity.
