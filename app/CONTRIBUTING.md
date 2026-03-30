# App Contributing

- Scope:
  - These rules apply to work in `app/`.

- Read before substantial app work:
  - Read `app/best-practices.md` for app architecture and implementation patterns.
  - Read `app/glossary.md` before naming or changing app-local concepts, states, or user-facing nouns.
  - Read the relevant upstream package or platform docs when working with third-party APIs or patterns.

- Architecture:
  - Treat `app/` as an Electrobun desktop app with a Bun-owned host layer and a frontend-heavy TypeScript webview.
  - Put desktop integrations behind the Electrobun RPC bridge instead of importing host APIs directly into UI code.
  - Prefer shared host adapters over ad hoc browser-to-host calls so new desktop capabilities follow one transport boundary.
  - In `app/src/components`, keep feature components and their sigma state modules together inside feature folders. Use Pascal-cased file names, add an optional `state/` directory when needed, and do not add barrel modules. Core shell files such as `AppShell` may stay at the top level of `src/components`.

- Syntax and naming:
  - Prefer the `class` JSX prop over `className`.
  - Do not end `preact-sigma` module names with `State`.
  - Prefer app nouns that match `app/glossary.md`; use `project` for user-added local roots unless a feature specifically requires a git repository.
  - Do not use rollout labels such as `sprint-1` in API descriptions, code-facing comments, or other durable implementation naming.
  - Prefer Preact Context over prop drilling `preact-sigma` instances through component trees.
  - Never destructure component props; read them from the `props` object.
  - Define component prop types inline instead of creating `Props` aliases or interfaces.
  - Use inline `css(...)` calls by default; only hoist a Panda class when the same class value is reused in multiple places.
  - When a sigma instance helper type is essentially `InstanceType<typeof MySigma>`, export it as a same-name interface: `export interface MySigma extends InstanceType<typeof MySigma> {}`.

- Testing:
  - Do not add automated tests for `app/`.

- Verification:
  - Run formatting after modifying app files.
