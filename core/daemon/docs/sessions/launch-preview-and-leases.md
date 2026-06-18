# Launch Preview and Leases

> Launch preview lets a client inspect agent and repository capabilities before creating a durable daemon session. This page explains how previews and leases support launch dialogs without committing abandoned choices.

- **Core idea**
  - Launch preview lets a client inspect launch-time capabilities before committing to a durable daemon session.
  - A launch lease can keep prepared live launch state available while the user finishes a launch decision.

- **Launch preview**
  - Answers which adapter and repository capabilities are available for a proposed launch.
  - Supports launch dialogs that need model, config, or repository capability information before session creation.
  - Does not by itself create the final durable daemon session.
  - If preview cannot resolve a required launch choice, the client should present that as a launch problem before session creation.
  - Preview output is current daemon guidance, not a saved user preference.

- **Launch lease**
  - Holds daemon-owned live ACP session preparation for a launch dialog.
  - Lets the eventual session reuse capability discovery and prepared launch state.
  - Can be abandoned and released if the user changes the dialog or cancels launch.
  - A lease is still pre-session state: it has launch authority only for the pending choice it represents.
  - If the user changes the launch decision, the client should release the old lease instead of treating it as reusable for unrelated work.

- **Release**
  - Releasing an abandoned lease tells the daemon it no longer needs to preserve that prepared state.
  - Release does not delete an already-created durable daemon session.
  - If a client disappears, later launch state should be recovered by asking the daemon what remains valid instead of assuming the lease survived.

- **Boundaries**
  - Preview and lease behavior belongs to launch-time preparation.
  - Durable session behavior begins only when a session is created.
  - A lease grants no broad daemon authority outside the launch it was prepared for.
  - Related pages: [session lifecycle](./lifecycle.md), [adapters](../concepts/adapters.md), and [managed agent installs](../concepts/managed-agent-installs.md).
