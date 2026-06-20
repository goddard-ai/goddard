# Logs

Read this ruleset when debugging Goddard daemon behavior, app behavior, daemon/app interactions, IPC flows, or failures where runtime logs may explain what happened.

- Use `pnpm goddard:logs page` as the first log-inspection tool for Goddard runtime debugging; it reads the shared SQLite log store used by both the daemon and app.
- Read each default output line as `id at scope level message pid=<pid> ...properties`.
- Treat the default `pnpm goddard:logs page` output as a combined timeline. Keep both scopes visible when diagnosing daemon/app handoffs, IPC, startup, auth, or UI-triggered behavior.
- Use `--scope daemon` or `--scope app` only after the cross-process timeline is no longer needed.
- `pnpm goddard:logs page` hides `debug` rows by default. Use `--level debug` to include all debug rows, or `--debug <scope-prefix>` to show debug rows with a matching full debug scope or namespace prefix, such as `--debug session` for `session.acp`, `session.queue`, and `session.stream`.
- Use `pnpm goddard:logs scopes` to list observed debug scopes grouped by log scope; add `--since`, `--scope`, or `--prefix` before choosing a focused `--debug <scope-prefix>` filter.
- Start with recent, focused queries such as `pnpm goddard:logs page --since 30m`, then add `--grep`, `--regex`, or `--property path=value` filters to narrow the result.
- For IPC/session investigations, use `pnpm goddard:logs page --json` early to identify stable correlation fields such as `opId`, `sessionId`, `requestName`, `method`, and `pid`, then pivot with `--property`, `--grep`, or `--debug`.
- Use `--property` for top-level properties or dot-notation paths inside inline object properties, such as `--property method=daemon.health` or `--property ipcRequest.opId=<id>`; collapsed `{obj_...}` values must be inspected with `expand`.
- Useful filters include `pnpm goddard:logs page --since 30m --grep ipc.`, `pnpm goddard:logs page --property method=daemon.health`, and `pnpm goddard:logs page --scope daemon --grep error`.
- Use the leading log entry IDs with `--before-id` and `--after-id` when the interesting event is near a page boundary.
- Use `pnpm goddard:logs tail` while reproducing an issue so new app and daemon entries appear in one stream.
- Use `pnpm goddard:logs page --json` when exact fields, structured properties, or machine-readable output matter.
- Use `pnpm goddard:logs expand <collapsed_id>` for collapsed property values wrapped as `{obj_...}`, `{arr_...}`, or `{str_...}` in default output; omit the braces when passing the ID to `expand`, and add `--json` when the collapsed metadata matters.
- Use `pnpm goddard:logs path` only when you need to inspect, copy, or remove the underlying database directly.
- If no logs match, widen `--since`, remove scope/debug filters, confirm the app or daemon process is running, inspect `pnpm goddard:logs path`, and remember development IPC client logging may require `GODDARD_CLIENT_IPC_LOG=1`.
- When inspecting daemon persistence directly, remember development runs use the development data profile. The dev daemon DB is `~/.goddard/development/goddard.db`; `~/.goddard/goddard.db` is the non-development profile and may be stale or unrelated.
- Remember that large property values are collapsed and likely secrets are redacted before persistence, so absence of a raw value in logs is expected.
- Prefer adding or improving structured log fields over relying on long free-form messages when a debugging gap requires a code change.
- Use normal logs for events that belong in the default operational timeline: startup/shutdown, lifecycle transitions, auth/config changes, IPC/app-daemon handoffs, user-visible failures, degraded behavior, and actionable warnings/errors.
- Use `createDebug("<scope>")` for focused subsystem traces that are useful only when investigating that scope: queue movement, stream/message flow, retries, timing/order details, state counters, handled noisy errors, and internal branch decisions.
- If the event changes the durable story of what happened, use a normal log. If it only explains how one subsystem got there and would clutter default logs, use debug.
- Keep debug scopes stable and dotted, and reuse nearby scopes when possible.
- Do not hide important failures only in debug logs.
