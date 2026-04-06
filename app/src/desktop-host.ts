import type { GoddardSdk } from "@goddard-ai/sdk"
import { Electroview } from "electrobun/view"
import { goddardSdk } from "./sdk.ts"
import type {
  AppDesktopRpc,
  DaemonRequestName,
  DaemonRequestPayload,
  DaemonRequestResponse,
  DaemonSendInput,
  RuntimeInfo,
} from "~/shared/desktop-rpc.ts"

const rpc = Electroview.defineRPC<AppDesktopRpc>({
  // Native dialogs and host-side daemon work can legitimately outlive Electrobun's
  // default 1s request timeout, so the app bridge must wait for the host response.
  maxRequestTime: Infinity,
  handlers: {
    requests: {},
    messages: {},
  },
})

let electroview: Electroview<typeof rpc> | undefined

/** Browser-facing desktop bridge methods used by the app and manual smoke checks. */
export interface DesktopHostBridge {
  /** Returns the active desktop runtime reported by the Bun host. */
  getRuntimeInfo(): Promise<RuntimeInfo>

  /** Opens one native directory picker and returns the chosen project root when present. */
  browseForProject(): Promise<string | null>

  /** Maximizes the active desktop window through the Bun host bridge. */
  maximizeWindow(): Promise<void>

  /** Forwards one daemon IPC request through the Bun host's default daemon client. */
  daemonSend<Name extends DaemonRequestName>(
    name: Name,
    payload: DaemonRequestPayload<Name>,
  ): Promise<DaemonRequestResponse<Name>>

  /** Shared SDK instance backed by the Bun-owned daemon client bridge. */
  sdk: GoddardSdk
}

declare global {
  interface Window {
    __goddardDesktop: DesktopHostBridge
  }
}

/** Creates the Electrobun view bridge once for the active browser context. */
export function initializeDesktopHost(): void {
  if (electroview) {
    return
  }

  electroview = new Electroview({ rpc })
}

/** Returns one runtime handshake from the Bun host. */
export async function getRuntimeInfo(): Promise<RuntimeInfo> {
  return await rpc.request.runtimeInfo({})
}

/** Opens one native directory picker for project selection. */
export async function browseForProject(): Promise<string | null> {
  const response = await rpc.request.browseForProject({})
  return response.path
}

/** Maximizes the active desktop window through the Bun host. */
export async function maximizeWindow(): Promise<void> {
  await rpc.request.maximizeWindow({})
}

/** Forwards one daemon IPC request through the Bun host. */
export async function daemonSend<Name extends DaemonRequestName>(
  name: Name,
  payload: DaemonRequestPayload<Name>,
): Promise<DaemonRequestResponse<Name>> {
  const input: DaemonSendInput<Name> = { name, payload }
  return (await rpc.request.daemonSend(input)) as DaemonRequestResponse<Name>
}

/** Shared browser-side desktop host adapter for the current webview. */
export const desktopHost: DesktopHostBridge = {
  getRuntimeInfo,
  browseForProject,
  maximizeWindow,
  daemonSend,
  sdk: goddardSdk,
}
