# Logs

Read this ruleset when debugging Goddard daemon behavior, app behavior, daemon/app interactions, IPC flows, or failures where runtime logs may explain what happened.

- Use `pnpm goddard:logs` as the first log-inspection tool for Goddard runtime debugging; it reads the shared SQLite log store used by both the daemon and app.
- Read each default output line as `id at scope level message pid=<pid> ...properties`.
- Treat the default `pnpm goddard:logs` output as a combined timeline. Keep both scopes visible when diagnosing daemon/app handoffs, IPC, startup, auth, or UI-triggered behavior.
- Use `--scope daemon` or `--scope app` only after the cross-process timeline is no longer needed.
- Start with recent, focused queries such as `pnpm goddard:logs --since 30m`, then add `--grep`, `--regex`, or `--property key=value` filters to narrow the result.
- Use the leading log entry IDs with `--before-id` and `--after-id` when the interesting event is near a page boundary.
- Use `pnpm goddard:logs tail` while reproducing an issue so new app and daemon entries appear in one stream.
- Use `pnpm goddard:logs --json` when exact fields, structured properties, or machine-readable output matter.
- Use `pnpm goddard:logs expand <collapsed_id>` for collapsed property values that begin with `obj_`, `arr_`, or `str_`; add `--json` when the collapsed metadata matters.
- Use `pnpm goddard:logs path` only when you need to inspect, copy, or remove the underlying database directly.
- Remember that large property values are collapsed and likely secrets are redacted before persistence, so absence of a raw value in logs is expected.
- Prefer adding or improving structured log fields over relying on long free-form messages when a debugging gap requires a code change.
