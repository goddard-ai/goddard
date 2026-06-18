import type { BrowserWindow } from "electrobun/bun"

import type { appRpc } from "./rpc.ts"

let mainWindow: BrowserWindow<typeof appRpc> | null = null
let didShowMainWindow = false

/** Stores the active primary Electrobun window for Bun-side RPC handlers. */
export function setMainWindow(window: BrowserWindow<typeof appRpc> | null): void {
  mainWindow = window
  didShowMainWindow = false
}

/** Returns the active primary Electrobun window when one exists. */
export function getMainWindow(): BrowserWindow<typeof appRpc> | null {
  return mainWindow
}

/** Shows the primary Electrobun window at most once after the renderer is ready. */
export function showMainWindow(): boolean {
  if (!mainWindow || didShowMainWindow) {
    return false
  }

  didShowMainWindow = true
  mainWindow.show()
  return true
}
