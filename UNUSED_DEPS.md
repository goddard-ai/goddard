# Unused Dependencies Report

The following unused dependencies were identified during `pnpm typecheck` (via `tsdown`'s unused dependency check). This report details why these dependencies are likely considered unused by the build system.

## @goddard-ai/config
- **`@goddard-ai/schema`**
  - **Reason**: While `@goddard-ai/config` defines configuration schemas, it appears to use `zod` directly for its internal primitives and does not currently import any types or values from the workspace `@goddard-ai/schema` package.
  - **Recommended Resolution**: **Remove** from `package.json` if no future usage is planned.

## @goddard-ai/backend
- **`@hattip/adapter-cloudflare-workers`**
  - **Reason**: Used in `core/backend/src/worker.ts`, but this file might not be part of the main entry point (`src/index.ts`) or the build graph analyzed by `tsdown` for the primary bundle.
  - **Recommended Resolution**: **Keep**. It is required for the Cloudflare Workers entry point, even if not reachable from the main `index.ts`.
- **`@octokit/webhooks`**
  - **Reason**: The backend uses the main `octokit` package in `src/github-app.ts`. `@octokit/webhooks` is likely a transitive dependency or was previously used for explicit webhook type definitions that are now handled by `octokit` or `@goddard-ai/schema`.
  - **Recommended Resolution**: **Remove**. Explicit types should be imported from `octokit` or `@goddard-ai/schema`.
- **`alchemy`**
  - **Reason**: Used in `alchemy.run.ts` and the `deploy` script, but `alchemy.run.ts` is likely a standalone script/entry point that is not imported by the main source code analyzed during the types build.
  - **Recommended Resolution**: **Keep**. Required for deployment via the `alchemy` CLI and the `deploy` script.
- **`zod`**
  - **Reason**: Although `zod` is heavily used in the project, the backend package primarily consumes types from `@goddard-ai/schema`. If it doesn't define its own `z` schemas in the tracked build graph, it is flagged as unused.
  - **Recommended Resolution**: **Keep**. The backend likely needs `zod` for any local runtime validation not covered by the shared schema.

## @goddard-ai/sdk
- **`@goddard-ai/ipc`**
  - **Reason**: No imports found in `core/sdk/src`. It might have been intended for IPC communication that is now handled via `@goddard-ai/daemon-client` or directly via `@goddard-ai/schema`.
  - **Recommended Resolution**: **Remove**. IPC logic seems to have migrated to `@goddard-ai/daemon-client`.
- **`@libsql/client`** & **`drizzle-orm`**
  - **Reason**: These are database-related dependencies. While they are used in `@goddard-ai/backend`, the SDK package does not seem to perform direct database operations in its current implementation.
  - **Recommended Resolution**: **Remove**. The SDK should remain a lightweight client; database logic belongs in the backend/storage layers.
- **`zod`**
  - **Reason**: Similar to the backend, the SDK relies on pre-defined schemas from `@goddard-ai/schema` and doesn't appear to use `zod` directly for runtime validation or schema definition in its core logic.
  - **Recommended Resolution**: **Remove** if no direct `z` schema definitions are added to the SDK.
