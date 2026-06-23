# Daemon Concepts

The daemon has cross-cutting concepts that appear in many feature workflows, such as launch choices, runtime ownership, configuration, agents, and auth. This section defines those concepts independently so feature pages can stay focused.

## Purpose

- This folder gives user-findable names to daemon concepts that cut across feature areas.
- These pages explain ownership boundaries and visible behavior, not internal implementation.

## Runtime boundaries

- [Runtime ownership](./runtime-ownership.md)
  - What the daemon owns and what clients may only observe or request.
- [Daemon server](./daemon-server.md)
  - The local control surface exposed to app, SDK, and operational clients.
- [Browser access](./browser-access.md)
  - How hosted browsers and desktop webviews can reach the local daemon safely.
- [Feature composition](./feature-composition.md)
  - How the default daemon product surface is assembled from feature-owned capabilities.

## Launch and configuration

- [Launch](./launch.md)
  - Backend URL, local port, agent wrapper directory, runtime feature selection, and agent launch environment.
- [Data profiles](./data-profiles.md)
  - Production, development, and mock persistence profiles.
- [Configuration refresh](./configuration-refresh.md)
  - Last-good config behavior, future-work-only refresh, and trust boundaries.

## Agent availability and identity

- [Agents](./agents.md)
  - Launch catalog entries, local launch visibility, and runnable process resolution.
- [Managed agent installs](./managed-agent-installs.md)
  - User-authorized install and update behavior for managed ACP agents.
- [Auth session](./auth-session.md)
  - Device-flow auth, current identity, and logout.
