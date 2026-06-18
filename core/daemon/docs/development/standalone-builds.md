# Standalone Builds

> Standalone builds package the daemon and helper tools into local executables for distribution. This page explains the output as a packaging concern, not a different daemon behavior model.

- **Core idea**
  - The daemon package can produce standalone local executables for the daemon and bundled helper tools.
  - Standalone builds are distribution artifacts, not a separate behavior model.

- **Output**
  - A daemon executable.
  - Bundled agent helper tools.
  - A manifest describing the standalone output.

- **What it changes**
  - How the daemon and helper tools are packaged for local execution.
  - It does not change daemon-owned runtime contracts such as sessions, inbox, pull requests, or workforce.
  - Packaging failures are distribution problems; they do not redefine daemon runtime behavior.

- **Boundaries**
  - Standalone output does not replace configuration, data profiles, or daemon feature composition.
  - Clients should still treat the running daemon as the local source of truth for daemon-managed state.
  - Related pages: [launch](../concepts/launch.md), [feature composition](../concepts/feature-composition.md), and [data profiles](../concepts/data-profiles.md).
