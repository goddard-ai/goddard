# App Agent Notes

- These rules apply to work in `app/` unless a deeper `AGENTS.md` narrows them.
- `app/AGENTS.md` is the bootstrap doc for app-local hard rules.
- Load `goddard-contributor` for app-local guidance that does not belong in `app/AGENTS.md`.
- Before editing files in `app/`, read `app/AGENTS.md`.
- Read `app/glossary.md` before naming or changing app-local concepts, states, or user-facing nouns.
- You may update this file when a human gives durable feedback that future agents working in `app/` should follow.

## App Rules

- Treat `app/` as an Electrobun desktop app with a Bun-owned host layer and a frontend-heavy TypeScript webview.
- Put desktop integrations behind the Electrobun RPC bridge instead of importing host APIs directly into UI code. UI components should render props and invoke actions, not call host APIs.
- Keep complex shared state, persistence, and IPC in `preact-sigma` modules rather than components.
- Use `useSignal()` or local component state for simple UI state such as open flags, drafts, and ephemeral form status. Do not model that kind of UI state in `preact-sigma`.
- Keep custom Preact hooks for state management local to the component that uses them. Do not extract single-use state hooks into shared modules.
- Use the local query cache in `src/lib/query.ts` for shared SDK or daemon-backed reads. Query functions passed to `useQuery()` should be stable references, not inline per-render closures. Call SDK writes directly, then manually invalidate affected queries with `useQueryClient()`. Do not add optimistic UI or loading indicators for local form submissions.
- Reuse shared SDK, daemon, schema, and config contracts instead of inventing app-only payloads or storage models.
- Within `src/`:
  - Keep feature components and their sigma state modules together inside feature folders. Do not add barrel modules there, and do not create `state/` subfolders.
  - Use all-lowercase kebab-case folder names for UI feature trees.
  - Use all-lowercase kebab-case component filenames and avoid repeating the parent feature name in child component names.
  - Do not use bare generic component names like `List`, `View`, `Page`, or `Dialog`; include feature-specific context in exported component names.
  - Import-path precedence is `./...`, then `~/...`, then `../...`.
  - Use explicit TypeScript source extensions on those imports. Prefer `.ts` for `.ts` modules and `.tsx` for `.tsx` modules, including `~/...` imports.
  - Use `./...` for same-folder modules first.
  - Use `~/...` for imports that would otherwise traverse up to `src/` or across feature roots.
  - Use `../...` only when it does not traverse up to `src/` itself. A single `../...` is allowed when it still lands inside a child path such as `src/foo/...`, but do not use `../...` to reach `src/...` broadly.
  - Never use `../../...` or deeper upward traversal imports.
- In UI components, prefer `useListener` from `preact-sigma` over manual `addEventListener` and `removeEventListener` wiring.
- Prefer the `class` JSX prop over `className`.
- Prefix reusable, pre-styled UI primitives with `Good` (for example `GoodTooltip`). Reserve that prefix for opinionated design-system components, not feature/domain modules or state.
- Prefer app nouns that match `app/glossary.md`. Use `project` for user-added local roots unless a feature specifically requires a git repository.
- Never destructure component props. Define component prop types inline instead of creating `Props` aliases or interfaces.
- Do not add automated tests for UI components unless explicitly asked.
- Run formatting after modifying app files.
- When a human asks for a new task, commit any app work from the previous task before starting. If that work is unfinished, include `Next step: ...` in the commit message body.
- Do not add module-level first-line description comments or routine component description comments unless a comment explains non-obvious behavior that the code itself does not make clear.
- Keep this file short, scannable, and app-local. Move longer explanations to `goddard-contributor`.
