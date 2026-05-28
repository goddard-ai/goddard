# `@goddard-ai/ipc`

Typed IPC contract helpers shared across host environments.

## Package Surfaces

- `@goddard-ai/ipc`
  - Schema declarations, schema-fragment definition and composition helpers, typed client creation, transport types, and shared request-handler types.
- `@goddard-ai/ipc/node`
  - Node TCP transport and Node IPC server implementation.

## Feature IPC Schemas

Feature packages that expose daemon-backed SDK behavior should keep transport
contracts in `src/daemon-ipc.ts` and use `defineIpcSchema()` from this package.
Public composition roots combine feature fragments with `composeIpcSchemas()`
so route names, stream names, request payload schemas, and response types do not
drift between the daemon and SDK.

`features/inbox/src/daemon-ipc.ts` is the reference feature-owned daemon IPC
contract.

## License

This project is licensed under the [Functional Source License, Version 1.1, ALv2 Future License](./LICENSE).
