# Component: LoopListRow
- **Minimum Viable Component:** One loop row that shows repository, loop name, runtime state, cycle metrics, and lifecycle actions.
- **Props Interface:** `loop: { key, rootDirLabel, loopName, runtimeState, cycleCount, lastPromptAt, sessionId? }`; `isSelected: boolean`; `onSelect: (loopKey) => void`; `onStart: (loopKey) => void`; `onStop: (loopKey) => void`; `onOpenSession?: (sessionId) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only hover and action visibility state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Selects the loop; starts or stops the runtime; opens the linked session chat when present.
