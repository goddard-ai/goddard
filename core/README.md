# `@goddard-ai/core`

The Goddard Core repository houses shared packages and utilities that are used across various Goddard subprojects but do not have their own standalone git repositories.

Core packages included here:
- `schema`: Defines strict Zod validation schemas and backend API routes using the `rouzer` library.
- `paths`: Resolves shared `.goddard` filesystem paths without owning persistence.
- `config`: Shared configuration definitions.
- `review-sync`: Git-only review branch synchronization for agent-owned worktrees.

## Issues & Feature Requests

Please direct bug reports and feature requests to the [Issue Tracker](https://github.com/goddard-ai/core/issues).

## License

This project is licensed under the [MIT License](./LICENSE-MIT).
