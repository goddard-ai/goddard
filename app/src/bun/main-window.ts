import type { BrowserWindow } from "electrobun/bun"
import type { appRpc } from "./rpc.ts"

let mainWindow: BrowserWindow<typeof appRpc> | null = null

/** Stores the active primary Electrobun window for Bun-side RPC handlers. */
export function setMainWindow(window: BrowserWindow<typeof appRpc> | null): void {
  mainWindow = window
}

/** Returns the active primary Electrobun window when one exists. */
export function getMainWindow(): BrowserWindow<typeof appRpc> | null {
  return mainWindow
}
