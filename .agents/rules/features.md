# Features

Read this ruleset when adding or changing cross-layer feature packages, feature schemas, daemon plugins, SDK/app feature wiring, or scaffolded features.

- New cross-layer product capabilities should normally start as internal feature packages under `features/<name>`.
- Use `pnpm run scaffold:feature` for the standard feature package shape.
- Internal full-stack feature packages use the package name `@goddard-ai/<name>` without a `feature-` prefix.
- Scaffolded packages are inert until a public composition root imports one of their entrypoints.
- Wire only selected feature entrypoints into public composition roots.
- Do not register a feature in `core/sdk`, `core/daemon`, or `app` unless the task includes making that feature part of the supported product surface.
- Feature packages must import thin plugin support packages such as `@goddard-ai/sdk-plugin`, `@goddard-ai/daemon-plugin`, and `@goddard-ai/app-plugin`, not public packages that bundle them.
- Feature packages self-declare schemas from their own `schema` entrypoint.
- Do not add feature-owned schemas to `@goddard-ai/schema`; reserve it for core daemon/backend/shared substrate schemas.
- Feature-owned client-visible error-code identifiers belong in the owning feature schema entrypoint, not in `@goddard-ai/schema`.
- App feature entrypoints must stay SDK-agnostic at the package level. Express SDK needs as type-level app plugin requirements or app composition metadata, and let the static app composition root provide the actual SDK instance.
- Daemon feature packages that own persistence declare their kindstore schema through the daemon plugin `db` option and use inferred setup `context.db`.
- Do not import the core daemon persistence singleton for feature-owned tables.
- Daemon feature dependencies are explicit. A daemon plugin may expose a named `provides` map and list other daemon plugins in `consumes`; consumed feature extensions appear as direct `context.<feature>` fields in `setup(context)`.
- Do not introduce package-level cycles between feature packages.
- Shared daemon IPC contracts belong in `src/daemon-ipc.ts` and use `defineIpcSchema()` from `@goddard-ai/ipc`; public composition roots combine fragments with `composeIpcSchemas()`.
- `features/inbox` is the current reference package for a low-risk daemon + SDK + app feature. Inspect it before adding a new feature package with similar layers.
