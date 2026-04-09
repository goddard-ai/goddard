# State Module: ShortcutRegistry
- **Goal:** Own the app-wide keyboard shortcut registry so the desktop workspace can resolve built-in keymaps plus user overrides, dispatch typed shortcut-command events, and back the `Keyboard Shortcuts` workbench tab.
- **Scope:** Covers shortcut-command definitions, resolved bindings, recording ownership, persistence status, command-row read models, and native-menu projection inputs. It does not own the tab’s local search draft or modal open state.
- **Why now:** The app already has hardcoded menu accelerators and ad hoc dispatch. Customizable shortcuts need one shared state module before the workbench tab or capture modal can stay thin.

## State

- `status`
  - `idle`, `loading`, `ready`, or `error`.
- `selectedProfileId`
  - The active built-in keymap profile.
- `overrides`
  - Per-command user overrides or explicit unbinds.
- `resolvedBindings`
  - The merged effective expressions per shortcut command.
- `activeScopes`
  - Runtime scopes in precedence order for `powerkeys`.
- `runtimeContext`
  - Minimal tracked context for `workbench.*`, `navigation.*`, and `overlay.*` values that future `when` clauses will depend on.
- `recordingState`
  - Either idle, search recording, or capture recording for one shortcut command.
- `recordingPreview`
  - The currently captured expression list for the active recording session.
- `loadError` and `writeError`
  - User-facing error state for invalid files or failed writes.
- `commandRows`
  - Derived rows for the `Keyboard Shortcuts` tab: label, command id, expressions, and optional `when` clause.

## Mutations / Actions

- `hydrate`
  - Loads the user override file, resolves the effective keymap, and binds the runtime.
- `setActiveScopes`
  - Updates scope precedence for dispatch.
- `setContext` and `batchContext`
  - Feed `when`-clause state into the underlying shortcut runtime.
- `syncWorkbenchContext`
  - Keeps `workbench.activeTabKind`, `workbench.hasClosableActiveTab`, and `navigation.selectedNavId` current.
- `syncOverlayContext`
  - Keeps `overlay.isOpen` and `overlay.kind` current.
- `pause` and `resume`
  - Temporarily disable dispatch globally or by scope.
- `startSearchRecording`
  - Starts recording-search mode for the workbench tab.
- `startCommandCapture`
  - Starts capture mode for one shortcut command.
- `stopRecording`
  - Ends the active recording session.
- `saveCommandBindings`
  - Persists one command’s replacement expressions or explicit unbind.
- `resetCommandBindings`
  - Restores one command to its selected-profile defaults.
- `dispatch`
  - Emits one typed shortcut-command event for keyboard, native-menu, or programmatic sources.
- `explain`
  - Exposes debug tracing for one keyboard event.

## Scope & Hoisting

- Hoisted into a global provider because shortcut dispatch, native-menu handling, and the `Keyboard Shortcuts` tab all need the same runtime and resolved keymap state.
- Must remain a singleton. Multiple shortcut runtimes would make dispatch, recording ownership, and persistence ambiguous.

## Dependencies

- `powerkeys`
  - Owns combo and sequence matching, `when` evaluation, scopes, recording, and trace output.
- `preact-sigma`
  - Owns typed event dispatch and shared state exposure to components.
- Electrobun RPC host bridge
  - Reads and writes the user keymap file and refreshes menu accelerators after successful writes.
- `@goddard-ai/paths/node`
  - Resolves the user keymap path under `~/.goddard/user/`.

## Side Effects

- Reads the user override JSON file through the Bun host bridge during hydration.
- Writes validated override snapshots through the Bun host bridge when bindings change.
- Rebuilds the `powerkeys` binding set when the resolved keymap changes.
- Supplies native-menu projection inputs so the Bun host can keep accelerators aligned with the resolved keymap.
- Emits typed shortcut-command events that components and other state modules consume with `useListener()` or sigma listeners.
- Keeps a small stable context namespace synchronized from app shell state even before many shipped bindings use every key.

## Related Components

- `KeyboardShortcutsView`
- `KeyboardShortcutCaptureDialog`

## Open Questions

- Whether external manual edits to the keymap file should live-reload while the app is open.
- Whether debug-only menu surfaces should stay outside the shared shortcut registry in the first slice.
- Whether modal ownership should be fed into the registry from one shared overlay source immediately or by incremental call sites until a shared overlay model exists.
