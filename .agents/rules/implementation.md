# Implementation

Read this ruleset when editing production code, refactoring, changing architecture, adding abstractions or exports, or changing dependencies.

- This repo is unreleased and pre-alpha.
- Backwards compatibility is not required.
- Prefer the simplest forward-looking design.
- Do not add legacy paths, deprecation shims, or fallback behavior unless explicitly asked.
- Make the smallest responsible change that fixes the current problem.
- Preserve existing architecture, naming, and file layout unless changing them is the simplest correct path.
- Prefer local, concrete, private implementation over exported, configurable, or abstract structure.
- Do not add speculative helpers, abstractions, options, extension points, alternate code paths, or future-proofing.
- Do not add public exports, optional parameters, config flags, interfaces, base classes, generic utility modules, lifecycle hooks, plugin systems, or endpoint variants unless the current change requires them.
- Do not export a symbol unless another module imports it or a framework/tooling contract requires it.
- Extract one-use helpers only when they clearly improve readability or isolate a meaningful constraint.
- Prefer focused module names over broad catch-all files; follow nearby naming patterns.
- Avoid explicit return types unless they improve safety or clarity.
- Document exported APIs when their purpose, constraints, or domain role are not obvious.
- Add short comments only when non-obvious control flow, helper state, retry logic, synchronization, or error handling handles a specific constraint.
- Name the race, invariant, platform behavior, ordering requirement, or failure mode when it matters.
- Minimize churn: touch as few files as possible and avoid unrelated cleanup, formatting, moves, or renames.
- If refactoring is required for correctness, keep it mechanical and separate from behavior changes when possible.
- Fix the smallest responsible class of bug rather than overfitting to the exact failing example.
- After implementation, remove unnecessary configurability, exposure, abstraction, files, functions, classes, and unused code.
- Prefer rules in this order: correctness, compatibility with existing behavior, local consistency, minimal public surface, minimal churn.
- Avoid new dependencies unless clearly justified.
- Prefer platform APIs, workspace packages, and existing project utilities.
- If a new dependency is necessary, choose the smallest option that fits the repo's conventions and explain why.
