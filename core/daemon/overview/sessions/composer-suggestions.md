# Composer Suggestions

- **Core idea**
  - Composer suggestions help clients present relevant command or launch choices for a session or draft session.

- **Session-scoped suggestions**
  - A live or stored session can provide suggestions based on its current session context.
  - Suggestions may reflect commands or options the active agent has made available.

- **Draft suggestions**
  - Draft suggestions can be requested before a daemon session exists.
  - They depend on a repository working directory rather than a session id.
  - They help launch or composer UI offer relevant choices early.

- **Subpackage discovery**
  - Repository discovery can identify launchable working directories inside a project.
  - This helps users pick the intended project scope before creating a session.

- **Boundaries**
  - Suggestions are convenience data for clients.
  - They do not create sessions or mutate repository state.
  - Clients should treat suggestions as current daemon guidance, not as durable configuration.
