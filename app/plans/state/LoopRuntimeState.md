# State Module: LoopRuntimeState

- **Responsibility:** Own loop definition discovery, runtime status inspection, and lifecycle actions for daemon-managed loops.
- **Data Shape:** Loop definition records keyed by project and loop name; active runtime status keyed the same way; filter fields; start-dialog draft; loading, submission, and error state; optional linked session ids.
- **Mutations/Actions:** `loadLoops`; `refreshLoops`; `setLoopFilters`; `openStartLoopDialog`; `closeStartLoopDialog`; `setStartLoopDraft`; `startLoop`; `stopLoop`; `openLoopSession`.
- **Scope & Hoisting:** Hoisted into a shared provider because loop status appears on the loops page, may feed global search, and can open linked session tabs.
- **Side Effects:** Reads loop definitions from shared config roots; calls daemon loop lifecycle APIs; refreshes runtime state after start or stop operations; correlates running loops with session records when session ids are available.
