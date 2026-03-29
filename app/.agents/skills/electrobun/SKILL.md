---
name: electrobun
description: Build, refactor, debug, and review Electrobun desktop apps that use Bun main-process code, BrowserWindow or BrowserView, typed RPC, Electroview, electrobun-webview, menus, tray, clipboard, dialogs, sessions, global shortcuts, build config, or shutdown hooks. Use when Codex must write or inspect Electrobun-specific code and avoid Electron-specific APIs or architecture.
---

# electrobun

Build Electrobun apps with Electrobun-native APIs and patterns. Preserve the split between the Bun runtime and browser views, and use the local playgrounds as the fastest source of concrete examples.

## Start Here

- Confirm whether the code runs in the Bun process or a browser view before editing.
- Search `./playgrounds` for the closest example before inventing a pattern. Use [playground-map.md](./references/playground-map.md) to jump to the right folder.
- Use `views://` URLs for bundled HTML, scripts, styles, images, and preload assets.
- Instantiate `new Electroview({ rpc })` in views that use RPC, window controls, or draggable regions.
- Keep shared RPC contracts in a shared types module when both sides need typed requests or messages.

## Preserve Electrobun Boundaries

- Import Bun-side APIs from `electrobun/bun`.
- Import browser-side APIs from `electrobun/view`.
- Use `BrowserWindow` and `BrowserView` for window creation and Bun-driven webview control.
- Use `electrobun-webview` for embedded content. Do not substitute Electron's `webview` tag or `ipcMain` and `ipcRenderer` patterns.
- Relay browser-to-browser communication through Bun. Do not design direct browser-to-browser RPC.
- Prefer `Electrobun.events.on("before-quit", ...)` for shutdown cleanup. Do not rely on `process.on("beforeExit")`.

## Keep A Stable App Shape

- Keep main-process code under a Bun-oriented entrypoint such as `src/bun/index.ts`.
- Keep browser views under `src/views/...`, with one folder per window or view surface when practical.
- Keep shared RPC schemas and other cross-boundary types under `src/shared/...`.
- Keep app metadata, renderer defaults, copied assets, and build hooks in `electrobun.config.ts`.

## Reach For The Native API First

- Use `BrowserWindow` for top-level windows, custom chrome, transparency, resize and move events, and default webview access.
- Use `BrowserView.defineRPC(...)` with `Electroview.defineRPC(...)` for typed requests and messages in both directions.
- Use `Utils` for notifications, file dialogs, reveal or open helpers, clipboard access, and quitting.
- Use `Session.fromPartition(...)` and webview `partition` attributes when content needs isolated or shared storage.
- Use `GlobalShortcut.register(...)` for system-wide shortcuts and clean them up on teardown.
- Use `Tray`, `ContextMenu`, and `ApplicationMenu` for native menus. Check Linux support before depending on context menus or application menus there.
- Use `BuildConfig.get()` and `Utils.paths` instead of hardcoding renderer assumptions or OS paths.
- Use sandboxed webviews plus explicit navigation rules when loading untrusted external content.
- Prefer CEF when a feature needs Chromium-only behavior or strong cross-platform consistency. Expect Linux to favor CEF and avoid mixed renderers there.

## Reuse Local Playgrounds Deliberately

- Inspect both `index.ts` and the paired HTML or CSS files. Several playgrounds keep the important behavior inline in HTML instead of TypeScript.
- Copy patterns, not whole playgrounds. Strip demo-only UI, logging, and long RPC timeouts unless the production case truly needs them.
- Search with focused patterns such as `rg -n "defineRPC|before-quit|partition|host-message|toggleTransparent|GlobalShortcut|ApplicationMenu|Tray|openFileDialog" ./playgrounds`.
- Prefer the closest playground over generic Electron examples whenever behavior is unclear.

## Keep These Electrobun Details In Mind

- Use `window.__electrobunWindowId` and `window.__electrobunWebviewId` when a browser view needs native identity.
- Apply `electrobun-webkit-app-region-drag` and `electrobun-webkit-app-region-no-drag` classes for custom titlebars and draggable regions.
- Keep `renderer`, `sandbox`, `partition`, `preload`, and navigation rules explicit when they affect security or platform behavior.
- Clean up window maps, shortcut registrations, tray instances, and long-lived listeners.
- Confirm whether a feature is platform-specific before promising it on Linux, macOS, or Windows.
