import {
  BrowserDaemonAuthorizationError,
  createBrowserDaemonIpcClient,
} from "@goddard-ai/daemon-client/browser"
import type { GoddardClient } from "@goddard-ai/sdk"

import { formatClientIpcLogEvent } from "./lib/ipc-log-event.ts"
import { writeRendererDebug } from "./lib/renderer-log-capture.ts"

const browserDaemonUrlKey = "goddard.daemonUrl"
const browserDaemonTokenKey = "goddard.daemonBrowserToken"

type DaemonAccess = {
  daemonUrl: string
  token: string
}

/** Browser-safe daemon client that uses direct loopback IPC instead of Bun request proxying. */
export const browserDaemonClient = createBrowserDaemonClient() as GoddardClient

/** Creates an isolated direct daemon client proxy for browser or desktop webview runtimes. */
export function createBrowserDaemonClient(): GoddardClient {
  return createBrowserDaemonIpcClient({
    access: resolveDaemonAccess,
    ipcHook(event) {
      const { message, properties } = formatClientIpcLogEvent(event)
      writeRendererDebug("ipc.client", message, properties)
    },
  })
}

async function resolveDaemonAccess(): Promise<DaemonAccess> {
  // Do not import from ~/desktop-host.ts here; the app may be running in an actual browser.
  const desktopBridge = globalThis.window?.__goddardDesktop
  if (desktopBridge) {
    const access = await desktopBridge.createDaemonWebviewAccessToken(window.location.origin)
    return {
      daemonUrl: access.daemonUrl,
      token: access.token,
    }
  }

  const daemonUrl = window.localStorage.getItem(browserDaemonUrlKey)
  const token = window.localStorage.getItem(browserDaemonTokenKey)
  if (!daemonUrl || !token) {
    throw new BrowserDaemonAuthorizationError("Browser daemon access is not paired.", 403)
  }

  return {
    daemonUrl,
    token,
  }
}
