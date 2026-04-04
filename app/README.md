# Goddard App

The desktop app now runs on Electrobun with a Bun-owned host layer and a Preact webview.

## Prerequisites

- Bun
- `pnpm install`

## Development

- Run `pnpm --dir app run dev` to start the Electrobun desktop app.
- Run `pnpm --dir app run build` to produce a packaged local Electrobun build.
- Run `pnpm --dir app run build:stable` to produce release artifacts in `app/artifacts`.
- Run `pnpm --dir app run typecheck` to typecheck both the browser code and the Bun host code.

## Releases

- Pushing a `v*` git tag triggers `.github/workflows/release-app.yml`.
- The workflow runs native Electrobun `stable` builds on macOS, Windows, and Linux, then uploads `app/artifacts/*` into the matching GitHub Release.
- Before publishing a new stable release, the workflow also copies forward older `.patch` assets from the previous latest release so Electrobun `bsdiff` updates remain available across versions.

## Host Model

- `app/src/bun/index.ts` owns the desktop host runtime.
- Browser code talks to the host through Electrobun RPC rather than Tauri APIs.
- Read the relevant upstream platform docs directly when app work depends on third-party APIs or platform constraints.

## License

This project is licensed under the [GNU Affero General Public License v3.0 (AGPLv3)](./LICENSE-AGPLv3).
