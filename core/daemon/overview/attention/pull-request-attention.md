# Pull Request Attention

- **Core idea**
  - Managed pull requests can produce daemon-local inbox attention.
  - Attention follows the daemon-managed pull request entity rather than external service ids.

- **Creation attention**
  - A pull request submitted through the daemon can create an inbox row.
  - The row helps humans find newly created daemon-managed pull request work.

- **Update attention**
  - Daemon activity can refresh a managed pull request's inbox row when the pull request is updated.
  - The row may reopen even if the user previously read, saved, archived, replied to, or completed it.

- **Completion**
  - Pull request completion means the pull request is no longer an open current concern.
  - Merge or closure are examples of states that can make the row completed.

- **Boundaries**
  - The inbox does not replace GitHub or another host's notification system.
  - Pull request rows are local daemon workflow state.
  - Clients should not infer pull request attention independently from daemon-managed pull request events.
