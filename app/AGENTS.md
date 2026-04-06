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
- Reuse shared SDK, daemon, schema, and config contracts instead of inventing app-only payloads or storage models.
- In `app/src`, keep feature components and their sigma state modules together inside feature folders. Do not add barrel modules there, and do not create `state/` subfolders.
- In `app/src`, use all-lowercase kebab-case folder names for UI feature trees.
- In `app/src`, use all-lowercase kebab-case component filenames and avoid repeating the parent feature name in child component names.
- In `app/src`, use `~/` imports for cross-folder modules (resolves to `app/src`) and reserve relative imports for same-folder `./...` only.
- In UI components, prefer `useListener` from `preact-sigma` over manual `addEventListener` and `removeEventListener` wiring.
- Prefer the `class` JSX prop over `className`.
- Prefer app nouns that match `app/glossary.md`. Use `project` for user-added local roots unless a feature specifically requires a git repository.
- Never destructure component props. Define component prop types inline instead of creating `Props` aliases or interfaces.
- Do not add automated tests for UI components unless explicitly asked.
- Run formatting after modifying app files.
- When a human asks for a new task, commit any app work from the previous task before starting. If that work is unfinished, include `Next step: ...` in the commit message body.
- Keep this file short, scannable, and app-local. Move longer explanations to `goddard-contributor`.
