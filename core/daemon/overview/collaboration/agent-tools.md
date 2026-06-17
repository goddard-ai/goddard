# Agent Tools

> Daemon-launched agents receive local command tools that report structured workflow events back to Goddard. This page explains those tools as daemon controls for initiatives, blockers, turn endings, pull requests, and workforce delegation.

- **Core idea**
  - Daemon-launched agents receive command-line tools that report structured status and collaboration events back to the daemon.
  - These tools are hidden workflow controls, not replacements for the agent's user-facing response.
  - The tools use daemon session tokens or workforce context to act only within the current daemon-managed scope.

- **Session status tools**
  - `declare-initiative`
    - Records the current outcome-oriented initiative for the session.
    - Does not create an inbox row by itself.
  - `report-blocker`
    - Reports that the session is blocked and needs human assistance.
    - Marks or refreshes the session's inbox attention as unread.
  - `end-turn`
    - Reports that a meaningful turn boundary was reached when no other entity already claimed attention.
    - Can refresh the session inbox row with scope and headline metadata.

- **Pull request tools**
  - `submit-pr`
    - Submits a pull request through the daemon pull request contract.
    - Uses the current daemon session token and current working directory to preserve session and repository context.
  - `reply-pr`
    - Sends a reply through the daemon pull request contract.
    - Uses the current session token so the reply is tied back to daemon-managed session context.

- **Workforce tools**
  - `request`
    - Delegates work to a target workforce agent within the current workforce root.
  - `update`
    - Appends information to an existing workforce request.
  - `cancel`
    - Cancels an existing workforce request with an optional reason.
  - `truncate`
    - Removes pending work for a workforce scope.
  - `respond`
    - Responds to the current workforce request.
    - Requires the current session token because completion must be attributable to the active workforce session.
  - `suspend`
    - Suspends the current workforce request with a reason.
    - Keeps the request blocked until explicit recovery.

- **Guardrails**
  - Tools that need session authority require the session token injected by the daemon.
  - Workforce tools require workforce root context for workforce sessions.
  - Long or multi-line messages should be passed through files so shell quoting does not corrupt the reported content.
  - Hidden inbox metadata should stay concise and should not duplicate the visible user-facing response.
