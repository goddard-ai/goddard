# Testing

Read this ruleset when adding, changing, reviewing, or deciding whether to add tests, and when verifying behavior changes.

- Delete temporary verification tests before commit; keep only stable regression coverage for durable behavior or important edge cases.
- Add or update tests when behavior changes, unless a deeper `AGENTS.md` narrows that subtree.
- When fixing a bug, add a regression test that reproduces the issue when practical.
- Test observable contracts through public interfaces; prefer behavior visible to users, callers, processes, or consumers over internal structure.
- Exercise the executable path being covered; prefer invoking runtime entry points over inspecting static artifacts.
- Assert outcomes, not implementation evidence, and preserve refactor freedom.
- Mock only at external boundaries to control nondeterminism, cost, or unavailable systems.
- Do not use repository-local `bun:test` mocking or stubbing APIs such as `vi.mock`, `vi.doMock`, `vi.hoisted`, `vi.fn`, `vi.spyOn`, `vi.mocked`, `vi.stubGlobal`, `vi.stubEnv`, `vi.unstubAllGlobals`, or `vi.unstubAllEnvs`, or similar helper methods such as `mockImplementation`, `mockResolvedValue`, or `mockReturnValue`, except at explicit non-local third-party integration boundaries.
- Treat first-party packages, local modules, Node stdlib seams, prompt libraries, Tauri host APIs, `console`, `process`, and local daemon or client wrappers as non-exception cases.
- Prefer real temp directories, temp `HOME`, copied fixtures, real git repositories, real worktrees, real daemon servers, subprocess-based CLI tests, and real ACP fixture processes over fake layers.
- Use snapshots only when serialized output is the contract, or pair them with explicit behavior assertions.
- Remove tests that only prove one first-party wrapper calls another unless they protect a meaningful user-visible contract not covered elsewhere.
- Use `expect` rather than `assert` in `bun:test` files.
- For daemon logging tests, capture logs through explicit seams such as `configureDaemonLogging({ writeLine })` instead of spying on stdout.
- For CLI tests, capture real subprocess output instead of spying on `console` or `process`.
- Prefer stable, contract-level assertions over incidental wording-heavy output checks.
- Kindstore migrations do not require dedicated regression tests.
- Do not test feature-package plugin seams directly. Move complex or critical feature logic outside the plugin entrypoint and test that module directly.
- When practical test infrastructure is missing, document the limitation and give concrete manual verification steps.
- From the repository root, run the full workspace test suite with `pnpm run test`.
- Do not use `bun test` at the repository root; it bypasses workspace package test scripts and monorepo orchestration.

## App Testing

- For runtime UI, Bun host, or full-stack app behavior changes, treat manual QA through `bun run dev` from the workspace root and the flow in `app/README.md` as the default verification path.
- Reserve automated tests in `app/` for extracted pure logic or fragile user-visible contracts that are hard to validate manually and easy to regress, such as deterministic transforms, ordering or merge rules, serialization boundaries, theme derivation, and helpers with meaningful edge cases.
- When app tests are warranted, keep them small and assert observable outputs rather than component internals.
- Do not add routine tests for rendered UI, styling, layout, simple RPC wiring, or fast-moving UX flows unless explicitly asked.
