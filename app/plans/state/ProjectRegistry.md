# State Module: ProjectRegistry
- **Responsibility:** Own the machine-wide registry of projects the user has explicitly added to the workspace.
- **Data Shape:** Normalized project records keyed by project path plus stable list ordering. Keep only `{ path, name }` in persisted shared state.
- **Mutations/Actions:** `loadProjects`; `addProject`; `removeProject`.
- **Scope & Hoisting:** Hoisted into a global provider because project identity is referenced by session launch, project-scoped actions, loop filters, specs, tasks, and roadmap views.
- **Side Effects:** Persists the managed project registry to workspace storage. Directory validation and picker flows should live outside sigma in UI-local query-backed flows or small service adapters.
