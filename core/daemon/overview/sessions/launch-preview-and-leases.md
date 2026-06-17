# Launch Preview and Leases

- **Core idea**
  - Launch preview lets a client inspect launch-time capabilities before committing to a durable daemon session.
  - A launch lease can keep prepared live launch state available while the user finishes a launch decision.

- **Launch preview**
  - Answers which adapter and repository capabilities are available for a proposed launch.
  - Supports launch dialogs that need model, config, or repository capability information before session creation.
  - Does not by itself create the final durable daemon session.

- **Launch lease**
  - Holds daemon-owned live ACP session preparation for a launch dialog.
  - Lets the eventual session reuse capability discovery and prepared launch state.
  - Can be abandoned and released if the user changes the dialog or cancels launch.

- **Release**
  - Releasing an abandoned lease tells the daemon it no longer needs to preserve that prepared state.
  - Release does not delete an already-created durable daemon session.

- **Boundaries**
  - Preview and lease behavior belongs to launch-time preparation.
  - Durable session behavior begins only when a session is created.
  - A lease grants no broad daemon authority outside the launch it was prepared for.
