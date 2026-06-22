import { createDaemonIpcClient, type DaemonIpcClient } from "@goddard-ai/daemon-client/node"

import type { DaemonWebviewAccessInput } from "~/shared/desktop-rpc.ts"
import { ensureDaemonRuntime } from "./daemon-runtime.ts"
import { createClientIpcLogHook } from "./ipc-client-logging.ts"

let daemonClient: DaemonIpcClient | undefined
let daemonUrl: string | undefined

/** Reuses one daemon IPC client for the Bun host process. */
async function getDaemonClient() {
  if (daemonClient) {
    return daemonClient
  }

  const runtime = await ensureDaemonRuntime()
  const client = createDaemonIpcClient({
    ...runtime,
    ipcHook: createClientIpcLogHook(),
  })
  daemonUrl = runtime.daemonUrl
  daemonClient = client
  return client
}

/** Issues a short-lived daemon browser-access token for the active desktop webview origin. */
export async function daemonWebviewAccess(input: DaemonWebviewAccessInput) {
  const client = await getDaemonClient()
  const token = await client.daemon.browserAccess.webviewToken.create({
    origin: input.origin,
  })

  return {
    daemonUrl: daemonUrl ?? (await ensureDaemonRuntime()).daemonUrl,
    ...token,
  }
}
