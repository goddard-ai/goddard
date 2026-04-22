# `@goddard-ai/daemon`

The Goddard Daemon is a local background process that executes autonomous coding tasks. It launches daemon-managed sessions in response to backend events such as pull request feedback or merged proposals.

## Related Docs

- [Daemon Glossary](./glossary.md)
- [Daemon IPC Server Concepts](./src/ipc/server.md)
- [Session Manager Domain Concepts](./src/session/manager.md)
- [Session Turn History Design](./src/session/turn-history-design.md)
- [Worktree Sync Session Design](./src/worktrees/sync-session.md)
- [Workforce Runtime Domain Concepts](./src/workforce/runtime.md)

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

Fresh linked worktrees created by the built-in `default` plugin can also be prepared automatically before the agent starts. Repositories may declare repo-local `worktrees.bootstrap` settings in `.goddard/config.json` to control untracked seeding and daemon-owned package-manager bootstrap. Sync-enabled worktree sessions apply the same preparation flow before the sync mount is established.

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

If no values are provided, the daemon falls back to the standard local backend URL and listens on `http://127.0.0.1:49827/`. The default data profile keeps kindstore data in `~/.goddard/goddard.db`; the `development` data profile isolates it under `~/.goddard/development/goddard.db`.

## Standalone Build

Build standalone Bun executables for the daemon and bundled helper tools with:

```sh
bun run build:standalone
```

The command runs the normal package build first, then emits a platform-specific standalone runtime under `dist/standalone/<target>/` with:

- `bin/goddard-daemon`
- `agent-bin/goddard`
- `agent-bin/workforce`
- `manifest.json`

## Issues & Feature Requests

Please direct bug reports and feature requests to the [Issue Tracker](https://github.com/goddard-ai/daemon/issues).

## License

This project is licensed under the [GNU Affero General Public License v3.0 (AGPLv3)](./LICENSE-AGPLv3).
