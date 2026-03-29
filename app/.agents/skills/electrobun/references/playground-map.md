# Playground Map

Use this file to pick the closest working example before editing Electrobun code.

## Window Chrome And Window Events

- `./playgrounds/custom-titlebar/`
  Browser controls wired through `Electroview` RPC. Open this when implementing close, minimize, maximize, or a custom titlebar.
- `./playgrounds/draggable/`
  Minimal draggable-region setup. Open this when only the drag classes matter.
- `./playgrounds/transparent-window/`
  Transparent and borderless window behavior plus draggable floating elements.
- `./playgrounds/window-events-move-resize/`
  Bun-to-view updates for window move and resize events.
- `./playgrounds/window-events-blur-focus/`
  Bun-to-view updates for blur and focus events.
- `./playgrounds/multiwindow-cef/`
  Multi-window flow with CEF plus an embedded webview readiness signal.

## Embedded Webviews And External Content

- `./playgrounds/webviewtag/index.html`
  The most complete `electrobun-webview` example. Use it for navigation, find-in-page, DevTools, inline `html`, inline `preload`, masks, transparency, passthrough, hidden state, and `views://` asset loading.
- `./playgrounds/webview-settings/index.html`
  Dynamic `electrobun-webview` creation plus runtime toggles for `transparent`, `passthrough`, and `hidden`.
- `./playgrounds/host-message/index.html`
  Parent-view handling for `host-message` plus `window.__electrobunSendToHost(...)` from preload logic inside the embedded page.
- `./playgrounds/session/`
  Partition behavior. Compare the default partition with `persist:*` and `temp:*` partitions by watching `localStorage` stay shared or isolated.
- `./playgrounds/webview-cleanup/`
  Drive a browser animation from Bun-sent cursor and window data.
- `./playgrounds/wgpu-tag/`
  `electrobun-wgpu` lifecycle, resize syncing, masking, and transparent or passthrough toggles.

## Native Integrations

- `./playgrounds/clipboard/`
  Clipboard read and write flows through view-side RPC.
- `./playgrounds/file-dialog/`
  File dialog options, long-running request timeout, and result history handling.
- `./playgrounds/shortcuts/`
  Register, unregister, and observe global shortcuts from a view.
- `./playgrounds/tray/`
  Create, update, and remove a tray icon, including menu and submenu cases.
- `./playgrounds/context-menu/`
  Context-menu roles, custom actions, custom data payloads, disabled items, and submenus.
- `./playgrounds/application-menu/`
  Application-menu roles, accelerators, nested submenus, and menu swapping.

## Shutdown And Lifecycle

- `./playgrounds/quit-test/`
  Surface shutdown progress back into a view while Bun handles `before-quit`.

## Reading Strategy

- Open `index.ts` first when you need `Electroview` or RPC wiring.
- Open `index.html` next when the view owns the interesting behavior, especially for `electrobun-webview`, preload snippets, or inline controls.
- Open `index.css` when drag regions, transparent layouts, or native-feeling interaction depend on styling.
