# Component: StartLoopDialog
- **Minimum Viable Component:** Modal flow for starting one daemon-managed loop with project context and optional runtime overrides.
- **Props Interface:** `isOpen: boolean`; `draft: { projectPath, loopName, cycleDelay, maxOpsPerMinute, maxCyclesBeforePause }`; `projects: array of managed project summaries`; `availableLoops: array of { id, name, projectPath? }`; `validation: { canSubmit, fieldErrors }`; `isSubmitting: boolean`; `onDraftChange: (patch) => void`; `onSubmit: () => void`; `onClose: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only modal and validation message state; lifecycle and submission belong in `LoopRuntimeState`.
- **Required Context:** `LoopRuntimeContext`, `ProjectRegistryContext`.
- **Electrobun RPC:** None directly; loop start requests route through `LoopRuntimeState`.
- **Interactions & Events:** Selects a project and loop definition; overrides runtime limits when allowed; starts the loop runtime; closes on success or cancellation.
