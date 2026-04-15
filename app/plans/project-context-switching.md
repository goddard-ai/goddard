# Design: Switch Project and Sticky Project Context

## Overview / Problem Statement

The centered header button currently opens the generic command menu and displays the selected primary navigation label. That behavior does not give the app one stable default project for project-scoped actions, and it forces project switching into secondary flows such as the Projects page or session form selects.

This design replaces that header button with a `Switch Project` control. The control displays the current project context, opens a searchable dropdown of every opened project, supports the existing `Open folder` flow as the first empty-search action, and establishes one persisted active project context that project-related actions can use as their default.

The goal is not only faster project switching. The goal is one precise app-wide rule for "which project is current right now" without forcing every tab or action flow to invent its own fallback logic.

## Context

- The app shell header button in `app/src/app-shell/chrome.tsx` currently triggers `navigation.openCommandMenu`.
- The project registry in `app/src/projects/project-registry.ts` persists opened projects, but it does not persist a distinct active project or a most-recently-active ordering.
- `project` detail tabs already carry `projectPath` in tab payload.
- `sessionChat` detail tabs only carry `sessionId`; the actual `cwd` is fetched inside the tab view.
- The native app menu in `app/src/bun/menu.ts` already dispatches `AppCommand` actions into the webview, so command-menu access can remain available there after the header button changes purpose.

## Goals

- Replace the current header command-menu button with a `Switch Project` button.
- Show the active project name in that button when a project context exists.
- Maintain one persisted active project context that defaults project-related actions.
- Derive project context from the focused tab when the tab is project-aware.
- Keep the current project context unchanged when the focused tab is contextless.
- Order dropdown projects by most recently active, persist that order, and keep it stable across restart.
- Make `Mod+o` open the switch-project dropdown.
- Make empty-search `Mod+o Enter` trigger the existing `Open folder` flow.
- Keep the generic command palette reachable from the native `View` menu.

## Non-Goals

- Redesign the generic command palette beyond moving its primary entry point.
- Infer project context from in-tab selections, hover state, or partially focused subviews.
- Add new SDK or daemon behavior. This is app-only state and UI behavior.
- Rework the full Projects page information architecture.
- Introduce broad compatibility shims for older persisted workbench tab payloads.

## Assumptions and Constraints

- The repo is pre-alpha. The implementation can choose the simplest forward-looking model.
- `Project` keeps the existing app glossary meaning: one user-added local root, whether or not it is a git repository.
- The active project context determines the default project for project-related actions. The first concrete consumer in the current app is the new-session flow.
- A project-aware tab resolves to the nearest containing opened project, not only an exact path match.
- The switch-project surface is a dropdown anchored to the header button, not a centered modal replacement for the command palette.
- Contextless tabs must not clear the active project context.

## Terminology

- `Active Project`: The project whose name is currently displayed in the switch-project button.
- `Project Context`: The app-wide default project used by project-related actions when they need one project and the current tab does not provide a more specific override.
- `Project-Aware Tab`: A focused tab that can resolve to one opened project path, either directly from payload or indirectly from loaded data such as `session.cwd`.
- `Contextless Tab`: A focused tab that does not resolve to any opened project path.
- `Recent Project Order`: The persisted ordering of opened projects by most recent time they became the active project.

## Proposed Design

### 1. Add a dedicated `ProjectContextState`

Introduce a new app state module that is separate from `ProjectRegistry`.

Responsibilities:

- Persist the current active project path.
- Persist recent-project ordering.
- Accept project-context updates from focused tabs and manual project selection.
- Clean up active and recent references when a project is removed from the registry.

This state must stay separate from `ProjectRegistry` because the registry owns machine-wide project identity, while project context owns UI focus semantics and action defaults.

### 2. Replace the header button contract

`AppShellChrome` should stop receiving `selectedNavigationLabel` for the centered button. It should instead receive:

- the active project record or `null`
- a handler that opens the switch-project dropdown

Button label behavior:

- When `activeProjectPath` resolves to a current registry record, show that record’s `name`.
- When there is no active project, show `Open project`.

### 3. Add a switch-project dropdown

Add a new dropdown surface owned by the app shell.

Required behavior:

- Opens from the header button.
- Opens from a new `navigation.openSwitchProject` app command bound to `Mod+o`.
- Auto-focuses the search input on open.
- Filters the project list as the user types.
- Uses keyboard highlight state so `Enter` activates the current selection while focus remains in the search field.
- Closes after a successful project selection or `Open folder` action dispatch.

List behavior:

- When the search query is empty, render `Open folder…` as the first action row.
- When the search query is non-empty, do not render `Open folder…`.
- Render every opened project after the optional `Open folder…` row.
- Sort project rows by recent-project order, then by remaining registry order for never-active projects.
- Highlight the first row by default on open so `Mod+o Enter` triggers `Open folder…` when the query is empty.

Selection behavior:

- Selecting a project sets it as the active project.
- Selecting a project also opens or focuses that project’s `project` detail tab.
- Selecting `Open folder…` runs the existing filesystem picker flow. If the user picks a folder, add it to the registry when needed, set it active, and open or focus its `project` tab. If the user cancels the picker, leave the active project unchanged.

### 4. Resolve project context from the focused tab

Project context updates come from two sources:

- synchronous tab resolution in the app shell for tab kinds whose payload already carries enough data
- asynchronous tab reports from views that learn their project only after loading data

Synchronous resolution:

- `project` detail tabs resolve directly from `payload.projectPath`
- all current main-navigation surfaces are contextless for this feature

Asynchronous resolution:

- `sessionChat` resolves from `session.cwd` after the tab view loads session data
- the session tab reports its resolved opened-project path into shared project-context state
- the state only applies a reported tab context when that tab is still focused

Project matching rule:

- Resolve a candidate filesystem path to the nearest containing opened project.
- "Nearest" means the longest opened project path that contains the candidate path on a real path-segment boundary.
- Exact project-path matches also count.
- If no opened project contains the candidate path, the tab is contextless for project-context purposes.

Example:

- Opened projects: `/repo`, `/repo/packages/ui`
- Candidate path: `/repo/packages/ui/src`
- Resolved project: `/repo/packages/ui`

### 5. Make project context sticky across contextless tabs

When focus moves from a project-aware tab to a contextless tab, the active project must remain unchanged.

Examples:

- If the user focuses a session tab under project `alpha`, then switches to the Sessions page, `alpha` stays active.
- If the user manually selects project `beta` from the dropdown, then focuses Inbox, `beta` stays active.

Contextless focus changes therefore do not mutate the active project or recent-project order.

### 6. Use project context as the default project for project-related actions

The first concrete integration is the new-session flow:

- Opening the new-session dialog should seed its preferred project path from `ProjectContextState.activeProjectPath`.
- If the focused tab provides a more specific project-aware context later, that context becomes the new default automatically because it updates `activeProjectPath`.

Future project-related actions should read the same state instead of re-deriving their own default project.

### 7. Preserve command palette access in the native `View` menu

Keep `navigation.openCommandMenu` and the existing command palette dialog.

Change its primary access path:

- remove it from the centered header button
- add a native `View -> Command Palette` menu item that dispatches `navigation.openCommandMenu`

This keeps the command palette available without competing with the new project-switching role of the header button.

## API / Interface Specification

### `ProjectContextState`

Planned public shape:

- persisted state
  - `activeProjectPath: string | null`
  - `recentProjectPaths: string[]`
- ephemeral state
  - tab-scoped reported project contexts keyed by tab id
- computed values
  - `activeProject`
  - `orderedProjects`
- actions
  - `hydrate()`
  - `setActiveProject(path: string | null, source: "manual" | "focused-tab" | "hydrate-fallback")`
  - `applyFocusedTabProject(tabId: string, path: string | null)`
  - `reportTabProject(tabId: string, path: string | null)`
  - `clearTabProject(tabId: string)`
  - `removeProject(path: string)`

State invariants:

- `activeProjectPath` must be `null` or present in the current project registry.
- `recentProjectPaths` must be deduplicated and only contain current registry paths.
- Every transition that changes `activeProjectPath` to a non-null project path must move that path to the front of `recentProjectPaths`.
- Applying a `null` focused-tab project must not clear the active project.

### `navigation.openSwitchProject`

New app command:

- purpose: open the switch-project dropdown
- default shortcut: `Mod+o`
- availability: global

### `projects.openFolder`

Existing command behavior remains, but the new dropdown becomes a first-class entry point for it when search is empty.

### Native `View` menu entry

New menu item:

- label: `Command Palette`
- action: dispatch `navigation.openCommandMenu`
- scope: native app menu only

No new shared schema or SDK contract is required.

## Behavioral Semantics

### Active-project precedence

The active project can change in three ways:

1. Manual selection in the switch-project dropdown.
2. Focus changing to a project-aware tab that resolves to an opened project.
3. Hydration fallback during app startup when no focused-tab resolution is available yet.

Precedence:

- Manual selection wins immediately and opens or focuses the selected project tab.
- If the newly focused tab resolves to a different project, the focused tab wins because its project is now displayed in the button.
- A contextless focused tab never clears the current active project.

### Recency updates

Update `recentProjectPaths` only when the active project path changes to a non-null value.

Do not update recency when:

- the active project remains the same
- the focused tab becomes contextless
- the user cancels the `Open folder…` picker

### Startup hydration

Hydration order:

1. Hydrate `ProjectRegistry`.
2. Hydrate `WorkbenchTabSet`.
3. Hydrate `ProjectContextState`.
4. Let focused-tab resolution run and override the hydrated active project when the focused tab is project-aware.

Hydration fallback rules:

- Drop persisted paths that no longer exist in the project registry.
- If the focused tab synchronously resolves to a project, use it.
- Otherwise, keep the persisted `activeProjectPath` when it is still valid.
- Otherwise, use the first valid recent-project path.
- Otherwise, leave the active project `null`.

### Project removal

When a project is removed from the registry:

- remove it from `recentProjectPaths`
- clear `activeProjectPath` if it was active
- if the currently focused tab resolves to another opened project, switch to that project
- otherwise fall back to the next recent project or `null`

### Search filtering

The dropdown search should match project `name` and `path`.

Search semantics:

- case-insensitive
- substring match
- local-only; no async lookup

### `Open folder…` execution

Execution order:

1. Close the dropdown.
2. Open the filesystem picker.
3. If canceled, stop with no state change.
4. If a path is returned, upsert the project into the registry.
5. Set the returned project active.
6. Open or focus its project tab.

## Architecture / Data Flow

### Main flow: focused tab changes

1. The user focuses a different workbench tab.
2. The app shell attempts synchronous project resolution from tab payload.
3. If the tab resolves immediately, `ProjectContextState` applies it and updates recency when needed.
4. If the tab requires async data, the tab view loads its backing data and reports the resolved project path.
5. `ProjectContextState` ignores async reports for tabs that are no longer focused.
6. `AppShellChrome` re-renders the button label from the active project record.

### Main flow: manual switch from dropdown

1. The user opens the switch-project dropdown with the header button or `Mod+o`.
2. The dropdown auto-focuses search and highlights the first row.
3. The user selects a project or hits `Enter`.
4. The app sets that project active, updates recency, and opens or focuses the project tab.
5. Project-related actions now default to that project until a project-aware focused tab changes it.

### Main flow: `Mod+o Enter`

1. `Mod+o` opens the dropdown.
2. Search is empty and focused.
3. `Open folder…` is row 1 and highlighted by default.
4. `Enter` dispatches the filesystem picker flow.

### Affected app surfaces

- `app/src/app-shell.tsx`
- `app/src/app-shell/chrome.tsx`
- `app/src/app-state-context.tsx`
- `app/src/commands/app-command.ts`
- `app/src/shared/shortcut-keymap.ts`
- `app/src/bun/menu.ts`
- `app/src/projects/project-registry.ts`
- `app/src/sessions/dialog.tsx`
- `app/src/session-chat/view.tsx`
- new project-context state and dropdown modules

## Alternatives and Tradeoffs

### Alternative: store active-project semantics inside `ProjectRegistry`

Rejected.

Why:

- It mixes machine-wide project identity with workbench-local focus and recency behavior.
- Registry operations and UI-context operations have different invariants.
- Removing a project would need UI-specific behavior inside the registry model.

Tradeoff:

- Separate state means one more model to hydrate.
- The benefit is clearer ownership and lower coupling.

### Alternative: resolve project context only from static tab payload

Rejected.

Why:

- `sessionChat` tabs do not have enough synchronous data today.
- Restored session tabs would lose correct project inference until manually reopened.

Tradeoff:

- Allowing async tab reports adds a small amount of coordination logic.
- The benefit is correct behavior for restored tabs and future async project-aware tabs.

### Alternative: reuse the generic command palette as the project switcher

Rejected.

Why:

- The requested interaction is a dropdown anchored to the current-project button.
- The project switcher has distinct semantics: sticky active context, `Open folder…` first on empty search, and project-tab focus on selection.

Tradeoff:

- This introduces a separate surface instead of one universal command menu.
- The benefit is tighter semantics and faster project switching.

## Failure Modes and Edge Cases

- No opened projects:
  - Button label is `Open project`.
  - Empty-search dropdown shows only `Open folder…`.
- Focused tab path is outside every opened project:
  - The tab is contextless.
  - The active project remains unchanged.
- Nested opened projects:
  - Choose the longest containing path.
- Active project renamed in the registry:
  - Keep the active path.
  - The button text updates to the new project name.
- Active project removed:
  - Remove it from active and recent state, then fall back as specified above.
- Async tab reports arrive late:
  - Ignore them unless their tab is still focused.
- Search has no results:
  - Show an empty state.
  - `Enter` does nothing.

## Testing and Observability

Primary automated coverage should target state and pure resolution logic rather than UI snapshot tests.

Add tests for:

- nearest-containing-project resolution
- sticky active-project behavior across contextless tabs
- recency ordering and persistence cleanup
- hydration fallback rules
- project removal cleanup
- ignoring stale async tab reports

Manual QA should cover:

- `Mod+o Enter` opens the folder picker
- selecting a project focuses its project tab
- focusing a session tab updates the button label to the nearest containing opened project
- moving to Inbox, Sessions, or Projects leaves the current project unchanged
- command palette still opens from the native `View` menu

## Rollout / Migration

- Add a new workspace storage key for project-context state, for example `goddard.app.project-context.v1`.
- Keep existing project-registry storage intact.
- No SDK, daemon, or schema migration is needed.
- No spec edits are part of this change.

## Open Questions

None for the current implementation plan.

## Ambiguities and Blockers

- AB-1 - Resolved - Project selection side effect
  - Affected area: Behavioral Semantics / Dropdown selection
  - Issue: It was unclear whether choosing a project should only change default context or also navigate.
  - Why it matters: It changes both selection semantics and user expectation after choosing a project.
  - Next step: Resolved to set active context and open or focus that project tab.

- AB-2 - Resolved - Project inference rule
  - Affected area: Focused-tab resolution
  - Issue: It was unclear whether focused-tab inference used exact project-path match or a containing-project rule.
  - Why it matters: Session tabs often live below the opened project root.
  - Next step: Resolved to choose the nearest containing opened project.

- AB-3 - Resolved - Command palette access after header-button repurpose
  - Affected area: Native menu / command access
  - Issue: Replacing the centered command-menu button could strand the generic command palette.
  - Why it matters: Existing command-menu functionality still needs a supported entry point.
  - Next step: Resolved to add a native `View -> Command Palette` item.

- AB-4 - Resolved - Recency lifetime
  - Affected area: Persistence
  - Issue: It was unclear whether most-recently-active ordering only needed to live in memory.
  - Why it matters: The dropdown order would feel unstable across relaunches.
  - Next step: Resolved to persist active-project state and recent-project order.

## Appendix / Example

Example switch flow:

1. The user is on a session tab with `cwd` `/work/acme/app`.
2. Opened projects are `/work/acme` and `/work/acme/app`.
3. The active project becomes `/work/acme/app` because it is the nearest containing opened project.
4. The user switches to Inbox.
5. The button still displays the `/work/acme/app` project name.
6. The user presses `Mod+o`, types `plat`, and hits `Enter`.
7. The selected project becomes active and its project tab is focused.
