# `@goddard-ai/default-features`

Default Goddard product feature composition.

This private package owns the statically bundled feature set used by the default
backend, daemon, and clients. Backend plugins, daemon plugins, and client-safe
IPC routes use separate entrypoints so each layer can share the product contract
without loading unrelated implementation modules.

Default feature composition is static product wiring, not dynamic runtime plugin
loading.

## Package Surfaces

- `@goddard-ai/default-features/daemon`
  - Composes the default daemon feature plugins.
- `@goddard-ai/default-features/backend`
  - Composes the default backend route tree, backend event definitions, and
    backend event sources from backend-capable feature plugins.
- `@goddard-ai/default-features/daemon-ipc`
  - Composes the default daemon IPC route contract from core and feature route
    fragments.

## License

This project is licensed under the [Functional Source License, Version 1.1, ALv2 Future License](./LICENSE).
