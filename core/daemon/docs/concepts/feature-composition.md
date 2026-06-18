# Feature Composition

> The daemon provides shared local runtime substrate, while feature packages contribute product behavior on top of it. This page explains that composition boundary and the default feature surface clients expect.

- **Core idea**
  - The daemon provides shared runtime substrate.
  - Product behavior is composed from feature-owned daemon capabilities.
  - The default product surface is the feature set ordinary clients expect from the local daemon.

- **Daemon substrate**
  - Process lifetime.
  - Local server lifetime.
  - Persistence setup.
  - Logging and request context.
  - Root configuration loading and refresh.
  - Shared session launch policy.

- **Default feature surface**
  - Sessions.
  - Inbox.
  - Pull requests.
  - Review sessions.
  - Actions.
  - Loops.
  - Workforce.
  - Adapters.
  - Auth.

- **Feature ownership**
  - Feature packages own their product behavior and daemon handlers.
  - The daemon keeps cross-feature runtime boundaries coherent.
  - Feature-owned persistence belongs behind feature-owned substrate boundaries when a feature owns that data.
  - When a feature fails or is unavailable, clients should treat that as a capability-specific outcome rather than as permission to recreate runtime state outside the daemon.

- **Boundary**
  - Feature composition is not a user configuration system for arbitrary runtime extension.
  - Repository-local configuration can declare shared intent, but trusted executable daemon extension remains user-scoped.
