# State Module: ProjectContextState
- **Responsibility:** Own the app-wide active project context, persisted recent-project order, and focused-tab project resolution for project-aware tabs. Do not own the full project registry.
- **Data Shape:** `activeProjectPath`; `recentProjectPaths`; ephemeral `reportedTabProjectsByTabId`; derived `activeProject`; derived `orderedProjects`.
- **Mutations/Actions:** `hydrate`; `setActiveProject`; `applyFocusedTabProject`; `reportTabProject`; `clearTabProject`; `removeProject`; one helper for nearest-containing-project resolution or access to a shared resolver.
- **Scope & Hoisting:** Shared app-shell state. Hoist this into app state because the header button, switch-project dropdown, focused-tab coordination, and project-default actions all depend on one consistent source of truth.
- **Side Effects:** Persist active and recent-project state to workspace storage; prune stale project references after registry changes; ignore async tab reports for tabs that are no longer focused.
