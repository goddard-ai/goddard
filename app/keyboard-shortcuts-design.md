# Keyboard Shortcuts Design

## Overview / Problem Statement

The desktop app needs one coherent keyboard-shortcut subsystem that supports:

- stable typed command identifiers
- built-in default keymap profiles
- user overrides persisted outside the browser runtime
- one dispatch path shared by browser keyboard events and native menu actions
- a `Keyboard Shortcuts` workbench tab for discovery and rebinding

Today the app has hardcoded native menu accelerators in the Bun host and ad hoc event handling in the webview. That is not sufficient for customizable shortcuts, IDE-aligned default profiles, typed event-based listeners, or a user-facing shortcut browser.

This document defines the technical design for the app shortcut subsystem and the surrounding host, persistence, and UI contracts.

## Context

- The app runs as an Electrobun desktop shell with a Bun-owned host and a Preact webview.
- Shared app state should live in `preact-sigma` modules, not in view components.
- The app already routes native menu actions into the webview through shared event modules such as [app-menu.ts](./src/shared/app-menu.ts).
- `powerkeys` `0.1.0` is a document-bound shortcut runtime that supports combos, sequences, scopes, `when` clauses, editable-target policies, recording, and debug tracing.
- `preact-sigma` instances are typed `EventTarget` implementations and work cleanly with `useListener()`.
- In `app/`, `Action` already means a prompt-driven workflow from the glossary. Reusing `Action` for keyboard behavior would collide with existing app language.

## Goals

- Define every bindable shortcut command in one exported id module.
- Prevent other modules from relying on raw string literals for shortcut command ids.
- Support one built-in default keymap profile now, with room for future IDE-aligned profiles.
- Persist user overrides in a machine-readable JSON file under `~/.goddard/user/`.
- Merge built-in profiles with user overrides deterministically at startup and after edits.
- Expose shortcut dispatch as typed sigma events so app code can listen through `useListener()` or sigma `on(...)`.
- Make native menu invocation emit the same command events as keyboard dispatch.
- Provide a `Keyboard Shortcuts` workbench tab that lists every registered shortcut command, its label, its current bindings, and its `when` clause.
- Support two recording flows:
  - recording-search mode in the workbench tab
  - command rebinding in a lightweight capture modal

## Non-Goals

- Repository-local or project-local keymaps.
- OS-global shortcuts outside the current window or DOM boundary.
- User-editable `when` clauses, scopes, priorities, or other dispatch semantics.
- Fuzzy sequence matching before a sequence timeout expires.
- A generalized settings system beyond the shortcut feature.
- Replacing all debug-menu behavior in the first slice.

## Success Criteria

- A command can be renamed by changing one exported symbol instead of chasing string literals.
- The app can ship a default keymap profile and load a user override file without losing critical bindings when the override file is invalid.
- The same command id fires whether the user presses the keyboard shortcut or picks the native menu item.
- The `Keyboard Shortcuts` tab can find commands by typed text or by recorded shortcut input.
- The capture modal can record multi-step sequences and append a new expression after inactivity without reopening the modal.

## Assumptions and Constraints

- The runtime target is `document`. Shortcuts are app-wide, not local to one subtree.
- `powerkeys` only sees DOM keyboard events. Native menu accelerators remain a host concern and must be projected separately.
- The shortcut subsystem must remain a singleton because the app should have one authoritative binding set, one persisted keymap snapshot, and one recording owner.
- Shortcut dispatch is broadcast-based. The registry does not pick one listener. Ownership comes from scopes, context, and where listeners are attached.
- The user override file is app-only preference data. It is not part of root daemon or SDK execution configuration.
- Missing or invalid user overrides must fall back to built-in defaults instead of leaving the app unbound.

## Terminology

- `Shortcut Command`
  - One bindable app behavior addressed by a stable id such as `workbench.closeActiveTab`.
- `Shortcut Command Id`
  - The stable string exported from the command-id module and used as the registry event name.
- `Shortcut Label`
  - The human-readable display name shown in the shortcut browser UI.
- `Keymap Profile`
  - One built-in set of default shortcut expressions for known shortcut commands.
- `User Override File`
  - The persisted JSON document that selects a profile and overrides bindings for specific shortcut commands.
- `Resolved Keymap`
  - The effective bindings after the selected built-in profile is merged with the user override file.
- `Menu Projection`
  - The subset of resolved bindings that can be translated into native Electrobun accelerator strings.
- `Recording Search Mode`
  - A temporary search mode where live key capture replaces typed search input.
- `Capture Modal`
  - The lightweight modal used to record replacement bindings for one shortcut command.
- `Sequence Window`
  - The inactivity timeout after which a recorded multi-step expression is considered complete.

## Proposed Design

The subsystem has seven pieces:

1. one exported shortcut-command id module
2. one code-owned command-definition table
3. one or more built-in keymap profiles
4. one file-backed user override document
5. one module-scoped `ShortcutRegistry` sigma singleton backed by `powerkeys`
6. one Bun-host persistence and native-menu bridge
7. one `Keyboard Shortcuts` workbench tab plus capture modal

### Command ids

All bindable commands are declared in one module:

```ts
export const ShortcutCommands = {
  closeActiveTab: "workbench.closeActiveTab",
  newSession: "session.new",
  openGlobalSearch: "search.openGlobal",
} as const

export type ShortcutCommandId =
  (typeof ShortcutCommands)[keyof typeof ShortcutCommands]
```

Rules:

- other modules import from `ShortcutCommands`
- other modules do not write raw ids directly
- persisted override keys, registry event names, and menu dispatch all use `ShortcutCommandId`

### Command definitions

Each shortcut command has a code-owned definition:

```ts
type ShortcutCommandDefinition = {
  label: string
  description: string
  scope?: string | string[]
  when?: string
  keyEvent?: "keydown" | "keyup"
  priority?: number
  editablePolicy?: "inherit" | "ignore-editable" | "allow-in-editable" | "allow-if-meta"
  preventDefault?: boolean
  stopPropagation?: boolean
  allowRepeat?: boolean
  nativeMenuAction?: true
}
```

The definition owns runtime semantics. Profiles and user overrides only choose expressions such as `Mod+w` or `g g`.

### Keymap profiles

Built-in profiles are code-owned:

```ts
type KeymapProfileId = "goddard"

type KeymapBindings = Partial<Record<ShortcutCommandId, readonly string[]>>

type ShortcutKeymapProfile = {
  id: KeymapProfileId
  label: string
  bindings: KeymapBindings
}
```

The first slice ships only `goddard`. Later profiles such as `vscode` or `jetbrains` can be added without changing persistence format.

### User override file

The override file lives at `~/.goddard/user/keymap.json`.

Path resolution belongs in `@goddard-ai/paths/node`, not in an app-local string join. The Bun host owns file reads and writes through Electrobun RPC.

Proposed file shape:

```json
{
  "version": 1,
  "profile": "goddard",
  "overrides": {
    "workbench.closeActiveTab": ["Mod+w"],
    "search.openGlobal": ["Mod+k"],
    "session.new": null
  }
}
```

Semantics:

- `profile` selects the built-in base profile
- missing override key means inherit the profile binding list
- `null` means explicitly unbound
- a non-empty string array replaces the profile binding list
- an empty array is invalid and is rejected as malformed input

### ShortcutRegistry

`ShortcutRegistry` is a module-scoped sigma singleton backed by one `powerkeys` runtime.

Event map:

```ts
type ShortcutDispatchDetail = {
  commandId: ShortcutCommandId
  source: "keyboard" | "native-menu" | "programmatic"
  match?: ShortcutMatch
}

type ShortcutRegistryEvents = {
  [TCommandId in ShortcutCommandId]: ShortcutDispatchDetail
}
```

The registry owns:

- resolved profile plus override state
- bound `powerkeys` handles
- active scopes and `when`-clause context
- recording ownership
- persisted write status
- command-row read models for the browser UI

The registry should keep the initial context namespace small and stable instead of trying to mirror arbitrary app state.

### Workbench tab and capture modal

The UI surface is a closable detail tab titled `Keyboard Shortcuts`.

It shows one row per registered shortcut command with:

- shortcut label
- stable command id
- resolved expressions, if any
- `when` clause, if any

The tab has:

- a text input for typed search
- a keyboard-icon toggle button for recording-search mode

Clicking a row opens a lightweight capture modal. The modal records one or more expressions and persists changes only after explicit confirmation.

### Native menu projection

Commands marked `nativeMenuAction: true` participate in menu projection.

Projection rules:

- only single-step expressions are eligible
- sequence expressions never project into menu accelerators
- the first resolved expression that can be translated into Electrobun accelerator syntax wins
- if no expression is projection-safe, the menu item remains available without an accelerator label

## API / Interface Specification

### Registry creation

The registry bootstraps with:

- `document` as the `powerkeys` target
- `editablePolicy: "ignore-editable"` as the runtime default
- `getActiveScopes: () => activeScopes`
- `onError` wired to registry error reporting
- one shared `sequenceTimeout` used for dispatch and recording unless later UX work splits them

### Registry state

Minimum public state:

- `status: "idle" | "loading" | "ready" | "error"`
- `selectedProfileId: KeymapProfileId`
- `overrides: Partial<Record<ShortcutCommandId, readonly string[] | null>>`
- `resolvedBindings: Partial<Record<ShortcutCommandId, readonly string[]>>`
- `activeScopes: readonly string[]`
- `recordingState: { mode: "search" | "capture"; commandId?: ShortcutCommandId } | null`
- `recordingPreview: readonly string[]`
- `loadError: string | null`
- `writeError: string | null`

### Registry actions

Minimum public actions:

- `hydrate()`
- `setActiveScopes(scopes: readonly string[])`
- `setContext(path: string, value: unknown)`
- `batchContext(update: Record<string, unknown>)`
- `pause(scope?: string)`
- `resume(scope?: string)`
- `startSearchRecording()`
- `startCommandCapture(commandId: ShortcutCommandId)`
- `stopRecording()`
- `applyUserKeymap(nextFile: UserShortcutKeymapFile)`
- `saveCommandBindings(commandId: ShortcutCommandId, expressions: readonly string[] | null)`
- `resetCommandBindings(commandId: ShortcutCommandId)`
- `dispatch(commandId: ShortcutCommandId, detail?: Omit<ShortcutDispatchDetail, "commandId">)`
- `explain(event: KeyboardEvent)`

### Command-row read model

The workbench tab consumes a derived row model:

```ts
type ShortcutCommandRow = {
  commandId: ShortcutCommandId
  label: string
  when?: string
  expressions: readonly string[]
}
```

The view must derive this from the central registry or from registry-backed data. It must not duplicate label or `when` metadata by hand.

### Initial context surface

The initial implementation should track a minimal `powerkeys` context surface even before many bindings consume it.

Recommended initial context keys:

- `workbench.activeTabKind`
  - The current detail-tab kind or `main`.
- `workbench.hasClosableActiveTab`
  - Whether the active tab can be closed.
- `navigation.selectedNavId`
  - The current primary workbench view when the main tab is active.
- `overlay.isOpen`
  - Whether a modal or dialog currently owns top-level keyboard priority.
- `overlay.kind`
  - A stable identifier for the current modal or dialog when one is open.

Why these belong in the first slice:

- `workbench.hasClosableActiveTab` is the clean `when` gate for `workbench.closeActiveTab`.
- `workbench.activeTabKind` and `navigation.selectedNavId` are the most likely first discriminators for tab-specific shortcuts.
- `overlay.isOpen` and `overlay.kind` match `powerkeys`' intended use for transient app state such as modal visibility and keep future escape, submit, or dialog-local bindings from requiring a later context-model retrofit.

What should not be tracked yet:

- editor selection state
- read-only editor state
- auth state
- project or session selection details

Those can be added later when real bindings need them. The initial implementation should avoid speculative context sprawl.

### Host bridge

The Bun host adds RPC for:

- reading the user override file
- writing the user override file atomically
- reinstalling the native menu after a successful write

This may be separate requests or one small shortcut-keymap namespace. The important contract is that the write path does not resolve until persistence succeeds and the menu projection has been refreshed.

## Behavioral Semantics

### Startup and hydration

1. The Bun host installs the application menu from the current keymap snapshot when present, otherwise from the default `goddard` profile.
2. The webview creates the module-scoped registry.
3. `hydrate()` reads the user override file through host RPC.
4. The registry validates the file.
5. The registry resolves built-in profile plus overrides into `resolvedBindings`.
6. The registry binds all commands into `powerkeys`.
7. The registry exposes command-row read models for the UI.
8. The registry enters `ready`.

### Merge precedence

Precedence is:

1. built-in profile bindings
2. user override bindings for specific commands
3. runtime pause, scope, and `when` filtering during dispatch

The override file never mutates the built-in profile object.

### Override failure behavior

- unknown `profile` falls back to `goddard`
- unknown command ids in `overrides` are ignored
- invalid per-command values fall back to the selected profile binding for that command
- if rebinding fails completely, the registry keeps the last valid live binding set

### Dispatch semantics

- `powerkeys` guarantees at most one winning binding for a keyboard event
- a winning keyboard binding emits one registry event named by `ShortcutCommandId`
- native menu invocation emits the same event name with `source: "native-menu"`
- programmatic tests or UI affordances may dispatch with `source: "programmatic"`
- the registry does not consume command events after emission

### Search semantics

Typed search filters rows by case-insensitive substring match over:

- `label`
- `commandId`
- current resolved expressions

Recording-search mode semantics:

- recording-search mode is mutually exclusive with capture-modal recording
- entering recording-search mode calls `powerkeys.record({ suppressHandlers: true, consumeEvents: true })`
- while it is active, ordinary shortcut handlers do not fire
- pressing `Escape` exits recording-search mode instead of capturing `Escape`
- when a recorded expression completes, it becomes the current shortcut-search query
- after the sequence window expires, the next non-escape keypress starts a new expression and replaces the previous recorded query
- exiting recording-search mode does not implicitly clear the current query

### Capture-modal semantics

- opening the modal starts capture mode for the selected shortcut command
- capture mode is mutually exclusive with recording-search mode
- the modal captures canonical expressions through `powerkeys.record(...)`
- handlers are suppressed while the modal is recording
- when a recorded sequence completes, the expression is appended to the modal draft
- after one expression is finalized, the next keypress starts a new expression inside the same open modal
- the modal persists changes only after explicit confirmation
- cancelling or closing the modal discards uncommitted draft expressions

### Scope and context semantics

- `activeScopes` is ordered from highest precedence to lowest
- `powerkeys` appends `root` automatically
- scope and context are runtime-only state and are never persisted to the override file
- commands that should work in editable surfaces must opt in through code-owned definitions
- the first slice should actively keep `workbench.*`, `navigation.*`, and `overlay.*` context values synchronized even when only a subset of them are used by shipped bindings

## Architecture / Data Flow

### Keyboard dispatch path

1. The browser receives `keydown` or `keyup`.
2. `powerkeys` evaluates boundary, active scopes, editable policy, sequence state, and `when` clauses.
3. One binding wins.
4. The registry emits the matching `ShortcutCommandId`.
5. App code reacts through `useListener()` or `on(...)`.

### Native menu path

1. The user invokes a native menu item.
2. Electrobun executes the host menu action.
3. The host injects the matching `ShortcutCommandId` into the webview.
4. The registry emits the same command event with `source: "native-menu"`.
5. The same listeners run as if the binding came from the keyboard.

### Persistence path

1. The UI produces a new override snapshot.
2. The registry validates it before writing.
3. The browser sends it to the Bun host.
4. The host writes `~/.goddard/user/keymap.json` atomically.
5. The host rebuilds native menu projection from the updated keymap.
6. The browser applies the same snapshot and rebuilds its `powerkeys` bindings.

### Discovery and editing path

1. The user opens the `Keyboard Shortcuts` tab.
2. The view reads command rows from the registry.
3. The user filters by typed text or recording-search mode.
4. Clicking a row opens the capture modal.
5. The modal records one or more expressions.
6. Confirming the modal routes persistence through the registry and host write path.

## Alternatives and Tradeoffs

### Keep the name `ActionRegistry`

Rejected.

`Action` already has a product meaning in `app/`. Reusing it would create ambiguity with prompt-driven actions and `ActionCatalogState`.

### Use raw string literals everywhere

Rejected.

It is easy at first but makes rename safety, event typing, and definition-table exhaustiveness worse.

### Store shortcuts in `localStorage`

Rejected.

It is easy in the webview but cannot drive native menu setup at host startup and is scoped to one browser runtime instead of one desktop user preference source.

### Store shortcuts in root `goddard.json`

Rejected for this slice.

Keyboard shortcuts are app-only interaction preferences, not shared runtime execution config. Moving them into root config would couple the app to broader configuration precedence without a strong benefit.

### Commit shortcut changes immediately during capture

Rejected.

It feels fast but makes accidental overwrites too easy, especially when capture supports multiple inactivity-delimited expressions in one modal session.

## Failure Modes and Edge Cases

- missing keymap file falls back to the default profile
- invalid JSON falls back to the last valid runtime or to defaults
- unknown profile id falls back to `goddard`
- unknown command ids are ignored
- empty override arrays are rejected
- invalid expressions are rejected per command without discarding the whole resolved keymap
- commands with no resolved binding still appear in the browser UI as unbound
- multiple listeners may observe the same command event by design
- sequence bindings do not project into native menu accelerators
- host write failure preserves the previous live runtime and surfaces `writeError`
- unknown command ids injected by the native menu are ignored
- runtime rebuild failure after a valid file write preserves the previous bound runtime
- pressing `Escape` during recording-search mode exits search recording without treating it as the query
- inactivity during modal capture finalizes the current expression and waits for the next keypress to start another

## Testing and Observability

Minimum test coverage:

- command-id typing and definition-table exhaustiveness
- label presence on every registered shortcut command
- merge precedence across profile plus overrides
- `null` unbind semantics
- fallback on invalid per-command override
- fallback on invalid profile id
- keyboard dispatch emits the expected command id and `source: "keyboard"`
- native menu dispatch emits the same command id and `source: "native-menu"`
- scope precedence and `when` gating
- editable-target default blocking
- menu projection for valid single-step expressions
- no menu projection for sequences
- typed text search matches label, id, and expressions
- recording-search mode suppresses shortcut dispatch and exits on `Escape`
- capture-modal recording appends one expression per inactivity-delimited sequence
- cancelling the modal does not mutate persisted overrides
- write failure preserves the previous resolved keymap

Observability hooks:

- `loadError` and `writeError` on registry state
- `explain(event)` as a thin debug wrapper around `powerkeys.explain(...)`
- development-only logging for malformed overrides and menu projection failures

## Rollout / Migration

Phase 1:

- add command ids and command definitions
- add the built-in `goddard` profile
- add the user override file contract and Bun host bridge
- convert existing `closeTab` menu dispatch to the shared command-id path
- add the `Keyboard Shortcuts` tab and capture modal
- bind only critical existing commands first

Phase 2:

- add more polished settings entry points
- expand the built-in profile list
- consider file-watch reload of external manual edits

No backward-compatibility migration is required because the app does not already ship persisted shortcut preferences.

## Open Questions

- Should debug-only menu surfaces become shortcut commands or remain separate debug-menu affordances?
  - Current recommendation: keep them separate in the first slice.
- Should the app live-reload manual edits to `~/.goddard/user/keymap.json` while running?
  - Current recommendation: defer file watching until after the basic persistence flow lands.

## Ambiguities and Blockers

- AB-1 - Resolved - Terminology collision with app `Action`
  - Affected area: Terminology / API
  - Issue: `ActionRegistry` conflicts with the existing glossary meaning of `Action`.
  - Why it matters: The codebase already uses `Action` for prompt workflows.
  - Next step: Use `ShortcutCommand` and `ShortcutRegistry` consistently.

- AB-2 - Resolved - Persistence boundary
  - Affected area: Persistence / Architecture
  - Issue: The original plan named `~/.goddard/user/` without defining whether shortcuts should join shared runtime config.
  - Why it matters: App-only shortcut preferences should not silently become daemon config.
  - Next step: Keep them in a dedicated user keymap file resolved through `@goddard-ai/paths/node`.

- AB-3 - Non-blocking - External file edit live reload
  - Affected area: Operational behavior
  - Issue: The design does not require a file watcher for manual edits made while the app is running.
  - Why it matters: Advanced users may expect live reload from direct file edits.
  - Next step: Revisit after the first implementation exists.

- AB-4 - Deferred - Capture-modal interaction polish
  - Affected area: UI behavior
  - Issue: The runtime semantics are defined, but the exact interaction polish of the lightweight modal is still flexible.
  - Why it matters: The UI can vary without changing the underlying recording and persistence contract.
  - Next step: Finalize during implementation review.

## Appendix / Example Initial Commands

- `workbench.closeActiveTab`
- `session.new`
- `search.openGlobal`
- `navigation.openInbox`
- `navigation.openSessions`
- `navigation.openSpecs`
- `navigation.openTasks`
- `navigation.openRoadmap`

Unbound commands are valid. The registry does not require every defined command to ship with a default binding on day one.
