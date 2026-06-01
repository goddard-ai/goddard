# `@goddard-ai/default-features`

Default Goddard product feature composition.

This private package owns the statically bundled feature set used by the default
daemon and its clients. Server-side daemon plugins and client-side IPC routes
use separate entrypoints so clients can share the product route contract without
loading daemon implementation modules.

## Package Surfaces

- `@goddard-ai/default-features/daemon`
  - Composes the default daemon feature plugins.
- `@goddard-ai/default-features/daemon-ipc`
  - Composes the default daemon IPC route contract from core and feature route
    fragments.

## License

This project is licensed under the [Functional Source License, Version 1.1, ALv2 Future License](./LICENSE).
