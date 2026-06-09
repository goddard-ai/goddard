<workspace-agent-notes>
* These rules apply repo-wide unless a deeper `AGENTS.md` adds or narrows rules for its subtree.
* Before editing a subtree, read the root and nearest local `AGENTS.md`.
* Keep `AGENTS.md` short, scannable, and scoped to durable, easy-to-miss instructions.
* Put long-form contribution guidance in `goddard-contributor`.
* Update `AGENTS.md` when a human gives durable guidance future agents should follow.
</workspace-agent-notes>

<repository-state>
* This repo is unreleased and pre-alpha.
* Backwards compatibility is not required.
* Prefer the simplest forward-looking design.
* Do not add legacy paths, deprecation shims, or fallback behavior unless explicitly asked.
</repository-state>

<product-contracts>
* `spec/` is the canonical source of product behavior and intent.
* Do not edit `spec/` unless explicitly asked.
* Do not knowingly let code, docs, or tests diverge from `spec/`.
* If a request conflicts with `spec/`, call it out instead of silently working around it.
* New user-facing capability in `app/` that depends on shared data loading, shared data mutation, or system configuration must also be implemented in `core/sdk/` in the same PR.
* UI-only behavior does not require `core/sdk/` parity.
* Do not ship `app/` ahead of `core/sdk/` when shared data or system configuration behavior is involved.
</product-contracts>

<features>
* New cross-layer product capabilities should normally start as internal feature packages under `features/<name>`.
* Use `bun run scaffold:feature` for the standard feature package shape.
* Wire only selected feature entrypoints into public composition roots.
* Feature packages must import thin plugin support packages such as `@goddard-ai/sdk-plugin` and `@goddard-ai/daemon-plugin`, not public packages that bundle them.
* Feature packages self-declare schemas from their own `schema` entrypoint.
* Do not add feature-owned schemas to `@goddard-ai/schema`; reserve it for core daemon/backend/shared substrate schemas.
* Daemon feature packages that own persistence declare their kindstore schema through the daemon plugin `db` option and use inferred setup `context.db`.
* Do not import the core daemon persistence singleton for feature-owned tables.
</features>

<defaults>
* Treat behavior-affecting defaults as resolution-boundary logic, not local convenience.
* Put default config, persisted data, SDK inputs, and domain values only in named resolver, factory, normalization, or config-loading modules.
* Once a value crosses into a resolved type, do not default it again downstream.
* UI-only presentation fallbacks are allowed when they do not affect shared behavior, persistence, SDK behavior, or system configuration.
</defaults>

<code-style>
* Make the smallest responsible change that fixes the current problem.
* Preserve existing architecture, naming, and file layout unless changing them is the simplest correct path.
* Prefer local, concrete, private implementation over exported, configurable, or abstract structure.
* Do not add speculative helpers, abstractions, options, extension points, alternate code paths, or future-proofing.
* Do not add public exports, optional parameters, config flags, interfaces, base classes, generic utility modules, lifecycle hooks, plugin systems, or endpoint variants unless the current change requires them.
* Do not export a symbol unless another module imports it or a framework/tooling contract requires it.
* Prefer readability and local reasoning over new abstractions.
* Extract one-use helpers only when they clearly improve readability or isolate a meaningful constraint.
* Name helpers by their operation or predicate, using clear verbs such as `find`, `build`, `resolve`, `parse`, `create`, `is`, `has`, or `get`.
* Avoid a `From` infix unless needed to distinguish otherwise identical names.
* Prefer feature-local modules with sharp names and focused ownership over broad catch-all files.
* Split helpers by concern only when the current change would otherwise mix unrelated filtering, type guards, search, formatting, data shaping, or actions.
* Use broad filenames like `presentation.ts` only for small, cohesive presentation layers; otherwise choose narrower names such as `filters.ts`, `entity-kind.ts`, `search.ts`, or `format.ts`.
* Avoid explicit function return types unless needed to preserve a public contract, constrain inference, clarify recursion, or prevent unsafe or unclear inference.
* When adding UI components or interactive elements in `app/`, use the local `pandark-ui` skill to align Panda CSS and Ark UI composition with the existing design system.
</code-style>

<comments>
* Add human-readable `/** ... */` description comments for exported modules, TypeScript type aliases and interfaces, except types inferred from a same-name Zod schema.
* Comment top-level functions only when their purpose, constraints, or domain role are not obvious from their name and body.
* Comments should explain non-obvious correctness constraints, invariants, external/platform workarounds, or edge-case failure modes.
* Do not add `@param` or `@returns` boilerplate, narrate the code, restate names/types, or compensate for unclear code.
* For patches, add short comments only when non-obvious control flow, helper state, retry logic, synchronization, or error handling handles a specific constraint.
* Name the race, invariant, platform behavior, ordering requirement, or failure mode when it matters.
* Maintain nearby comments when editing logic; update or remove stale comments in the same patch.
</comments>

<patch-discipline>
* Minimize churn: touch as few files as possible and avoid unrelated cleanup or formatting.
* Do not rename or move files unless necessary.
* If refactoring is required for correctness, keep it mechanical and separate from behavior changes when possible.
* Fix the smallest responsible class of bug rather than overfitting to the exact failing example.
* After implementation, remove unnecessary configurability, exposure, abstraction, files, functions, classes, and unused code.
* Prefer rules in this order: correctness, compatibility with existing behavior, local consistency, minimal public surface, minimal churn.
</patch-discipline>

<git>
* Use Conventional Commits: `<type>(optional-scope): <description>`.
* Commit requested changes without waiting for a separate prompt.
* Keep commits atomic, single-purpose, concise, and imperative.
* Split docs-only or policy-only changes from behavior or test changes unless inseparable.
* Include a body in every commit explaining why the change exists, with important context, tradeoffs, risks, migration notes, or links when useful.
* In non-interactive terminals, set `GIT_EDITOR=true` for commands that would otherwise open an editor.
</git>

<testing>
* Use temporary tests freely to reproduce, debug, or confirm a fix, but delete them before commit unless intentionally promoted.
* Before committing, classify every new test as temporary verification or permanent regression coverage.
* Keep permanent regression tests only when they protect durable behavior that could realistically regress, cover an important edge case, or document expected behavior not already covered.
* When in doubt, do not commit the test; extract only the smallest stable regression assertion worth preserving.
* From the repository root, run the full workspace test suite with `bun run test`.
* Do not use `bun test` at the repository root; it bypasses workspace package test scripts and monorepo orchestration.
* Do not test feature-package plugin seams directly. Move complex or critical feature logic outside the plugin entrypoint and test that module directly.
</testing>

<documentation-routing>
* Whenever implementing a new user-visible feature, add one or more entries for it to `.git/undocumented-features.yaml`.
* Read the nearest `glossary.md` before changing domain behavior, naming, states, roles, identifiers, or ownership rules in a package that has one.
* Put package boundaries and integration surfaces in the nearest `README.md`.
* Put domain terminology in the nearest `glossary.md`.
* Put long-form contribution guidance in `goddard-contributor`.
* Use repository-relative paths when printing workspace paths.
* Do not use `AGENTS.md` as a spec, plan, backlog, or changelog.
* When guidance outgrows an `AGENTS.md`, move it to a better-scoped document and leave a short pointer.
</documentation-routing>