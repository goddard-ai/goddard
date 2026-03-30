# State Module: RoadmapState
- **Responsibility:** Own roadmap proposal discovery, filtering, prioritization, and proposal detail mutations using a list-first model.
- **Data Shape:** Normalized proposal records keyed by proposal id; ordered visible ids; filters for project, status, owner, and query; selected proposal id; loading and error state.
- **Mutations/Actions:** `loadProposals`; `refreshProposals`; `setProposalFilters`; `mergeProposalUpdate`; `openProposal`; `updateProposalStatus`; `updateProposalOwner`; `updateProposalPriority`; `clearProposalFilters`.
- **Scope & Hoisting:** Hoisted into a shared provider because roadmap summaries, proposal detail tabs, and global search all need the same normalized proposal records.
- **Side Effects:** Fetches proposal records through shared app or SDK adapters; routes proposal detail openings through `WorkbenchTabsState`; keeps roadmap interactions aligned with the same cross-project data model used elsewhere in the app.
