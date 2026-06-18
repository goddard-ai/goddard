# Managed Agent Installs

> Managed agent installs let user-owned daemon policy decide when an ACP agent may be installed or updated for launch. This page explains that authority boundary and how managed install status appears to launch flows.

- **Core idea**
  - Managed agent installs let user-owned daemon policy control when an ACP agent may be installed or updated.
  - They keep install and update authority explicit while letting launch flows show managed agent status.

- **What the daemon decides**
  - Whether a managed agent may be installed before use.
  - Whether proactive update behavior applies after recent use.
  - Whether managed install state should be surfaced in launch listings.

- **What the ACP client layer owns**
  - Managed install metadata.
  - Update checks.
  - Runnable process resolution for managed agents.

- **Launch visibility**
  - A managed agent can appear in launch listings even without a Goddard adapter-install marker.
  - The entry should expose managed install status so users understand whether the agent is installed, installable, or pending update.
  - Launch UI should treat install status as part of the user's launch decision, not as hidden background state.
  - If installation or update is required before launch, failure should be reported as a launch availability problem.

- **Boundaries**
  - Managed install policy is user-owned.
  - Managed agents do not use the same Goddard-owned binary cache path as unmanaged archive-backed or raw-binary targets.
  - Managed install status does not mean a session has already been created.
  - Related pages: [adapters](./adapters.md), [launch preview and leases](../sessions/launch-preview-and-leases.md), and [session lifecycle](../sessions/lifecycle.md).
