# Pull Request Attention

> Managed pull requests can appear in the daemon-local inbox when daemon activity makes them relevant to the user. This page explains how pull request attention follows the daemon's local entity rather than raw hosting-service identifiers.

- **Core idea**
  - Managed pull requests can produce daemon-local inbox attention.
  - Attention follows the daemon-managed pull request entity rather than external service ids.

- **Creation attention**
  - A pull request submitted through the daemon can create an inbox row.
  - The row helps humans find newly created daemon-managed pull request work.
  - The row belongs to the local managed pull request record, so clients can show daemon context even when the hosting service uses a different identifier.

- **Update attention**
  - Daemon activity can refresh a managed pull request's inbox row when the pull request is updated.
  - The row may reopen even if the user previously read, saved, archived, replied to, or completed it.
  - Supported feedback handling and daemon-submitted replies can both make the pull request relevant again.
  - Refreshing attention should preserve the distinction between pull request attention and session attention for the same underlying work.

- **Completion**
  - Pull request completion means the pull request is no longer an open current concern.
  - Merge or closure are examples of states that can make the row completed.
  - Completing pull request attention does not erase the daemon-managed pull request record or the session history that created it.

- **Boundaries**
  - The inbox does not replace GitHub or another host's notification system.
  - Pull request rows are local daemon workflow state.
  - Clients should not infer pull request attention independently from daemon-managed pull request events.
