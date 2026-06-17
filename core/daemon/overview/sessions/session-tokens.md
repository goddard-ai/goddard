# Session Tokens

- **Core idea**
  - A session token is narrow authority for a daemon-launched process to act on its own session.
  - It lets command tools report status or collaboration events without granting broad daemon control.

- **Where tokens appear**
  - The daemon injects a session token into daemon-launched agent environments.
  - Agent tools use the token to resolve the current daemon session.
  - Pull request and workforce tools use tokens when an action must be attributable to the active session.

- **What tokens support**
  - Reporting blockers.
  - Declaring initiatives.
  - Reporting turn-ended attention metadata.
  - Submitting or replying to pull requests through the daemon contract.
  - Responding to or suspending active workforce requests.

- **Boundaries**
  - A token is scoped to session authority, not global daemon administration.
  - User-facing clients should not ask agents to expose tokens in visible replies.
  - Token-based tools should still use daemon contracts rather than mutating daemon-owned records directly.
