# `@goddard-ai/backend`

The Goddard Backend is an edge-based service built using Cloudflare Workers, Turso (SQLite at the Edge), and Drizzle ORM. It manages the Goddard platform's persistent state (such as tracking Pi sessions and pull request feedback), streams server-sent events to connected local daemons, and processes webhooks from the Goddard GitHub App.

Feature packages declare backend route fragments through `@goddard-ai/backend-plugin`. Backend server and daemon composition roots compose those route fragments into the HTTP router and daemon backend client instead of maintaining separate hand-written backend client contracts.

## Related Docs

- [Backend Glossary](./glossary.md)

## Issues & Feature Requests

Please direct bug reports and feature requests to the [Issue Tracker](https://github.com/goddard-ai/backend/issues).

## License

This project is licensed under the [GNU Affero General Public License v3.0 (AGPLv3)](./LICENSE-AGPLv3).
