# Component: StartLoopDialog
- **Minimum Viable Component:** Modal flow for starting one daemon-managed loop with repository context and optional runtime overrides.
- **Props Interface:** `isOpen: boolean`; `draft: { repositoryId, loopName, cycleDelay, maxOpsPerMinute, maxCyclesBeforePause }`; `repositories: array of managed repository summaries`; `availableLoops: array of { id, name, repositoryId? }`; `validation: { canSubmit, fieldErrors }`; `isSubmitting: boolean`; `onDraftChange: (patch) => void`; `onSubmit: () => void`; `onClose: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only modal and validation message state; lifecycle and submission belong in `LoopRuntimeState`.
- **Required Context:** `LoopRuntimeContext`, `RepositoryRegistryContext`.
- **Tauri IPC:** None directly; loop start requests route through `LoopRuntimeState`.
- **Interactions & Events:** Selects a repository and loop definition; overrides runtime limits when allowed; starts the loop runtime; closes on success or cancellation.
