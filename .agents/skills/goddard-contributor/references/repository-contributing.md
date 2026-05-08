# Repository Contributing

Use this reference for repository-wide contribution guidance that intentionally does not live in `AGENTS.md`.

## Documentation Policy

- Read the relevant `glossary.md` before changing domain behavior, naming, states, roles, identifiers, or ownership rules.
- Read a sibling concept doc before editing its adjacent implementation file or another file that depends on the same local model.
- Update the relevant concept doc in the same change when you add, remove, rename, or change the meaning of a domain concept.
- Add or expand the relevant package glossary or sibling concept doc when recurring local abstractions are slow to recover from code comments, types, or signatures alone.
- Keep concept docs concise and focused on domain-level what and why.
- Do not turn concept docs into implementation walkthroughs, code tours, API references, or change logs.

## Expanded Code Style

- Good reviewability comments usually explain hidden invariants, intentional asymmetry, protocol or wire-format constraints, external tool workarounds, or edge-case handling whose failure mode is not locally obvious.
- Prefer clearer names or structure over comments when the issue is ordinary readability.
- Treat minimal design as an active review step, not just an initial intent. Before finishing a change, look for anything added for a hypothetical future caller or variant.
- Collapse any abstraction that does not serve the current requirement. Inline local helpers used once unless the helper makes non-obvious logic substantially clearer.
- Remove configurability that is not currently needed. Hardcode the current behavior when no caller or product requirement needs a choice yet.
- Keep implementation details private until another module in the same change needs them. Do not export symbols preemptively for imagined reuse.
- Prefer changing an existing focused file over creating a new file. Create a new file only when the existing file would mix distinct concerns or become harder to reason about.

## Default Value Policy

- Prefer separate raw and resolved shapes, such as `RawConfig` with optional fields and `ResolvedConfig` with required fields.
- Keep default constants close to the resolver that owns them, and name them for the specific behavior they control.
- Represent intentional absence with an optional field or explicit union instead of encoding it as a magic default value.
- Resolver tests should cover precedence across input sources and assert source metadata when provenance is kept for debugging.

## Dependency Policy

- Do not add dependencies lightly. Prefer existing platform APIs, workspace packages, and project utilities.
- If a new dependency is truly warranted, choose the smallest one that fits the repository style and explain why it is needed.

## Feature Package Policy

- Internal full-stack feature packages live under `features/<name>` and use the package name `@goddard-ai/<name>` without a `feature-` prefix.
- Start new feature packages with `bun run scaffold:feature`. In noninteractive agent workflows, use flags such as `--name`, `--layers daemon,sdk,app`, `--schema`, `--daemon-ipc`, `--styled-system`, `--skip-install`, or `--dry-run`.
- Scaffolded packages are inert until a public composition root imports one of their entrypoints. Do not register a feature in `core/sdk`, `core/daemon`, or `app` unless the task includes making that feature part of the supported product surface.
- Feature packages import plugin support packages and shared contract packages, not the public composition roots that bundle the feature. For example, import `@goddard-ai/sdk-plugin`, `@goddard-ai/daemon-plugin`, `@goddard-ai/app-plugin`, `@goddard-ai/ipc`, and shared schema packages instead of importing `@goddard-ai/sdk` from the feature package.
- App feature entrypoints must stay SDK-agnostic at the package level. Express SDK needs as type-level app plugin requirements or app composition metadata, and let the static app composition root provide the actual SDK instance.
- Daemon feature dependencies are explicit. A daemon plugin may expose a named `provides` map and list other daemon plugins in `consumes`; consumed feature extensions appear as direct `context.<feature>` fields in `setup(context)`. Do not introduce package-level cycles between feature packages.
- Shared daemon IPC contracts belong in `src/daemon-ipc.ts` and use `defineIpcSchema()` from `@goddard-ai/ipc`; public composition roots combine fragments with `composeIpcSchemas()`.
- `features/inbox` is the current reference package for a low-risk daemon + SDK + app feature. Inspect it before adding a new feature package with similar layers.

## Testing Policy

- Add or update tests when behavior changes, unless a deeper `AGENTS.md` narrows that subtree.
- Prefer small tests around observable behavior. Do not rewrite tests solely to match refactors or introduce a large new testing pattern in a narrow area.
- Keep the rest of the test suite lean and intentional.
- Do not use repository-local `bun:test` mocking or stubbing APIs such as `vi.mock`, `vi.doMock`, `vi.hoisted`, `vi.fn`, `vi.spyOn`, `vi.mocked`, `vi.stubGlobal`, `vi.stubEnv`, `vi.unstubAllGlobals`, or `vi.unstubAllEnvs`, or similar helper methods such as `mockImplementation`, `mockResolvedValue`, or `mockReturnValue`, except at explicit non-local third-party integration boundaries.
- Treat first-party packages, local modules, Node stdlib seams, prompt libraries, Tauri host APIs, `console`, `process`, and local daemon or client wrappers as non-exception cases.
- Prefer real temp directories, temp `HOME`, copied fixtures, real git repositories, real worktrees, real daemon servers, subprocess-based CLI tests, and real ACP fixture processes over fake layers.
- Remove tests that only prove one first-party wrapper calls another unless they protect a meaningful user-visible contract not covered elsewhere.
- Use `expect` rather than `assert` in `bun:test` files.
- For daemon logging tests, capture logs through explicit seams such as `configureDaemonLogging({ writeLine })` instead of spying on stdout.
- For CLI tests, capture real subprocess output instead of spying on `console` or `process`.
- Prefer stable, contract-level assertions over incidental wording-heavy output checks.
