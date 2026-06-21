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

type DirectDaemonClientState = {
  daemonUrl: string
  client: GoddardClient
}

type BrowserDaemonClientState = {
  accessPromise: Promise<DaemonAccess> | undefined
  directClient: DirectDaemonClientState | undefined
}

/** Browser-safe daemon client that uses direct loopback IPC instead of Bun request proxying. */
export const browserDaemonClient = createBrowserDaemonClient() as GoddardClient

/** Creates an isolated direct daemon client proxy for browser or desktop webview runtimes. */
export function createBrowserDaemonClient(): GoddardClient {
  const state: BrowserDaemonClientState = {
    accessPromise: undefined,
    directClient: undefined,
  }

  return new Proxy(
    {},
    {
      get(_target, property) {
        return createBrowserRouteNode(state, [String(property)])
      },
    },
  ) as GoddardClient
}

function createBrowserRouteNode(state: BrowserDaemonClientState, path: readonly string[]): unknown {
  const route = async (payload: unknown = undefined, options?: { signal?: AbortSignal }) =>
    await invokeBrowserRoute(state, path.join("."), payload, options)

  return new Proxy(route, {
    get(_target, property) {
      if (property === "then") {
        return undefined
      }

      return createBrowserRouteNode(state, [...path, String(property)])
    },
  })
}

async function invokeBrowserRoute(
  state: BrowserDaemonClientState,
  name: string,
  payload: unknown,
  options: { signal?: AbortSignal } | undefined,
) {
  const client = await getDirectDaemonClient(state)
  try {
    return await selectRouteFunction(client, name)(payload, options)
  } catch (error) {
    if (!canRefreshDesktopAccess(error)) {
      throw error
    }

    const refreshedClient = await getDirectDaemonClient(state, { refreshAccess: true })
    return await selectRouteFunction(refreshedClient, name)(payload, options)
  }
}

async function getDirectDaemonClient(
  state: BrowserDaemonClientState,
  options: { refreshAccess?: boolean } = {},
) {
  const access = await getDaemonAccess(state, options)
  if (state.directClient?.daemonUrl === access.daemonUrl) {
    return state.directClient.client
  }

  const client = createBrowserDaemonIpcClient({
    daemonUrl: access.daemonUrl,
    token: async () => (await getDaemonAccess(state)).token,
  }) as GoddardClient
  state.directClient = {
    daemonUrl: access.daemonUrl,
    client,
  }
  return client
}

function getDaemonAccess(
  state: BrowserDaemonClientState,
  options: { refreshAccess?: boolean } = {},
) {
  if (!state.accessPromise || options.refreshAccess) {
    state.accessPromise = resolveDaemonAccess()
  }

  return state.accessPromise
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

function canRefreshDesktopAccess(error: unknown) {
  return (
    error instanceof BrowserDaemonAuthorizationError && Boolean(globalThis.window?.__goddardDesktop)
  )
}

function selectRouteFunction(client: Record<string, any>, name: string) {
  let node: unknown = client
  for (const segment of name.split(".")) {
    if (!node || typeof node !== "object" || !(segment in node)) {
      throw new Error(`Unknown daemon IPC route: ${name}`)
    }
    node = (node as Record<string, unknown>)[segment]
  }

  if (typeof node !== "function") {
    throw new Error(`Daemon IPC route is not callable: ${name}`)
  }

  return node
}
