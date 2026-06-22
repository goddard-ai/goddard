import {
  BrowserDaemonAuthorizationError,
  createBrowserDaemonIpcClient,
} from "@goddard-ai/daemon-client/browser"
import type { GoddardClient } from "@goddard-ai/sdk"

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
  })
}

async function resolveDaemonAccess(): Promise<DaemonAccess> {
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
