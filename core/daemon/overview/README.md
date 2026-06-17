# Daemon Overview

- **Purpose**
  - This directory describes what `@goddard-ai/daemon` supports at a conceptual level.
  - It is written for agents and humans who need to understand daemon-owned behavior without reading source code.
  - It intentionally avoids implementation details:
    - Supported outcomes, guardrails, ownership boundaries, and recovery paths belong here.
    - Private schemas, helper functions, exact diagnostics, storage mechanics, and execution order do not.

- **Start here**
  - [Daemon model](./model.md)
    - Defines the daemon process, daemon server, feature composition, runtime ownership, live and historical state, and restart recovery.
  - [Core concepts](./concepts/README.md)
    - Launch, runtime ownership, data profiles, configuration refresh, auth, adapters, and managed agent installs.
  - [Launch](./concepts/launch.md)
    - Backend URL, local port, agent wrapper directory, runtime feature selection, and agent launch environment.
  - [Sessions](./sessions/README.md)
    - Session lifecycle, launch previews, tokens, history, diagnostics, cancellation, steering, worktrees, and composer suggestions.

- **Local control surfaces**
  - [Agent tools](./collaboration/agent-tools.md)
    - Explains the command tools exposed to daemon-launched agents for initiatives, blockers, turn endings, pull requests, and workforce delegation.
  - [Auth sessions](./concepts/auth-session.md)
    - Explains daemon-owned authentication state, device flow, identity reads, and logout.
  - [Adapters](./concepts/adapters.md)
    - Explains launch catalog visibility and local adapter install state.

- **Automation surfaces**
  - [Automation](./automation/README.md)
    - Entry point for action, loop, and workforce behavior.
  - [Actions](./automation/actions.md)
    - Explains named one-shot execution definitions that create daemon-managed sessions.
  - [Loops](./automation/loops.md)
    - Explains reusable daemon-owned loop runtimes.
  - [Workforce](./automation/workforce.md)
    - Explains daemon-owned multi-agent workforce orchestration, queue state, ownership validation, suspension, and recovery.

- **Attention and collaboration**
  - [Attention](./attention/README.md)
    - Entry point for inbox rows, statuses, session attention, and pull request attention.
  - [Inbox](./attention/inbox.md)
    - Explains daemon-local attention rows, workflow statuses, priority, and update ownership.
  - [Pull requests](./collaboration/pull-requests.md)
    - Explains daemon-managed pull request submission, replies, attention rows, and feedback handling.
  - [Pull request feedback](./collaboration/pr-feedback.md)
    - Explains background handling of pull request comments and reviews.
  - [Review sessions](./collaboration/review-sessions.md)
    - Explains review-sync mounting for daemon-managed session worktrees.

- **Local development data**
  - [Development](./development/README.md)
    - Entry point for mock data and standalone build behavior.
  - [Mock data](./development/mock-data.md)
    - Explains the isolated mock data profile, seeded scenarios, and boundaries for deterministic local-only fixtures.
