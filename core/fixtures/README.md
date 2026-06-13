# `@goddard-ai/fixtures`

Synthetic, platform-agnostic data fixtures for development and tests.

This package owns reusable fixture builders, response-shaped helpers, stable IDs, stable timestamps, and curated scenario data. It exists to keep duplicated mock data out of app components, launchable-state wiring, tests, and seed adapters.

This package does not own app navigation, route state, query injection, `state-launcher` registration, database writes, kindstore setup, SDK client mocking, test framework setup, process behavior, filesystem behavior, network behavior, or desktop behavior.

Production app, daemon, SDK, and backend runtime entrypoints must not import this package. Seed code may compose fixture data in the future, but seed execution belongs to the package or app that performs the seeding.
