# Component: TextModelSelector

- **Minimum Viable Component:** Reusable fieldset for editing one shared `Text Model Config` by choosing a provider first and a provider-scoped model second.
- **Props Interface:** `value: { provider?: string | null; model?: string | null }`; `providers: array of provider summaries { id, label }`; `popularProviderIds: string[]`; `models: array of selected-provider model summaries { id, label, lastUpdated }`; `validation?: { provider?: string | null; model?: string | null }`; `disabled?: boolean`; `readOnly?: boolean`; `onChange: (patch) => void`.
- **Sub-components:** None required for the MVP; searchable select popovers may stay internal.
- **State Complexity:** Pure form rendering plus lightweight local menu open or filter state; the parent surface owns catalog loading, draft persistence, and save or validation lifecycle.
- **Required Context:** None.
- **Electrobun RPC:** None directly; provider and model data should route through shared config or catalog adapters owned by a future configuration surface.
- **Interactions & Events:** Provider select renders two sections: `Most Popular Providers` using the curated `popularProviderIds` order and capped to roughly ten available providers, then `All Providers (A-Z)` using the remaining providers sorted by label; choosing a provider clears any stale model selection; the model select remains disabled until `value.provider` is non-empty; the model menu sorts by `lastUpdated` descending with alphabetical tie-breakers and unknown dates last; submit and save actions remain owned by the parent surface.
- **Dependencies:** Shared `Text Model Config` contract and catalog metadata from `ai-sdk-json-schema`; a future configuration or settings surface that can load provider and model metadata and persist config edits.
- **Likely Fit:** Deferred alongside `Configuration and Settings Editing`; reusable later in any settings or config-editing surface that needs one text-model picker.
- **Open Questions:** Whether provider search belongs in the MVP or can wait until a first host surface proves it necessary; whether the curated popular-provider ordering should live in one app constant or arrive from shared metadata in the future.
