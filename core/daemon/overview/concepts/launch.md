# Launch

> Starting the daemon chooses the backend, local control port, agent wrapper location, data profile, and enabled runtime surfaces for that process. This page explains those launch choices as user-visible behavior.

- **What launch answers**
  - Which backend the daemon talks to.
  - Which local port exposes daemon control.
  - Which agent wrapper directory daemon-launched sessions receive.
  - Which data profile stores daemon-managed state.
  - Which runtime surfaces are enabled for this daemon process.

- **Launch inputs**
  - Operators can provide launch values through command-line flags and environment variables.
  - The daemon can also read selected global defaults, such as the local daemon port, from user configuration.
  - When no explicit values are provided, the daemon uses standard local defaults.

- **Runtime feature selection**
  - A normal daemon run enables local control and background stream handling.
  - Operators may start only selected runtime features when they need a narrower process.
  - Feature selection affects the current daemon process; it does not redefine ownership of already persisted daemon data.

- **Agent launch environment**
  - Daemon-launched sessions receive daemon connection information and a session token.
  - The resolved agent wrapper directory is placed where launched agents can find Goddard helper tools.
  - Session environment policy applies before the agent starts so the agent receives only the intended runtime context.

- **Boundaries**
  - Launch configuration chooses how this daemon process starts.
  - It does not create sessions by itself.
  - Data profile selection chooses the active local store; it does not migrate records between stores.
