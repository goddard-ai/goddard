# Component: KeyboardShortcutCaptureDialog
- **Minimum Viable Component:** Lightweight modal for recording one shortcut command’s replacement bindings and confirming or discarding that draft.
- **Props Interface:** `commandId: string`; `label: string`; `whenClause?: string`; `currentExpressions: readonly string[]`; `isOpen: boolean`; `onSave: (expressions: readonly string[] | null) => void`; `onClose: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple local draft-expression list and modal focus state; active recording session, canonical expression capture, and persistence belong in `ShortcutRegistry`.
- **Required Context:** `ShortcutRegistryContext`.
- **Electrobun RPC:** None directly.
- **Interactions & Events:** Starts command-capture mode when opened; shows live captured key presses; finalizes one recorded sequence after inactivity; starts a fresh sequence on the next keypress; lets the user confirm the draft, clear it to unbind, or cancel without writing changes.
