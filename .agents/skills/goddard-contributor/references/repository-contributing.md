# Repository Contributing

Use this reference for repository-wide contribution guidance that intentionally does not live in `AGENTS.md`.

For bootstrap rules such as repository state, shared behavior, patch discipline, and git, follow the root `AGENTS.md` first.

## Documentation Policy

- Read the relevant `glossary.md` before changing domain behavior, naming, states, roles, identifiers, or ownership rules.
- Read a sibling concept doc before editing its adjacent implementation file or another file that depends on the same local model.
- Update the relevant concept doc in the same change when you add, remove, rename, or change the meaning of a domain concept.
- Add or expand the relevant package glossary or sibling concept doc when recurring local abstractions are slow to recover from code comments, types, or signatures alone.
- Keep concept docs concise and focused on domain-level what and why.
- Do not turn concept docs into implementation walkthroughs, code tours, API references, or change logs.

## Expanded Code Style

- Inline values instead of introducing single-use variables unless the variable materially improves readability or avoids repeating a complex expression.
- Strange coding patterns must have a brief `//` comment stating what they are doing and why they are necessary.
- Do not document the obvious or describe line-by-line implementation mechanics.

## Dependency Policy

- Do not add dependencies lightly. Prefer existing platform APIs, workspace packages, and project utilities.
- If a new dependency is truly warranted, choose the smallest one that fits the repository style and explain why it is needed.

## Testing Policy

- Before adding or changing tests, consult this section through `goddard-contributor`.
- Add or update tests when behavior changes, unless a deeper `AGENTS.md` narrows that subtree.
- Prefer small tests around observable behavior. Do not rewrite tests solely to match refactors or introduce a large new testing pattern in a narrow area.
- Keep the rest of the test suite lean and intentional.
- Prefer a brief local code comment over a new test when the goal is to preserve a non-obvious implementation detail, platform workaround, or other Chesterton's Fence-style invariant that is already adequately covered by existing observable behavior.
- Do not use repository-local `bun:test` mocking or stubbing APIs such as `vi.mock`, `vi.doMock`, `vi.hoisted`, `vi.fn`, `vi.spyOn`, `vi.mocked`, `vi.stubGlobal`, `vi.stubEnv`, `vi.unstubAllGlobals`, or `vi.unstubAllEnvs`, or similar helper methods such as `mockImplementation`, `mockResolvedValue`, or `mockReturnValue`, except at explicit non-local third-party integration boundaries.
- Treat first-party packages, local modules, Node stdlib seams, prompt libraries, Tauri host APIs, `console`, `process`, and local daemon or client wrappers as non-exception cases.
- Prefer real temp directories, temp `HOME`, copied fixtures, real git repositories, real worktrees, real daemon servers, subprocess-based CLI tests, and real ACP fixture processes over fake layers.
- Remove tests that only prove one first-party wrapper calls another unless they protect a meaningful user-visible contract not covered elsewhere.
- Remove tests that only lock down basic first-party behavior, implementation choices, or wrapper-to-wrapper plumbing that a local comment can explain well enough.
- Use `expect` rather than `assert` in `bun:test` files.
- For daemon logging tests, capture logs through explicit seams such as `configureDaemonLogging({ writeLine })` instead of spying on stdout.
- For CLI tests, capture real subprocess output instead of spying on `console` or `process`.
- Prefer stable, contract-level assertions over incidental wording-heavy output checks.
