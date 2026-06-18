# Adapters

> Adapters describe ACP agents that can be shown or selected when starting daemon-managed sessions. This page explains how the daemon presents catalog entries, local enablement, and launch visibility.

- **Core idea**
  - Adapters describe launchable ACP agents for daemon-managed sessions.
  - The adapter catalog answers which agents can be shown or selected in a global or project launch context.

- **Catalog sources**
  - Registry-provided adapter entries.
  - User or repository configuration entries.
  - Configuration-declared entries are launch-visible because the user or repository made them part of effective configuration.

- **Local install state**
  - Goddard-owned adapter install state records which registry adapters the user enabled for normal launch listings.
  - Ordinary registry adapters can remain hidden from launch listings until installed.
  - Clients may request broader listings when presenting catalog management.
  - Launch listings should make the difference visible between installed, merely available, and configuration-declared entries.

- **Install and uninstall**
  - Installing an adapter changes local launch catalog visibility.
  - Uninstalling an adapter removes that local launch catalog marker.
  - Neither action starts an agent session by itself.
  - If an installed entry later cannot resolve a runnable process, launch should fail as a launch problem rather than treating the catalog marker as proof of runtime readiness.

- **Boundaries**
  - Listing an adapter does not guarantee launch until the daemon can resolve a runnable process.
  - Adapter install state is separate from managed agent install state.
  - Repository-local catalog entries should not silently grant executable extension authority beyond the configured adapter behavior.
  - Related pages: [managed agent installs](./managed-agent-installs.md), [launch preview and leases](../sessions/launch-preview-and-leases.md), and [launch](./launch.md).
