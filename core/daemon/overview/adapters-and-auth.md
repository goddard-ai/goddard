# Adapters and Auth

- **Adapter catalog**
  - The adapter catalog answers which ACP agents are available for launch in a global or project context.
  - Catalog listing combines registry-provided adapters with user or repository configuration entries.
  - Configuration-declared entries are launch-visible because the user or repository explicitly made them part of effective configuration.

- **Local adapter install state**
  - Local adapter install state records which registry adapters the user enabled for normal launch listings.
  - Ordinary registry adapters can remain hidden from launch listings until installed.
  - Clients may ask to include uninstalled adapters when presenting broader catalog management UI.
  - Installing or uninstalling an adapter changes Goddard-owned launch catalog state; it does not by itself start a session.

- **Managed agent installs**
  - Managed agents are declared through user-owned daemon policy.
  - A managed agent can be installed before use or updated proactively by daemon policy.
  - The daemon decides when managed install behavior is allowed, while the ACP client layer owns managed install metadata and runnable process resolution.
  - A managed agent can appear in launch listings even without a local adapter-install marker.

- **Unmanaged binary installs**
  - Some agent distributions resolve to archive-backed or raw binary targets.
  - The daemon can prepare those targets in a Goddard-owned binary cache before launch.
  - Relative commands from installed payloads resolve from the installed payload root rather than the caller's current `PATH`.

- **Auth session**
  - The daemon owns the local auth session visible to daemon clients.
  - Device-flow start creates a pending authentication flow.
  - Device-flow completion promotes a successful flow into the current auth session.
  - `whoami` reads the current daemon-owned auth session.
  - Logout clears the current daemon-owned auth session.

- **Guardrails**
  - Repository-local configuration can contribute launch-visible catalog intent, but trusted executable install and update authority remains user-scoped.
  - Listing adapters does not imply an agent can be launched until the daemon can resolve a runnable process for it.
  - Auth state is local daemon state; clients should read it through the daemon instead of assuming their own separate identity.
