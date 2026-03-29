# State Module: NavigationState
- **Responsibility:** Own the icon-sidebar navigation model for the primary workbench view, including the selected domain, badge counts, and any persisted last-selection behavior.
- **Data Shape:** One registry of nav items keyed by id with icon metadata and availability flags; one selected nav id; derived badge counts per domain; one persisted snapshot version for restoring the last selected primary view.
- **Mutations/Actions:** `selectNavItem`; `registerNavItems`; `setBadgeCount`; `hydrateNavigation`; `resetNavigation`.
- **Scope & Hoisting:** Hoisted into a global provider at the app shell because the selected primary view drives the non-closable main workbench surface.
- **Side Effects:** Persists the selected nav id to workspace storage; may subscribe to inbox or session state changes to derive badge counts without making the sidebar query those modules directly.
