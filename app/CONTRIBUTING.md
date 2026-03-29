# App Contributing

- Scope:
  - These rules apply to work in `app/`.

- Read before substantial app work:
  - Read `app/best-practices.md` for app architecture and implementation patterns.
  - Read `app/glossary.md` before naming or changing app-local concepts, states, or user-facing nouns.
  - Read `docs/third_party/` when working with third-party package APIs or patterns that already have synced upstream docs.

- Architecture:
  - Treat `app/` as an Electrobun desktop app with a Bun-owned host layer and a frontend-heavy TypeScript webview.
  - Put desktop integrations behind the Electrobun RPC bridge instead of importing host APIs directly into UI code.
  - Prefer shared host adapters over ad hoc browser-to-host calls so new desktop capabilities follow one transport boundary.

- Testing:
  - Do not add automated tests for `app/`.

- Verification:
  - Run formatting after modifying app files.
