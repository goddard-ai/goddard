Use this reference for app-local testing guidance that intentionally does not live in `app/AGENTS.md`.

- For runtime UI, Bun host, or full-stack app behavior changes, treat manual QA through `bun run dev` from the workspace root and the flow in `app/README.md` as the default verification path.
- Reserve automated tests in `app/` for extracted pure logic or fragile user-visible contracts that are hard to validate manually and easy to regress, such as deterministic transforms, ordering or merge rules, serialization boundaries, theme derivation, and helpers with meaningful edge cases.
- When app tests are warranted, keep them small and assert observable outputs rather than component internals.
- Do not add routine tests for rendered UI, styling, layout, simple RPC wiring, or fast-moving UX flows unless explicitly asked.
