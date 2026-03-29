# `@goddard-ai/daemon`

The Goddard Daemon is a local background process that executes autonomous coding tasks. It spawns 'one-shot pi sessions' in response to events (such as pull request feedback or merged proposals) streamed from the backend.

## Related Docs

- [Daemon Glossary](./glossary.md)
- [Daemon IPC Server Concepts](./src/ipc/server.md)
- [Session Manager Domain Concepts](./src/session/manager.md)
- [Workforce Runtime Domain Concepts](./src/workforce/runtime.md)

## Launch Contract

The daemon now resolves its runtime configuration from one explicit contract:

- Backend URL: `--base-url` or `GODDARD_BASE_URL`
- IPC socket path: `--socket-path` or `GODDARD_DAEMON_SOCKET_PATH`
- Agent wrapper directory: `--agent-bin-dir` or `GODDARD_AGENT_BIN_DIR`

When the daemon launches agent sessions, it prepends the resolved agent-bin directory to `PATH` and injects:

- `GODDARD_DAEMON_URL`
- `GODDARD_SESSION_TOKEN`

Direct daemon session creation keeps the original `cwd` by default, even inside git repositories. Callers can opt into isolated session worktrees with `worktree: { enabled: true }`. The session manager provisions those worktrees during `newSession()` and persists the resulting worktree metadata on the session. `loadSession()` can reuse the persisted worktree for that session id. Worktree cleanup is not automatic on session exit or daemon restart; it is managed explicitly by separate cleanup flows. Higher-level daemon-owned lifecycles such as PR feedback one-shots can enable worktrees automatically when isolation is required.

If no values are provided, the daemon falls back to the standard local backend URL and the default socket path under `~/.goddard`.

## Issues & Feature Requests

Please direct bug reports and feature requests to the [Issue Tracker](https://github.com/goddard-ai/daemon/issues).

## License

This project is licensed under the [GNU Affero General Public License v3.0 (AGPLv3)](./LICENSE-AGPLv3).
