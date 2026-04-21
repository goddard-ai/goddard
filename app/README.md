# Goddard App

The desktop app now runs on Electrobun with a Bun-owned host layer and a Preact webview.

Unless stated otherwise, commands below are run from `./app`.

## Prerequisites

- Bun
- Run `bun install`.

## Development

- Run `bun run dev` from the workspace root for the full-stack desktop dev loop.
- If you run `bun run dev` from `./app`, start `../core/daemon` separately first.
- Run `bun run build` to produce a packaged local Electrobun build.
- Run `bun run build:stable` to produce release artifacts in `app/artifacts`.
- Run `bun run typecheck` to typecheck both the browser code and the Bun host code.
- The Electrobun `preBuild` step stages the standalone Goddard daemon runtime and bundled `serviceman` assets into app resources for packaged builds. Development reuses a separately watched daemon instead of embedding it.

## Embedded Daemon Runtime

- Outside development, the desktop host ensures the app-managed daemon runtime is installed and healthy before it opens the main window.
- During development, the Bun host waits for the separately watched daemon to answer IPC health checks before it opens the main window.
- During development, the workspace `bun run dev` flow starts `core/daemon` in watch mode, then launches Vite plus `electrobun dev --watch`.
- Browser-only changes stay in the Vite HMR loop, Bun host changes relaunch the desktop host, and daemon changes restart the watched daemon without rebuilding embedded app resources.
- Packaged builds still rerun `preBuild`, rebuild the standalone daemon payload, re-stage the bundled `serviceman` files, and copy the result into Electrobun resources.
- On launch, the host installs or updates the runtime under `~/.goddard/desktop-runtime`, registers the user-scoped service, waits for the daemon health check to pass, and only then opens the app window.
- If the installed runtime hash already matches and the daemon is healthy, startup reuses the existing install without restarting it.
- In the Electrobun `dev` channel, the bundled daemon defaults to `http://127.0.0.1:8787` unless `GODDARD_BASE_URL` is set.

## Full-Stack QA Flow

- Use `bun run dev` for full-stack QA. Running Vite by itself does not exercise daemon embedding, install or update behavior, or host startup gating.
- Start the backend the daemon should talk to before launching the app. By default the dev daemon expects `http://127.0.0.1:8787`; set `GODDARD_BASE_URL` first if you need a different backend.
- Make frontend changes in `app/src` and validate them through Vite HMR.
- Make Bun host changes in `app/src/bun` and let Electrobun watch rebuild and relaunch the desktop app.
- Make daemon changes in `core/daemon` and let the daemon watcher restart the source daemon process.
- For packaged update-path validation, relaunch validation is enough: if the daemon runtime hash changed, the relaunched app installs the new runtime before opening; if it did not change, the app reuses the healthy installed daemon.
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
