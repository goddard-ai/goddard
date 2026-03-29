# App Contributing

- Scope:
  - These rules apply to work in `app/`.

- Read before substantial app work:
  - Read `app/best-practices.md` for app architecture and implementation patterns.
  - Read `app/glossary.md` before naming or changing app-local concepts, states, or user-facing nouns.
  - Read `app/docs/third_party/` when working with third-party package APIs or patterns that already have synced upstream docs.

- Architecture:
  - Treat `app/` as a Tauri desktop app with a frontend-heavy TypeScript implementation.
  - Prefer TypeScript changes over Rust changes. Only touch Rust for configuration or unavoidable host integration.
  - Prefer official Tauri plugins before custom host code when they fit the need.

- Testing:
  - Do not add automated tests for `app/`.

- Verification:
  - Run formatting after modifying app files.
