# State Module: ActionDraftState

- **Responsibility:** Manage creation, editing, validation, and persistence of individual action drafts.
- **Data Shape:** Drafts keyed by action id or temporary draft id; metadata fields such as name and scope; prompt content; dirty status; validation errors; save status; source information describing whether the action is global or project-local.
- **Mutations/Actions:** `openActionDraft`; `createActionDraft`; `editActionDraft`; `saveActionDraft`; `revertActionDraft`; `closeActionDraft`; `duplicateActionDraft`; `deleteActionDraft`.
- **Scope & Hoisting:** Hoisted into a shared provider keyed by action id so action editor tabs can stay alive independently and reopen with their draft state intact.
- **Side Effects:** Persists action definitions through shared configuration and action adapters; updates `ActionCatalogState` after successful saves or deletes; must remain aligned with the shared persisted configuration model rather than inventing app-only action storage.
