# Pull Requests

- **Core idea**
  - The daemon owns the local pull request contract used by daemon-managed sessions and clients.
  - Pull request operations preserve session, repository, and attention context instead of letting each caller manage that context independently.

- **Managed pull request records**
  - A submitted pull request becomes a daemon-managed entity that can be fetched by its daemon-managed id.
  - Managed pull requests can produce inbox attention.
  - Pull request inbox rows identify daemon-managed entities, not external service identifiers.

- **Submission**
  - A daemon-managed session can submit a pull request through the daemon.
  - Submission uses the current session token and repository context.
  - Submission can create or refresh pull request attention for the human workflow.
  - Repository security policy can disable pull request submission.

- **Replies**
  - A daemon-managed session can post a pull request reply through the daemon.
  - Replies use the current session token and repository context.
  - A reply can refresh pull request attention.
  - Repository security policy can disable pull request replies.

- **Feedback handling**
  - A daemon runtime can listen for authenticated pull request feedback for the current user.
  - Feedback handling is queued by pull request.
  - The daemon avoids overlapping feedback sessions for the same pull request.
  - Each feedback session uses the repository and pull request context carried by the event.
  - Launch failures are reported with pull request context and do not crash the daemon runtime.

- **Boundaries**
  - Pull request feedback handling is a daemon runtime domain, separate from workforce orchestration.
  - The daemon does not replace the app or hosting service as the primary review UI.
  - The feedback runtime reacts to supported pull request comment and review feedback events; it is not broad long-running planning.
  - Clients should not infer pull request attention independently of daemon-managed pull request events.
