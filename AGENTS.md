<workspace-agent-notes>
- These rules apply repo-wide unless a deeper `AGENTS.md` adds or narrows rules for its subtree.
- Before editing a subtree, read the root and nearest local `AGENTS.md`.
- Keep `AGENTS.md` short, scannable, and scoped to durable, easy-to-miss instructions.
- Put long-form contribution guidance in `goddard-contributor`.
- Update `AGENTS.md` when a human gives durable guidance future agents should follow.
</workspace-agent-notes>

<repository-state>
- This repo is unreleased and pre-alpha.
- Backwards compatibility is not required.
- Prefer the simplest forward-looking design.
- Do not add legacy paths, deprecation shims, or fallback behavior unless explicitly asked.
</repository-state>

<product-contracts>
- `spec/` is the canonical source of product behavior and intent.
- Do not edit `spec/` unless explicitly asked.
- Do not knowingly let code, docs, or tests diverge from `spec/`.
- If a request conflicts with `spec/`, call it out instead of silently working around it.
- New user-facing capability in `app/` that depends on shared data loading, shared data mutation, or system configuration must also be implemented in `core/sdk/` in the same PR.
- UI-only behavior does not require `core/sdk/` parity.
- Do not ship `app/` ahead of `core/sdk/` when shared data or system configuration behavior is involved.
</product-contracts>

<features>
- New cross-layer product capabilities should normally start as internal feature packages under `features/<name>`.
- Use `pnpm run scaffold:feature` for the standard feature package shape.
- Wire only selected feature entrypoints into public composition roots.
- Feature packages must import thin plugin support packages such as `@goddard-ai/sdk-plugin` and `@goddard-ai/daemon-plugin`, not public packages that bundle them.
- Feature packages self-declare schemas from their own `schema` entrypoint.
- Do not add feature-owned schemas to `@goddard-ai/schema`; reserve it for core daemon/backend/shared substrate schemas.
- Daemon feature packages that own persistence declare their kindstore schema through the daemon plugin `db` option and use inferred setup `context.db`.
- Do not import the core daemon persistence singleton for feature-owned tables.
</features>

<defaults>
- Treat behavior-affecting defaults as resolution-boundary logic, not local convenience.
- Put default config, persisted data, SDK inputs, and domain values only in named resolver, factory, normalization, or config-loading modules.
- Once a value crosses into a resolved type, do not default it again downstream.
- UI-only presentation fallbacks are allowed when they do not affect shared behavior, persistence, SDK behavior, or system configuration.
</defaults>

<implementation-discipline>
- Make the smallest responsible change that fixes the current problem.
- Preserve existing architecture, naming, and file layout unless changing them is the simplest correct path.
- Prefer local, concrete, private implementation over exported, configurable, or abstract structure.
- Do not add speculative helpers, abstractions, options, extension points, alternate code paths, or future-proofing.
- Do not add public exports, optional parameters, config flags, interfaces, base classes, generic utility modules, lifecycle hooks, plugin systems, or endpoint variants unless the current change requires them.
- Do not export a symbol unless another module imports it or a framework/tooling contract requires it.
- Extract one-use helpers only when they clearly improve readability or isolate a meaningful constraint.
- Prefer focused module names over broad catch-all files; follow nearby naming patterns.
- Avoid explicit return types unless they improve safety or clarity.
- When adding UI components or interactive elements in `app/`, use the local `pandark-ui` skill to align Panda CSS and Ark UI composition with the existing design system.
- Document exported APIs when their purpose, constraints, or domain role are not obvious.
- For patches, add short comments only when non-obvious control flow, helper state, retry logic, synchronization, or error handling handles a specific constraint.
- Name the race, invariant, platform behavior, ordering requirement, or failure mode when it matters.
- Minimize churn: touch as few files as possible and avoid unrelated cleanup, formatting, moves, or renames.
- If refactoring is required for correctness, keep it mechanical and separate from behavior changes when possible.
- Fix the smallest responsible class of bug rather than overfitting to the exact failing example.
- After implementation, remove unnecessary configurability, exposure, abstraction, files, functions, classes, and unused code.
- Prefer rules in this order: correctness, compatibility with existing behavior, local consistency, minimal public surface, minimal churn.
</implementation-discipline>

<git>
- Use Conventional Commits: `<type>(optional-scope): <description>`.
- Commit requested changes without waiting for a separate prompt.
- Split docs-only or policy-only changes from behavior or test changes unless inseparable.
- Include a commit body when the reason, tradeoffs, risks, or migration notes are not obvious from the subject.
- In non-interactive terminals, set `GIT_EDITOR=true` for commands that would otherwise open an editor.
</git>

<testing>
- Delete temporary verification tests before commit; keep only stable regression coverage for durable behavior or important edge cases.
- Test observable contracts through public interfaces; prefer behavior visible to users, callers, processes, or consumers over internal structure.
- Exercise the executable path being covered; prefer invoking runtime entry points over inspecting static artifacts.
- Assert outcomes, not implementation evidence, and preserve refactor freedom.
- Mock only at external boundaries to control nondeterminism, cost, or unavailable systems.
- Use snapshots only when serialized output is the contract, or pair them with explicit behavior assertions.
- When fixing a bug, add a regression test that reproduces the issue when practical.
- Kindstore migrations do not require dedicated regression tests.
- When practical test infrastructure is missing, document the limitation and give concrete manual verification steps.
- From the repository root, run the full workspace test suite with `pnpm run test`.
- Do not use `bun test` at the repository root; it bypasses workspace package test scripts and monorepo orchestration.
- Do not test feature-package plugin seams directly. Move complex or critical feature logic outside the plugin entrypoint and test that module directly.
</testing>

<documentation-routing>
- Whenever implementing a new user-visible feature, add one or more entries for it to `.git/undocumented-features.yaml`.
- Read the nearest `glossary.md` before changing domain behavior, naming, states, roles, identifiers, or ownership rules in a package that has one.
- Put package boundaries and integration surfaces in the nearest `README.md`.
- Put domain terminology in the nearest `glossary.md`.
- Do not use `AGENTS.md` as a spec, plan, backlog, or changelog.
- When guidance outgrows an `AGENTS.md`, move it to a better-scoped document and leave a short pointer.
</documentation-routing>
