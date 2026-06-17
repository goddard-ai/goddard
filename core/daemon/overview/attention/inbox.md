# Inbox

> The daemon-local inbox is the list of Goddard work items that may need a human decision now. Each row represents one current session or managed pull request, so clients can triage attention without inventing their own rules.

- **Core idea**
  - The daemon-local inbox helps humans decide which daemon-owned work needs attention now.
  - It stores current attention state, not notification history.
  - Each row belongs to exactly one daemon-owned entity.

- **Supported entities**
  - Sessions.
  - Managed pull requests.

- **Daemon ownership**
  - The daemon owns row creation and attention refreshes.
  - Clients may list rows and update user workflow state.
  - Clients must not create rows or infer daemon attention independently.

- **Client operations**
  - List rows using daemon ordering and filtering.
  - Update one row by daemon entity id.
  - Bulk-update rows as one user workflow action.
  - Complete a session's inbox concern through entity-specific validation.
  - Stream daemon-published inbox item updates.

- **Boundaries**
  - The inbox is local to one daemon store.
  - It is not an external notification aggregator.
  - It is not append-only notification history.
  - It does not identify rows by external service ids such as GitHub pull request numbers.
