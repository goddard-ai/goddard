# `@goddard-ai/backend`

The Goddard Backend is an edge-based service built using Cloudflare Workers, Turso (SQLite at the Edge), and Drizzle ORM. It manages the Goddard platform's persistent state (such as tracking Pi sessions and pull request feedback), streams server-sent events to connected local daemons, coordinates cloud-owned agent sessions, and processes webhooks from the Goddard GitHub App.

Feature packages declare backend route fragments through `@goddard-ai/backend-plugin`. Backend server and daemon composition roots compose those route fragments into the HTTP router and daemon backend client instead of maintaining separate hand-written backend client contracts.

Cloud-owned sessions are isolated in the `CloudSession` Durable Object. Each object owns one session's ordered event log, idempotent command ledger, and WebSocket channel to the ACP harness running in the sandbox. Local daemons consume cloud session state by cursor; local-only sessions are not synchronized into the backend.

`bun run test` runs both Bun unit tests and Workers-runtime tests. Use `bun run test:workers` to exercise the Cloudflare Durable Object binding locally through `@cloudflare/vitest-pool-workers` without deploying.

## Related Docs

- [Backend Glossary](./glossary.md)

## Issues & Feature Requests

Please direct bug reports and feature requests to the [Issue Tracker](https://github.com/goddard-ai/backend/issues).

## License

This project is licensed under the [Functional Source License, Version 1.1, ALv2 Future License](./LICENSE).
