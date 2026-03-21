# `@goddard-ai/daemon`

The Goddard Daemon is a local background process that executes autonomous coding tasks. It spawns 'one-shot pi sessions' in response to events (such as pull request feedback or merged proposals) streamed from the backend.

## Related Docs

- [Daemon Glossary](./glossary.md)
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

When a daemon session `cwd` resolves inside a git repository, the daemon provisions an isolated session worktree under `.goddard-agents/` and starts the agent in the mapped directory inside that worktree. Callers can disable that isolation per session with `worktree: { enabled: false }`, which keeps changes in the original checkout. Non-repository directories also keep using the original `cwd`.

If no values are provided, the daemon falls back to the standard local backend URL and the default socket path under `~/.goddard`.

## Issues & Feature Requests

Please direct bug reports and feature requests to the [Issue Tracker](https://github.com/goddard-ai/daemon/issues).

## License

This project is licensed under the [GNU Affero General Public License v3.0 (AGPLv3)](./LICENSE-AGPLv3).
