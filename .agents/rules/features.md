# Features

Read this ruleset when adding or changing feature packages, feature-owned schemas, runtime predicates/type guards, event handlers, persistence, daemon plugins, SDK feature wiring, or scaffolded features.

- New cross-layer product capabilities should normally start as internal feature packages under `features/<name>`.
- Use `pnpm run scaffold:feature` for the standard feature package shape.
- Internal full-stack feature packages use the package name `@goddard-ai/<name>` without a `feature-` prefix.
- Scaffolded packages are inert until a public composition root imports one of their entrypoints.
- Prefer the scaffolded feature shape and layer-specific entrypoints such as `schema`, `backend`, `daemon`, and `sdk` over copying another feature package.
- Composition roots should import only the selected public entrypoints needed for the supported product surface. Do not import feature internals from a composition root.
- Do not register a feature in `core/sdk` or `core/daemon` unless the task includes making that feature part of the supported product surface.
- Feature packages must import thin plugin support packages such as `@goddard-ai/sdk-plugin` and `@goddard-ai/daemon-plugin`, not public packages that bundle them.
- Feature packages self-declare schemas from their own `schema` entrypoint.
- Feature-owned schemas are the runtime source of truth for feature-owned domain values. Runtime predicates and type guards must validate with the owning Zod schema or a schema derived from it; do not duplicate discriminator or property checks by hand.
- When narrowing an upstream schema-owned union, derive the narrowed schema from upstream schema members when possible, then infer the TypeScript type from that narrowed schema. Keep `is*` helpers as thin wrappers around schema validation.
- Do not add feature-owned schemas to `@goddard-ai/schema`; reserve it for core daemon/backend/shared substrate schemas.
- Feature-owned client-visible error-code identifiers belong in the owning feature schema entrypoint, not in `@goddard-ai/schema`.
- Cross-layer contracts should be exported from the owning feature's public entrypoints. Other packages should import feature contracts through those entrypoints, not by reaching into internal implementation files.
- Feature modules must not declare local structural types that mirror upstream route clients, plugin setup context, event buses, session services, kindstore collections, or schema records. Import or derive those types from the owning module with `Pick<>`, `Parameters<>`, `ReturnType<>`, `z.infer<>`, or plugin inference helpers.
- Daemon feature packages that own persistence declare their kindstore schema through the daemon plugin `db` option and use inferred setup `context.db`.
- If a feature-owned persistence schema needs to be referenced outside the plugin definition, name the schema object and export a `DbContext<typeof schema>` alias from the owning feature entrypoint.
- Do not import the core daemon persistence singleton for feature-owned tables.
- Daemon feature dependencies are explicit. A daemon plugin may expose a named `provides` map and list other daemon plugins in `consumes`; consumed feature extensions appear as direct `context.<feature>` fields in `setup(context)`.
- A feature package may depend on another feature only through an explicit public entrypoint and only when that dependency is part of the feature contract. Avoid incidental imports across feature internals.
- Do not introduce package-level cycles between feature packages.
- Feature-owned events should use the owning product domain in their names. Do not place workflow events in substrate namespaces such as `daemon.*` or `backend.*` unless the event is actually owned by that substrate.
- Backend event transport belongs to backend/daemon substrate; behavior derived from backend events belongs to feature-owned handlers.
- Provider integration packages should normalize provider facts and provenance. Product workflow behavior belongs to the product feature that owns the workflow.
- Shared daemon IPC contracts belong in `src/daemon-ipc.ts` and use `defineIpcSchema()` from `@goddard-ai/ipc`; public composition roots combine fragments with `composeIpcSchemas()`.
- Feature tests should use fixtures shaped like the real upstream contracts. Do not loosen production types merely to make partial fixtures easier to write.
