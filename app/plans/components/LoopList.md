# Component: LoopList
- **Minimum Viable Component:** List of known loop definitions and active runtimes with controls for start, stop, and open-related-session.
- **Props Interface:** `loops: array of loop summary records`; `selectedLoopKey?: string | null`; `onSelect: (loopKey) => void`; `onStart: (loopKey) => void`; `onStop: (loopKey) => void`; `onOpenSession?: (sessionId) => void`.
- **Sub-components:** `LoopListRow`.
- **State Complexity:** Simple UI-only selection and empty-state handling.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Selects a loop; starts or stops a runtime; opens the currently attached session when one exists.
