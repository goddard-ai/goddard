# Data Profiles

> A daemon data profile chooses which local store the daemon reads and writes during one process lifetime. This page explains how normal, development, and mock profiles stay separate.

- **Core idea**
  - A data profile chooses which local daemon store the process uses.
  - Profiles isolate normal use, development use, and deterministic mock data.

- **Production**
  - The default profile for normal local daemon use.
  - Stores ordinary daemon-local data under the user's Goddard home.

- **Development**
  - Isolates local development data from the default profile.
  - Useful when working on Goddard without mutating normal local daemon state.

- **Mock**
  - Isolates deterministic fixture data for app and SDK development.
  - Can be seeded through the daemon's mock seed flow.
  - See [Mock data](../development/mock-data.md).

- **What profiles affect**
  - Persisted daemon records.
  - Session, inbox, pull request, workforce, and other local daemon state stored in that profile.
  - What clients see when they connect to a daemon running with that profile.

- **Boundaries**
  - Profiles do not sync local state across machines.
  - Switching profiles changes which local store is active; it does not migrate records between profiles.
  - Mock data is local-only and should be consumed through the same daemon surface as ordinary data.
