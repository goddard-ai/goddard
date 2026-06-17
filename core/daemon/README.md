# `@goddard-ai/daemon`

The Goddard Daemon is a local background process that executes autonomous coding tasks. It launches daemon-managed sessions in response to backend events such as pull request feedback or merged proposals.

## Related Docs

- [Daemon Overview](./overview/README.md)
- [Daemon Glossary](./glossary.md)
- [Daemon IPC Server Concepts](./src/ipc/server.md)
- [Session Manager Domain Concepts](../../features/session/src/daemon/manager.md)
- [Session Turn History Design](../../features/session/src/daemon/turn-history-design.md)
- [Workforce Runtime Domain Concepts](../../features/workforce/src/daemon/runtime.md)

## Feature Composition

The daemon remains the local runtime substrate: process lifecycle, IPC server
mechanics, persistence setup, request context, logging, and startup policy live
here. Product-specific daemon contributions can live in internal feature
packages under `features/<name>/src/daemon.ts`. The default product feature set
is composed by `@goddard-ai/default-features/daemon`.

Feature packages should import `@goddard-ai/daemon-plugin` for static plugin
metadata and `@goddard-ai/ipc` for daemon IPC schema fragments. They should not
import the daemon package that bundles them.

Daemon plugins can declare feature interop through `provides` and `consumes`.
Consumed feature extensions are typed as first-class setup context fields, such
as `context.session`, so methods and event channels can share one feature-owned
extension surface without a generic service locator.

The daemon also owns JSON configuration substrate. Feature packages can
contribute namespaced config schemas through daemon plugins, but config files,
scope precedence, validation errors, persistence, and hot reload remain daemon
or core config responsibilities.

The adapter feature owns adapter catalog request handling and catalog
merge/parsing helpers. The daemon still owns the registry cache service and
root-config manager that supply substrate data to that feature.

`features/inbox` is the current reference daemon feature package. It owns the
inbox IPC contract and request-handler factory while the daemon composition root
still injects the daemon-owned inbox manager until inbox state is fully moved
behind a feature-owned substrate boundary.

## Launch Contract

The daemon now resolves its runtime configuration from one explicit contract:

- Backend URL: `--base-url` or `GODDARD_BASE_URL`
- IPC TCP port: `--port`, `GODDARD_DAEMON_PORT`, or `~/.goddard/config.json` via `daemon.port`
- Agent wrapper directory: `--agent-bin-dir` or `GODDARD_AGENT_BIN_DIR`
- Data profile: `--data-profile` or `GODDARD_DATA_PROFILE`

Global port overrides live only in `~/.goddard/config.json`:

```json
{
  "daemon": {
    "port": 49828
  }
}
```

When the daemon launches agent sessions, it prepends the resolved agent-bin directory to `PATH` and injects:

- `GODDARD_DAEMON_URL`
- `GODDARD_SESSION_TOKEN`

Direct daemon session creation keeps the original `cwd` by default, even inside git repositories. Callers can opt into isolated session worktrees with `worktree: { enabled: true }`. The session manager provisions linked Git worktrees during `newSession()` and persists the resulting worktree metadata on the session. `loadSession()` can reuse the persisted worktree for that session id. Worktree cleanup is not automatic on session exit or daemon restart; it is managed explicitly by separate cleanup flows. Higher-level daemon-owned lifecycles such as PR feedback runs can enable worktrees automatically when isolation is required.

Fresh linked worktrees created by the built-in `default` plugin can also be prepared automatically before the agent starts. Repositories may declare repo-local `worktrees.bootstrap` settings in `.goddard/config.json` to control untracked seeding and daemon-owned package-manager bootstrap. Review-session-enabled worktrees apply the same preparation flow before the review session is mounted.

Custom worktree plugins are loaded from the global Goddard config only. The daemon accepts `worktrees.plugins` entries that point at either a module path relative to `~/.goddard` or a package specifier imported directly by the runtime. Repository-local config cannot declare custom worktree plugins.

```json
{
  "worktrees": {
    "bootstrap": {
      "packageManager": "bun",
      "seedNames": ["node_modules", "dist", ".turbo"]
    },
    "plugins": [
      { "type": "path", "path": "plugins/my-worktree-plugin.mjs" },
      { "type": "package", "package": "@acme/goddard-worktree-plugin" }
    ]
  }
}
```

If no values are provided, the daemon falls back to the standard local backend URL and listens on `http://127.0.0.1:49827/`. The default data profile keeps kindstore data in `~/.goddard/goddard.db`; the `development` data profile isolates it under `~/.goddard/development/goddard.db`; the `mock` data profile isolates seeded local-only fixture data under `~/.goddard/mock/goddard.db`.

## Mock Data

The daemon can seed deterministic local-only fixture data into the isolated
`mock` data profile:

```sh
goddard-daemon seed mock --reset
goddard-daemon run --data-profile mock
```

`--reset` deletes only the mock database artifacts:

- `~/.goddard/mock/goddard.db`
- `~/.goddard/mock/goddard.db-shm`
- `~/.goddard/mock/goddard.db-wal`

Mock seeding does not contact external services, does not seed production or
development data, and does not create reconnectable live sessions. The app and
SDK should consume the mock profile through the normal daemon IPC surface.

The default mock dataset is scenario-comprehensive rather than
schema-comprehensive. It covers the core user-visible states needed for app and
SDK development:

- Eight daemon session scenarios spanning completed, blocked, errored,
  cancelled, archived, and idle-looking historical sessions.
- Session detail fixtures for multi-turn history, interrupted-style history,
  long copy, near-limit context usage, model state, config options, and slash
  command suggestions.
- Ten inbox rows spanning unread, read, saved, archived, replied, and completed
  statuses, including both session-owned and pull-request-owned rows.
- Five local-only daemon-managed pull request records across three repositories,
  with both created and updated PR attention rows.

Future mock fixtures should be added only when they exercise a named
user-visible screen state or workflow. Avoid adding rows solely because a schema
field or enum exists.

## Standalone Build

Build standalone Bun executables for the daemon and bundled helper tools with:

```sh
pnpm run build:standalone
```

The command runs the normal package build first, then emits a platform-specific standalone runtime under `dist/standalone/<target>/` with:

- `bin/goddard-daemon`
- `agent-bin/goddard`
- `agent-bin/workforce`
- `manifest.json`

## Issues & Feature Requests

Please direct bug reports and feature requests to the [Issue Tracker](https://github.com/goddard-ai/daemon/issues).

## License

This project is licensed under the [Functional Source License, Version 1.1, ALv2 Future License](./LICENSE).
