import { BrowserWindow, GlobalShortcut, Screen, type WindowOptionsType } from "electrobun/bun"

import type { appRpc } from "./rpc.ts"
import { createGlobalSessionLaunchOverlayHost } from "./global-session-launch-overlay-core.ts"

/** Owns Bun-side native primitives for the global session launch overlay. */
export function createElectrobunGlobalSessionLaunchOverlayHost(options: {
  getOverlayUrl: () => string
  onShortcut: () => void
  rpc: typeof appRpc
}) {
  return createGlobalSessionLaunchOverlayHost(
    options,
    {
      createWindow: (input) =>
        new BrowserWindow(input as Partial<WindowOptionsType<typeof appRpc>>),
      globalShortcut: GlobalShortcut,
      screen: Screen,
    },
  )
}
