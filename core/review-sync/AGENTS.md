# Review Sync Agent Notes

- These rules apply to `core/review-sync/`.
- Before changing review-sync behavior or docs, read `docs/README.md` and
  follow the links relevant to the change.
- Read `glossary.md` before changing domain terms, branch or worktree roles,
  states, identifiers, selection rules, or ownership boundaries.
- Treat `docs/` as the subsystem's public current-behavior documentation.
  - Read `docs/model.md` before changing roles, states, patch outcomes,
    guardrails, session selection, or ownership boundaries.
  - Read `docs/standard-review-workflow.md` before changing the normal
    review loop, watch behavior, pause/resume behavior, or recovery paths.
  - Read the matching `docs/commands/*.md` page before changing a command's
    externally visible behavior.
- Update `docs/` in the same change when you add, remove, rename, or change:
  - supported command behavior or outcomes
  - user-visible state or machine-readable output
  - branch, worktree, patch, pause, or session selection semantics
  - preconditions, guardrails, refusal cases, or read-only behavior
  - conflict, interruption, waiting, cleanup, or recovery behavior
  - human vs agent ownership boundaries
- Do not update `docs/` for implementation-only refactors unless supported
  behavior changes.
- Keep public docs durable and implementation-neutral:
  - Document what is supported and why it matters.
  - Do not document source layout, helper functions, private schemas, locks,
    exact diagnostics, parser details, storage mechanics, or internal execution
    order.
- When adding or removing a command page, keep `docs/README.md` as the
  scan-first map with a short audience and mutation summary.
- Put package boundaries and exported API details in `README.md`, domain terms
  in `glossary.md`, and conceptual behavior in `docs/`.
