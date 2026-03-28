# State Module: TaskListState
- **Responsibility:** Own task list discovery, filtering, prioritization, and task detail mutations using a list-first model rather than a board model.
- **Data Shape:** Normalized task records keyed by task id; ordered visible ids; filters for repository, status, owner, and query; selected task id; loading and error state.
- **Mutations/Actions:** `loadTasks`; `refreshTasks`; `setTaskFilters`; `mergeTaskUpdate`; `openTask`; `updateTaskStatus`; `updateTaskOwner`; `updateTaskPriority`; `clearTaskFilters`.
- **Scope & Hoisting:** Hoisted into a shared provider because task summaries, task detail tabs, and global search all need the same normalized task records.
- **Side Effects:** Fetches task records through shared app or SDK adapters; routes task detail openings through `WorkbenchTabsState`; persists user task-list filters as workspace preferences only after a dedicated preferences surface exists.
