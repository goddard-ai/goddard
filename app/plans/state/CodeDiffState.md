# State Module: CodeDiffState

- **Current Baseline:** There is no shared diff owner today. `session-changes` loads session diffs directly through `goddardSdk.session.changes`, and that should stay the default until another feature needs the same data or selection state.
- **Responsibility:** If introduced, own normalized diff records and per-source presentation state for diff sources reused across session changes, pull request detail, and turn-change summaries.
- **Data Shape:** Map keyed by `{ kind, id }` containing source metadata, raw diff text, optional normalized file records, selected file path, load status, error state, and last refresh timestamp.
- **Mutations/Actions:** `loadDiffSource`; `refreshDiffSource`; `setSelectedFile`; `replaceDiffSource`; `clearDiffError`; `evictDiffSource`.
- **Scope & Hoisting:** Do not hoist preemptively. Keep a shared owner only when at least two surfaces need the same diff cache or selection state.
- **Side Effects:** Fetch diff payloads through the SDK or host adapter that owns the source. Use `queryClient` invalidation for simple SDK reads before adding custom cache behavior.
