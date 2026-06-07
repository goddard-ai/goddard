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

- Any new user-facing capability added in `app/` that depends on shared data loading, shared data mutation, or system configuration must also be implemented in `core/sdk/` in the same PR.
- UI-only behavior does not require `core/sdk/` parity. Do not replicate presentation-only features, interaction affordances, or UI configuration in the SDK.
- Do not ship `app/` ahead of `core/sdk/` when the feature depends on shared data or system configuration. If `core/sdk/` cannot support that behavior yet, treat the `app/` work as incomplete.
- When adding new UI components or interactive elements in `app/`, use the local `pandark-ui` skill to align Panda CSS and Ark UI composition with the existing design system.
- New cross-layer product capabilities should normally start as internal feature packages under `features/<name>`. Use `bun run scaffold:feature` for a consistent package shape, then wire selected entrypoints into the public composition roots.
- Feature packages must import thin plugin support packages such as `@goddard-ai/sdk-plugin` and `@goddard-ai/daemon-plugin` instead of the public packages that bundle them.
- Feature packages self-declare their schemas from their own `schema` entrypoint. Do not add feature-owned schemas to `@goddard-ai/schema`; that package is reserved for core daemon/backend/shared substrate schemas.
- Daemon feature packages that own persistence declare their kindstore schema through the daemon plugin `db` option and use inferred setup `context.db`; do not import the core daemon persistence singleton for feature-owned tables.
- `spec/` is the canonical source of product behavior and intent.
- Do not edit `spec/` unless explicitly asked.
- Do not knowingly let code, docs, or tests diverge from `spec/`.
- If a request conflicts with `spec/`, call it out instead of silently working around it.

## Default Values

- Treat behavior-affecting defaults as resolution-boundary logic, not local convenience.
- Default config, persisted data, SDK inputs, and domain values only in named resolver, factory, normalization, or config-loading modules.
- Once a value crosses into a resolved type, do not default it again downstream.
- UI-only presentation fallbacks are allowed when they do not affect shared behavior, persistence, SDK behavior, or system configuration.

## Code Style And Patch Discipline

- Make the smallest responsible change that fixes the current problem. Preserve existing architecture, naming, and file layout unless changing them is the simplest way to make the fix correct, readable, and maintainable.
- Prefer local, concrete, private implementation over exported, configurable, or abstract structure. Follow the existing local pattern with the lowest architectural impact.
- Counteract overengineering bias: do not add new public exports, optional parameters, config flags, interfaces, base classes, generic utility modules, lifecycle hooks, plugin systems, endpoint variants, or future-proof abstractions unless the current change requires them.
- Do not add code before it is needed. Avoid speculative helpers, abstractions, options, extension points, or alternate code paths.
- Do not export a symbol unless another module imports it or a framework/tooling contract requires it. Keep single-module implementation details private.
- Prefer readability and local reasoning over new abstractions. Avoid extracting local helpers that are only used once unless the helper produces a clear readability win or isolates a meaningful constraint.
- Name helper functions by the operation or predicate they represent. Prefer clear verbs or predicates such as `find`, `build`, `resolve`, `parse`, `create`, `is`, `has`, or `get`; avoid a `From` infix unless needed to distinguish two otherwise identical names.
- Prefer feature-local modules with sharp names and focused ownership over broad catch-all files. Split helpers by concern only when the current change would otherwise make a module mix unrelated filtering, type guards, search, formatting, data shaping, or actions.
- Use broad filenames like `presentation.ts` only when the contents are truly a small, cohesive presentation layer. Otherwise choose narrower names such as `filters.ts`, `entity-kind.ts`, `search.ts`, or `format.ts`.
- Avoid explicit function return types unless they are needed to preserve a public contract, constrain inference, clarify recursion, or prevent an unsafe or unclear inferred type.
- Add human-readable `/** ... */` description comments for exported modules, TypeScript type aliases and interfaces except types inferred from a same-name Zod schema, and top-level functions whose purpose, constraints, or domain role are not obvious from their name and body.
- Comments should explain non-obvious correctness constraints, invariants, external/platform workarounds, or edge-case failure modes. Do not add `@param` or `@returns` boilerplate, narrate the code, restate names/types, or compensate for code that should instead be renamed or simplified.
- For patches, add short comments only when non-obvious code structure, such as control flow, helper state, retry logic, synchronization, or error handling, is needed to handle a specific constraint. Name the race, invariant, platform behavior, ordering requirement, or failure mode, especially if observed in production/CI or covered by a test, but do not overfit to one test or speculate beyond the evidence.
- Maintain nearby comments when editing logic. If a change makes an existing comment stale, update or remove the comment in the same patch.
- Minimize churn: touch as few files as possible, avoid unrelated cleanup or formatting, and do not rename or move files unless necessary.
- If refactoring is required for correctness, keep it mechanical and separate from behavior changes when possible.
- Fix the smallest responsible class of bug rather than overfitting to the exact failing example.
- After implementation, perform a YAGNI pass and remove unnecessary configurability, exposure, abstraction, files, functions, classes, or unused code.
- Prefer rules in this order: correctness, compatibility with existing behavior, local consistency, minimal public surface, minimal churn.

## Git

- Use Conventional Commits: `<type>(optional-scope): <description>`.
- Commit requested changes without waiting for a separate prompt to commit them.
- Keep commits atomic, single-purpose, concise, and imperative.
- Split docs-only or policy-only changes from behavior or test changes unless they are inseparable.
- Include a body in every commit that explains why the change exists, including important context, tradeoffs, risks, migration notes, or links when useful.
- In non-interactive terminals, set `GIT_EDITOR=true` for commands that would otherwise open an editor.

## Testing

- Use temporary tests freely to reproduce, debug, or confirm a fix during development, but delete them before commit unless intentionally promoted.
- Before committing, classify every new test as temporary verification or permanent regression coverage.
- Keep permanent regression tests only when they protect durable behavior that could realistically regress, cover an important edge case, or document expected behavior not already covered.
- When in doubt, do not commit the test; extract only the smallest stable regression assertion worth preserving.
- When running the full workspace test suite from the repository root, use `bun run test`.
- Do not use `bun test` at the repository root; it bypasses the workspace package test scripts and monorepo orchestration.
- Do not test feature-package plugin seams directly. If feature logic is complex or critical enough to test, put it in a clearly named module outside the plugin entrypoint and test that module directly.

## Documentation Routing

- Whenever implementing a new user-visible feature, add one or more entries for it to `.git/undocumented-features.yaml`.
- Read the nearest `glossary.md` before changing domain behavior, naming, states, roles, identifiers, or ownership rules in a package that has one.
- Put package boundaries and integration surfaces in the nearest `README.md`.
- Put domain terminology in the nearest `glossary.md`.
- Put long-form contribution guidance that does not belong in `AGENTS.md` in `goddard-contributor`.
- Use repository-relative paths when printing workspace paths.
- Keep each `AGENTS.md` short, scannable, and scoped to its directory tree.
- Do not use `AGENTS.md` as a spec, plan, backlog, or changelog.
- When guidance outgrows an `AGENTS.md`, move it to a better-scoped document and leave a short pointer.
