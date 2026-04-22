# State Module: CodeDiffState

- **Responsibility:** Normalize and cache diff data so the same diff viewer can serve standalone diff tabs and pull request diff sections.
- **Data Shape:** One map keyed by diff source id containing file-level diff records, presentation metadata, selected file path, load status, error state, and optional scroll or viewport restoration snapshots.
- **Mutations/Actions:** `loadDiff`; `refreshDiff`; `setSelectedFile`; `setPresentationMode`; `restoreViewport`; `evictUnusedDiff`.
- **Scope & Hoisting:** Hoisted into a shared provider because diffs can be reopened from sessions, inbox items, and pull request views using the same stable diff source.
- **Side Effects:** Fetches diff payloads from whichever adapter owns the source record; caches successful loads in memory so LRU-closed tabs can reopen quickly; can coordinate invalidation when the underlying session or pull request updates.
