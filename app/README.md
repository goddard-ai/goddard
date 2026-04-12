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
- The Electrobun `preBuild` step stages the standalone Goddard daemon runtime and bundled `serviceman` assets into app resources for both packaged builds and `electrobun dev --watch`.

## Embedded Daemon Runtime

- The desktop host always ensures the app-managed daemon runtime is installed and healthy before it opens the main window.
- During development, `pnpm --dir app run dev` starts Vite plus `electrobun dev --watch`. Browser-only changes stay in the Vite HMR loop, while Bun host changes and watched daemon changes rebuild and relaunch the desktop host.
- The Electrobun watch config includes `../core/daemon/src`, `../core/daemon/scripts`, and `../core/daemon/package.json`, so daemon edits trigger a fresh `preBuild`.
- Each `preBuild` reruns the standalone daemon build, re-stages the bundled `serviceman` payload, and copies the result into Electrobun resources.
- On launch, the host installs or updates the runtime under `~/.goddard/desktop-runtime`, registers the user-scoped service, waits for the daemon health check to pass, and only then opens the app window.
- If the installed runtime hash already matches and the daemon is healthy, startup reuses the existing install without restarting it.
- In the Electrobun `dev` channel, the bundled daemon defaults to `http://127.0.0.1:8787` unless `GODDARD_BASE_URL` is set.

## Full-Stack QA Flow

- Use `pnpm --dir app run dev` for full-stack QA. Running Vite by itself does not exercise daemon embedding, install or update behavior, or host startup gating.
- Start the backend the daemon should talk to before launching the app. By default the dev daemon expects `http://127.0.0.1:8787`; set `GODDARD_BASE_URL` first if you need a different backend.
- Make frontend changes in `app/src` and validate them through Vite HMR.
- Make Bun host changes in `app/src/bun` or daemon changes in `core/daemon`, then let Electrobun watch rebuild and relaunch the desktop app. That rebuild reruns `preBuild`, so daemon changes are recompiled and re-embedded automatically.
- Relaunch validation is enough for the normal update path: if the daemon runtime hash changed, the relaunched app installs the new runtime before opening; if it did not change, the app reuses the healthy installed daemon.
- For fresh-install or upgrade-from-scratch QA, remove `~/.goddard/desktop-runtime` before launching the app again.

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
