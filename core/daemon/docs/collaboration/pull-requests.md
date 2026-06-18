# Pull Requests

> Daemon-managed sessions can submit pull requests and send replies through a local daemon contract. This page explains how those operations preserve session, repository, security, and attention context.

- **Core idea**
  - The daemon owns the local pull request contract used by daemon-managed sessions and clients.
  - Pull request operations preserve session, repository, and attention context.

- **Managed pull request records**
  - A submitted pull request becomes a daemon-managed entity that can be fetched by its daemon-managed id.
  - Managed pull requests can produce inbox attention.
  - Pull request inbox rows identify daemon-managed entities, not external service identifiers.
  - The record keeps daemon context such as session and repository association available after the hosting service operation succeeds.
  - Clients should use the daemon-managed id when asking the daemon about local pull request workflow state.

- **Submission**
  - A daemon-managed session can submit a pull request through the daemon.
  - Submission uses the current session token and repository context.
  - Submission can create or refresh pull request attention for the human workflow.
  - Repository security policy can disable pull request submission.
  - If submission is disabled or cannot be completed, the failure belongs to the session workflow and should be visible there rather than guessed from host state alone.

- **Replies**
  - A daemon-managed session can post a pull request reply through the daemon.
  - Replies use the current session token and repository context.
  - A reply can refresh pull request attention.
  - Repository security policy can disable pull request replies.
  - Replies are tied back to daemon-managed session context so later feedback can be handled as part of the same local workflow.

- **Boundaries**
  - The daemon does not replace the app or hosting service as the primary review UI.
  - Pull request records are local daemon-managed entities.
  - Clients should not infer pull request attention independently of daemon-managed pull request events.
  - Related pages: [pull request attention](../attention/pull-request-attention.md), [pull request feedback](./pr-feedback.md), and [session tokens](../sessions/session-tokens.md).
