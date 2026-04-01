# Workspace Agent Notes

- These rules apply repo-wide unless a deeper `AGENTS.md` adds or narrows rules for its subtree.
- Use `AGENTS.md` for short, easy-to-miss instructions. Put long-form contribution or implementation guidance in `goddard-contributor`.
- Before editing a subtree, read the root and nearest local `AGENTS.md`.
- Update `AGENTS.md` when a human gives durable guidance future agents should follow.

## Repository State

- This repo is unreleased and pre-alpha.
- Backwards compatibility is not required.
- Prefer the simplest forward-looking design.
- Do not add legacy compatibility paths, deprecation shims, or fallback behavior unless explicitly asked.

## Shared Behavior Rules

- Any new user-facing capability added in `app/` must also be implemented in `core/sdk/` in the same PR.
- Do not ship `app/` ahead of `core/sdk/`. If `core/sdk/` cannot support it yet, treat the `app/` work as incomplete.
- `spec/` is the canonical source of product behavior and intent.
- Do not edit `spec/` unless explicitly asked.
- Do not knowingly let code, docs, or tests diverge from `spec/`.
- If a request conflicts with `spec/`, call it out instead of silently working around it.

## Code Style And Patch Discipline

- Make the smallest correct change. Preserve existing architecture, naming, and file layout unless the task requires refactoring.
- Prefer readability and local reasoning over new abstractions.
- Add human-readable `/** ... */` description comments to:
  - exported modules
  - non-trivial top-level functions
  - TypeScript type aliases and interfaces, except types inferred from a same-name Zod schema
- Comments should explain the non-obvious what or why. Do not add `@param` or `@returns` boilerplate, and do not restate the code.
- Minimize churn: touch as few files as possible, avoid unrelated cleanup or formatting, and do not rename or move files unless necessary.
- If refactoring is required for correctness, keep it mechanical and separate from behavior changes when possible.
- When uncertain, follow the existing local pattern with the lowest architectural impact.

## Git

- Use Conventional Commits: `<type>(optional-scope): <description>`.
- Keep commits atomic, single-purpose, concise, and imperative.
- Split docs-only or policy-only changes from behavior or test changes unless they are inseparable.
- Include a body in every commit with brief bullets describing what changed and why.
- In non-interactive terminals, set `GIT_EDITOR=true` for commands that would otherwise open an editor.

## Documentation Routing

- Read the nearest `glossary.md` before changing domain behavior, naming, states, roles, identifiers, or ownership rules in a package that has one.
- Put package boundaries and integration surfaces in the nearest `README.md`.
- Put domain terminology in the nearest `glossary.md`.
- Put long-form contribution guidance that does not belong in `AGENTS.md` in `goddard-contributor`.
- Use repository-relative paths when printing workspace paths.
- Keep each `AGENTS.md` short, scannable, and scoped to its directory tree.
- Do not use `AGENTS.md` as a spec, plan, backlog, or changelog.
- When guidance outgrows an `AGENTS.md`, move it to a better-scoped document and leave a short pointer.