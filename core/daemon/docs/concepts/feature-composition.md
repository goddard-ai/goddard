# Feature Composition

The daemon provides shared local runtime substrate, while feature packages contribute product behavior on top of it. This page explains that composition boundary and the default feature surface clients expect.

## Core idea

- The daemon provides shared runtime substrate.
- Product behavior is composed from feature-owned daemon capabilities.
- The default product surface is the feature set ordinary clients expect from the local daemon.

## Daemon substrate

- Process lifetime.
- Local server lifetime.
- Persistence setup.
- Logging and request context.
- Root configuration loading and refresh.
- Shared session launch policy.

## Default feature surface

- Sessions.
- Inbox.
- Pull requests.
- Review sessions.
- Actions.
- Loops.
- Workforce.
- Managed agents.
- Auth.

## Feature ownership

- Feature packages own their product behavior and daemon handlers.
- The daemon keeps cross-feature runtime boundaries coherent.
- Feature-owned persistence belongs behind feature-owned substrate boundaries when a feature owns that data.
- When a feature fails or is unavailable, clients should treat that as a capability-specific outcome rather than as permission to recreate runtime state outside the daemon.

## Backend events

- The daemon owns the backend stream transport lifecycle.
- The daemon starts one backend stream only when feature-owned backend event handlers are registered.
- The daemon stops the stream when the handler registry becomes empty.
- Backend streams carry backend event envelopes with `name`, `payload`, and optional provenance.
- Feature packages own backend event handlers and any daemon events emitted from those handlers.
- Host-level stream health is emitted as `backend.stream.degraded`.
- Pull request feedback automation emits `pull_request.feedback.ignored` and `pull_request.feedback.finished` from the pull-request feature.
- GitHub-originated webhook facts are parsed and authorized by `features/github`; provider-agnostic event contracts belong to the product feature that owns the vocabulary.
- Backend stream filters reuse the shared event-envelope matcher used by daemon event streams.

## Boundary

- Feature composition is not a user configuration system for arbitrary runtime extension.
- Repository-local configuration can declare shared intent, but trusted executable daemon extension remains user-scoped.
