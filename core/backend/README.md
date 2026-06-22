# `@goddard-ai/backend`

The Goddard Backend is an edge-based service built using Cloudflare Workers, Turso (SQLite at the Edge), and Drizzle ORM. It manages the Goddard platform's persistent state (such as tracking Pi sessions and pull request feedback), streams server-sent events to connected local daemons, and processes webhooks from the Goddard GitHub App.

Feature packages declare backend route fragments through `@goddard-ai/backend-plugin`. Backend server and daemon composition roots compose those route fragments into the HTTP router and daemon backend client instead of maintaining separate hand-written backend client contracts.

GitHub is the user-facing identity provider. The backend resolves GitHub login/device-flow
results into backend session tokens, and daemon event streams authenticate with those backend
tokens instead of raw GitHub tokens. The stream is authorized by a backend principal keyed to
GitHub identity.

Backend event streams are user-authorized. GitHub webhook deliveries are verified when
`GITHUB_WEBHOOK_SECRET` is configured, normalized by `features/github` into
`remote_repo.event.received` backend event envelopes, and fanned out only to principals allowed
to see the referenced repository. Requested stream filters are bandwidth hints, not security
boundaries.

The daemon uses one backend stream connection and dispatches received backend events to
feature-owned handlers. PR feedback automation is owned by `features/pull-request`; GitHub
provider integration and provenance are owned by `features/github`.

## Related Docs

- [Backend Glossary](./glossary.md)

## Issues & Feature Requests

Please direct bug reports and feature requests to the [Issue Tracker](https://github.com/goddard-ai/backend/issues).

## License

This project is licensed under the [Functional Source License, Version 1.1, ALv2 Future License](./LICENSE).
