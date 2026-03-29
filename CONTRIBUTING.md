# Contributing

- These rules apply repository-wide unless a deeper `CONTRIBUTING.md` adds or narrows rules for its subtree.

## Repository State

- This repository is unreleased and pre-alpha.
- Backwards compatibility is not a requirement.
- Prefer the simplest forward-looking design.
- Do not add legacy compatibility paths, deprecation shims, or fallback behavior unless a human explicitly asks for them.

## Shared Behavior Rules

- Any new user-facing capability added in `app/` must also be implemented in `core/sdk/` in the same PR.
- Do not leave `app/` ahead of `core/sdk/` for shipped behavior.
- If a capability cannot yet be supported in `core/sdk/`, treat the `app/` work as incomplete.
- `spec/` is the canonical source of product behavior and intent.
- Do not edit `spec/` unless a human explicitly asks you to.
- Do not knowingly make code, docs, or tests diverge from `spec/`.
- If a requested change appears inconsistent with `spec/`, call that out instead of silently implementing around it.

## Documentation

- Read a package `glossary.md` before changing domain behavior, naming, states, roles, identifiers, or ownership rules in a package that has one.
- Read a sibling concept doc before editing its adjacent implementation file or another file that depends on the same local model.
- Update the relevant concept doc in the same change when you add, remove, rename, or change the meaning of a domain concept.
- Add or expand a package glossary or sibling concept doc when recurring local abstractions are slow to recover from code comments, types, or signatures alone.
- Keep concept docs concise and focused on domain-level what and why.
- Do not turn concept docs into implementation walkthroughs, code tours, API references, or change logs.

## Code Style

- Inline values instead of introducing single-use variables unless the variable materially improves readability or avoids repeating a complex expression.
- Every exported module declaration must have a human-readable `/** ... */` description comment.
- Every TypeScript type alias and interface, whether exported or internal, must have a human-readable `/** ... */` description comment, except when the type is inferred from a same-name Zod schema.
- Every non-trivial top-level function must have a human-readable `/** ... */` description comment.
- These comments should explain the non-obvious what and, when useful, the why.
- Strange coding patterns must have a brief `//` comment stating what they are doing and why they are necessary.
- Do not add JSDoc tag boilerplate such as `@param` or `@returns`.
- Do not document the obvious or describe line-by-line implementation mechanics.

## Git

- Follow Conventional Commits with the format `<type>(optional-scope): <description>`.
- Keep commit subjects concise and imperative.
- Prefer atomic commits with one clear purpose.
- Split docs-only or policy-only changes from behavior or test changes unless they are inseparable.
- Every commit must include a body with clear bullet points explaining what changed and why.
- In non-interactive terminals, use `GIT_EDITOR=true git rebase --continue`.

## Testing

- Keep the rest of the test suite lean and intentional.
- Do not use repository-local Vitest mocking or stubbing APIs such as `vi.mock`, `vi.doMock`, `vi.hoisted`, `vi.fn`, `vi.spyOn`, `vi.mocked`, `vi.stubGlobal`, `vi.stubEnv`, `vi.unstubAllGlobals`, or `vi.unstubAllEnvs`, or similar helper methods such as `mockImplementation`, `mockResolvedValue`, or `mockReturnValue`, except at explicit non-local third-party integration boundaries.
- Treat first-party packages, local modules, Node stdlib seams, prompt libraries, Tauri host APIs, `console`, `process`, and local daemon or client wrappers as non-exception cases.
- Prefer real temp directories, temp `HOME`, copied fixtures, real git repositories, real worktrees, real daemon servers, subprocess-based CLI tests, and real ACP fixture processes over fake layers.
- Remove tests that only prove one first-party wrapper calls another unless they protect a meaningful user-visible contract not covered elsewhere.
- Use `expect` rather than `assert` in Vitest files.
- For daemon logging tests, capture logs through explicit seams such as `configureDaemonLogging({ writeLine })` instead of spying on stdout.
- For CLI tests, capture real subprocess output instead of spying on `console` or `process`.
- Prefer stable, contract-level assertions over incidental wording-heavy output checks.
