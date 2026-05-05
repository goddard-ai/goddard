# Daemon Agent Notes

- These rules apply to work in `core/daemon/` unless a deeper `AGENTS.md` narrows them.

## Daemon IPC

- When adding, removing, or changing daemon IPC server methods, update `app/src/daemon-ipc-test-handlers.ts` in the same change so the app bridge test stub stays aligned with the daemon server contract.
