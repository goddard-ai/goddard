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
  - [Launch and configuration](./launch-and-configuration.md)
    - Explains how the daemon starts, how data profiles work, how persisted configuration is refreshed, and what launch values are resolved.
  - [Sessions and worktrees](./sessions-and-worktrees.md)
    - Explains daemon-managed sessions, history, diagnostics, live control, launch previews, tokens, isolated worktrees, and reconnect behavior.

- **Local control surfaces**
  - [Agent tools](./agent-tools.md)
    - Explains the command tools exposed to daemon-launched agents for initiatives, blockers, turn endings, pull requests, and workforce delegation.
  - [Adapters and auth](./adapters-and-auth.md)
    - Explains adapter catalog listing, local adapter install state, managed agent installs, and daemon-owned authentication.

- **Automation surfaces**
  - [Actions and loops](./actions-and-loops.md)
    - Explains named one-shot actions and reusable loop runtimes.
  - [Workforce](./workforce.md)
    - Explains daemon-owned multi-agent workforce orchestration, queue state, ownership validation, suspension, and recovery.
  - [Review sessions](./review-sessions.md)
    - Explains review-sync mounting for daemon-managed session worktrees.

- **Attention and collaboration**
  - [Inbox](./inbox.md)
    - Explains daemon-local attention rows, workflow statuses, priority, and update ownership.
  - [Pull requests](./pull-requests.md)
    - Explains daemon-managed pull request submission, replies, attention rows, and feedback handling.

- **Local development data**
  - [Mock data](./mock-data.md)
    - Explains the isolated mock data profile, seeded scenarios, and boundaries for deterministic local-only fixtures.
