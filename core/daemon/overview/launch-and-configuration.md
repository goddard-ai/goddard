# Launch and Configuration

- **What launch answers**
  - Which backend the daemon talks to.
  - Which local port exposes daemon control.
  - Which agent wrapper directory daemon-launched sessions receive.
  - Which local data profile stores daemon-managed state.
  - Which runtime surfaces are enabled for this daemon process.

- **Launch values**
  - The daemon accepts launch-time values from command-line flags and environment variables.
  - The daemon can also read the global port from the user Goddard config.
  - If no explicit values are provided, the daemon uses the standard local defaults for backend URL, IPC port, agent wrapper directory, and production data profile.

- **Runtime feature selection**
  - A normal daemon run enables both local IPC control and background stream handling.
  - Operators may start only selected runtime features when they need a narrower daemon process.
  - Feature selection affects daemon process behavior, not the conceptual ownership of data already stored by the daemon.

- **Data profiles**
  - `production`
    - The default profile for normal local daemon use.
    - Stores the ordinary daemon-local database under the user's Goddard home.
  - `development`
    - Isolates local development data from the default profile.
    - Useful when working on the product without mutating normal local daemon data.
  - `mock`
    - Isolates deterministic local-only fixture data for app and SDK development.
    - See [Mock data](./mock-data.md).

- **Configuration refresh**
  - The daemon owns persisted root-config loading and watching.
  - Changes to valid user or repository configuration affect future work after the daemon accepts the updated snapshot.
  - Work that already began continues under the configuration resolved for that work.
  - Invalid edits do not replace the last valid behavior.
  - The daemon may report configuration watcher degradation, but the user-facing contract remains last-good behavior until a valid snapshot is available.

- **Configuration boundaries**
  - Persisted configuration is machine-readable JSON.
  - User configuration can define personal defaults and trusted executable extension references.
  - Repository configuration can define shared repository intent and non-executable session preparation policy.
  - Runtime input can override configuration for one invocation without persisting that choice.
  - Prompt content is not a configuration transport.

- **Agent launch environment**
  - Daemon-launched sessions receive daemon connection information and a session token.
  - The resolved agent wrapper directory is placed where launched agents can find the Goddard command tools.
  - Session-specific environment policy applies before the agent starts so the agent receives only the intended runtime context.

- **Standalone build outcome**
  - The daemon package can produce standalone local executables for the daemon and bundled agent helper tools.
  - The standalone output is a distribution artifact, not a separate product behavior model.
