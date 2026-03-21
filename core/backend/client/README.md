# `@goddard-ai/backend-client`

Low-level backend HTTP helpers shared by the daemon, SDK composition layers, and host-specific integrations.

Use this package when you need to:

- Start or complete backend device-flow authentication.
- Persist backend auth tokens through a custom `TokenStorage`.
- Create pull requests or reply through the backend API.
- Subscribe to the backend repo event stream.

Use `@goddard-ai/sdk` instead when you want the stable higher-level facade rather than direct route-level backend operations.

## License

This project is licensed under the [MIT License](./LICENSE-MIT).
