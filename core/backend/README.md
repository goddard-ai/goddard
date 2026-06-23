# `@goddard-ai/backend`

The Goddard Backend is an edge-based service built using Cloudflare Workers, Turso (SQLite at the Edge), and Drizzle ORM. It manages the Goddard platform's persistent state (such as tracking Pi sessions and pull request feedback), streams server-sent events to connected local daemons, and processes webhooks from the Goddard GitHub App.

Feature packages declare backend plugin fragments through `@goddard-ai/backend-plugin`.
`@goddard-ai/default-features/backend` owns the default product backend composition and provides
the route tree, event definitions, and event sources consumed by this package.

GitHub is the user-facing identity provider. The backend resolves GitHub login/device-flow
results into backend session tokens, and daemon event streams authenticate with those backend
tokens instead of raw GitHub tokens. The stream is authorized by a backend principal keyed to
GitHub identity.

Backend event streams carry backend event envelopes. Product features own provider-agnostic event
definitions, while remote-repo owns provider-agnostic event production and authorization for remote
repository events. For example, `features/remote-repo` defines specific pull-request comment,
review, and created backend event contracts, while `features/github` owns `/webhooks/github`, raw
GitHub payload parsing, signature verification, bot filtering, and provider provenance.

Core backend owns the HTTP runtime, request implementations, session/principal resolution,
publication validation, authorization calls, and stream fanout. Publishing validates that the
source may produce the event, validates the provider-agnostic payload schema, authorizes the source
event for the backend principal, then applies the shared event-envelope filter matcher. Requested
stream filters are bandwidth hints, not security boundaries.

The daemon uses one backend stream connection and dispatches received envelopes to feature-owned
handlers. PR feedback automation is owned by `features/pull-request`; GitHub provider integration
and provenance are owned by `features/github`.

## Related Docs

- [Backend Glossary](./glossary.md)

## Issues & Feature Requests

Please direct bug reports and feature requests to the [Issue Tracker](https://github.com/goddard-ai/backend/issues).

## License

This project is licensed under the [Functional Source License, Version 1.1, ALv2 Future License](./LICENSE).
