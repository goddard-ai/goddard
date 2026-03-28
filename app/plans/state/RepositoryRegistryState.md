# State Module: RepositoryRegistryState
- **Responsibility:** Own the machine-wide registry of repositories the user has explicitly added to the workspace.
- **Data Shape:** Normalized managed repository records keyed by repository id; list ordering; add-dialog draft; inspection metadata for selected filesystem paths; loading and error state; derived capability flags such as local config, actions, loops, specs, or tasks being present.
- **Mutations/Actions:** `loadManagedRepositories`; `openAddRepositoryDialog`; `closeAddRepositoryDialog`; `browseForRepository`; `inspectRepositoryPath`; `addManagedRepository`; `removeManagedRepository`; `refreshRepositoryMetadata`.
- **Scope & Hoisting:** Hoisted into a global provider because repository metadata is referenced by session launch, pull request filters, action scopes, loop filters, specs, tasks, and roadmap views.
- **Side Effects:** Uses host file-picker and filesystem inspection adapters; persists the managed repository registry to workspace storage; refreshes repository-derived capability metadata after add or removal.
